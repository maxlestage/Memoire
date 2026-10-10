//! Production static server for the Memoire ORM website.
//!
//! * serves the Trunk output (`STATIC_DIR`, default `./dist`), using pre-compressed
//!   `.br` / `.gz` siblings when they exist and compressing on the fly otherwise;
//! * the prerendered page of each route (`/docs/models` → `docs/models.html`,
//!   written by the site's prerenderer), and `404.html` for unknown routes; a
//!   site built without prerendering gets `index.html` (single-page-app
//!   fallback) instead; asset-like paths get a plain 404;
//! * `GET /healthz` → `ok`;
//! * long-lived immutable caching for hashed assets, `no-cache` for `index.html`;
//! * security headers, including a CSP that allows WebAssembly and pins the inline
//!   scripts of `index.html` by their SHA-256 hash (computed at startup);
//! * binds `0.0.0.0:$PORT` (default 8080) and shuts down gracefully on SIGTERM,
//!   which is how Heroku stops dynos.
//!
//! `memoire-site-server precompress <dir>` writes the `.br` / `.gz` files at build time.

mod precompress;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::handler::Handler;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use base64::Engine;
use sha2::{Digest, Sha256};
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::{error, info, Level};
use tracing_subscriber::EnvFilter;

const CACHE_IMMUTABLE: &str = "public, max-age=31536000, immutable";
const CACHE_NO_CACHE: &str = "no-cache";
const CACHE_SHORT: &str = "public, max-age=3600";

struct AppState {
    index_html: Vec<u8>,
    /// Prerendered pages by route (`/features`, `/docs/models`), loaded at startup.
    pages: HashMap<String, Vec<u8>>,
    /// Prerendered "page not found", when the site was prerendered.
    not_found: Option<Vec<u8>>,
    csp: HeaderValue,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("precompress") {
        let dir = args.get(1).map(String::as_str).unwrap_or("./dist");
        if let Err(err) = precompress::run(std::path::Path::new(dir)) {
            eprintln!("precompress failed: {err}");
            std::process::exit(1);
        }
        return;
    }
    serve();
}

#[tokio::main]
async fn serve() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        // Plain text when not attached to a terminal (Heroku log drains).
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .init();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let static_dir = PathBuf::from(std::env::var("STATIC_DIR").unwrap_or_else(|_| "./dist".into()));

    let index_path = static_dir.join("index.html");
    let index_html = match std::fs::read(&index_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            error!(path = %index_path.display(), %err, "cannot read index.html — did you run `trunk build --release`?");
            std::process::exit(1);
        }
    };
    let csp = build_csp(&String::from_utf8_lossy(&index_html));
    let pages = load_pages(&static_dir);
    let not_found = std::fs::read(static_dir.join("404.html")).ok();
    info!(pages = pages.len(), "prerendered pages loaded");
    let state = Arc::new(AppState {
        index_html,
        pages,
        not_found,
        csp: HeaderValue::from_str(&csp).expect("CSP is a valid header value"),
    });

    let app = router(state, static_dir.clone());

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(err) => {
            error!(%addr, %err, "cannot bind");
            std::process::exit(1);
        }
    };
    info!(%addr, static_dir = %static_dir.display(), "memoire-site-server listening");

    if let Err(err) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        error!(%err, "server error");
        std::process::exit(1);
    }
    info!("server stopped");
}

fn router(state: Arc<AppState>, static_dir: PathBuf) -> Router {
    let spa = spa_fallback.with_state(state.clone());
    let files = ServeDir::new(static_dir)
        .precompressed_br()
        .precompressed_gzip()
        .append_index_html_on_directories(false)
        .fallback(spa);

    Router::new()
        .route("/healthz", get(healthz))
        .route("/", get(index))
        .route("/index.html", get(index))
        .fallback_service(files)
        .layer(middleware::from_fn_with_state(state.clone(), headers_middleware))
        .layer(CompressionLayer::new())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
}

async fn healthz() -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], "ok")
}

async fn index(State(state): State<Arc<AppState>>) -> Response {
    index_response(&state)
}

fn index_response(state: &AppState) -> Response {
    html_response(StatusCode::OK, &state.index_html)
}

fn html_response(status: StatusCode, body: &[u8]) -> Response {
    (
        status,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, CACHE_NO_CACHE),
        ],
        body.to_vec(),
    )
        .into_response()
}

/// Every `*.html` page under `dir` (except `index.html` and `404.html`, served
/// on their own) by route: `docs/models.html` and `docs/models/index.html`
/// both answer `/docs/models`.
fn load_pages(dir: &Path) -> HashMap<String, Vec<u8>> {
    fn walk(dir: &Path, root: &Path, out: &mut HashMap<String, Vec<u8>>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root, out);
            } else if let Some(route) = route_of(&path, root) {
                if let Ok(bytes) = std::fs::read(&path) {
                    out.insert(route, bytes);
                }
            }
        }
    }
    let mut pages = HashMap::new();
    walk(dir, dir, &mut pages);
    pages
}

/// `<root>/docs/models.html` → `/docs/models`; `None` for other files.
fn route_of(path: &Path, root: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?.to_str()?.replace('\\', "/");
    let stem = rel.strip_suffix(".html")?;
    if stem == "index" || stem == "404" {
        return None;
    }
    Some(format!("/{}", stem.strip_suffix("/index").unwrap_or(stem)))
}

/// Called by `ServeDir` when no file matches the path.
async fn spa_fallback(State(state): State<Arc<AppState>>, req: Request) -> Response {
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let path = req.uri().path();
    let route = path
        .strip_suffix('/')
        .filter(|r| !r.is_empty())
        .unwrap_or(path);
    if let Some(page) = state.pages.get(route) {
        return html_response(StatusCode::OK, page);
    }
    let last_segment = path.rsplit('/').next().unwrap_or_default();
    if last_segment.contains('.') {
        // Looks like a file (missing asset): a real 404, not the app shell.
        return (
            StatusCode::NOT_FOUND,
            [(header::CACHE_CONTROL, CACHE_NO_CACHE)],
            "Not found",
        )
            .into_response();
    }
    match &state.not_found {
        Some(page) => html_response(StatusCode::NOT_FOUND, page),
        None => index_response(&state),
    }
}

/// Security headers on every response + cache policy for static files.
async fn headers_middleware(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path().to_owned();
    let mut res = next.run(req).await;
    let headers = res.headers_mut();
    set_security_headers(headers, &state.csp);

    if !headers.contains_key(header::CACHE_CONTROL) {
        let file_name = path.rsplit('/').next().unwrap_or_default();
        let value = if !res.status().is_success() {
            CACHE_NO_CACHE
        } else if is_hashed_asset(file_name) {
            CACHE_IMMUTABLE
        } else {
            CACHE_SHORT
        };
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    }
    res
}

fn set_security_headers(headers: &mut HeaderMap, csp: &HeaderValue) {
    let fixed: [(HeaderName, &'static str); 6] = [
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "strict-origin-when-cross-origin"),
        (header::X_FRAME_OPTIONS, "DENY"),
        (
            HeaderName::from_static("permissions-policy"),
            "camera=(), microphone=(), geolocation=(), payment=()",
        ),
        (
            HeaderName::from_static("cross-origin-opener-policy"),
            "same-origin",
        ),
        (
            header::STRICT_TRANSPORT_SECURITY,
            "max-age=31536000; includeSubDomains",
        ),
    ];
    for (name, value) in fixed {
        headers.insert(name, HeaderValue::from_static(value));
    }
    headers.insert(header::CONTENT_SECURITY_POLICY, csp.clone());
}

/// Trunk names hashed files `name-<16 hex chars>.ext` (or `…_bg.wasm`).
fn is_hashed_asset(file_name: &str) -> bool {
    file_name.match_indices('-').any(|(i, _)| {
        let rest = &file_name[i + 1..];
        rest.len() > 16
            && rest.as_bytes()[..16].iter().all(u8::is_ascii_hexdigit)
            && matches!(rest.as_bytes()[16], b'.' | b'_')
    })
}

/// Content-Security-Policy compatible with WebAssembly. Inline `<script>`s in
/// `index.html` (Trunk's loader, the theme boot script) are allowed by hash.
fn build_csp(index_html: &str) -> String {
    let hashes: Vec<String> = inline_scripts(index_html)
        .into_iter()
        .map(|body| {
            let digest = Sha256::digest(body.as_bytes());
            format!(
                "'sha256-{}'",
                base64::engine::general_purpose::STANDARD.encode(digest)
            )
        })
        .collect();
    info!(inline_scripts = hashes.len(), "CSP built from index.html");
    format!(
        "default-src 'self'; \
         script-src 'self' 'wasm-unsafe-eval' {}; \
         style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; \
         font-src 'self' https://fonts.gstatic.com data:; \
         img-src 'self' data: https:; \
         connect-src 'self'; \
         object-src 'none'; \
         base-uri 'self'; \
         form-action 'self'; \
         frame-ancestors 'none'",
        hashes.join(" ")
    )
}

/// Bodies of `<script>` elements without a `src` attribute.
fn inline_scripts(html: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<script") {
        let after = &rest[start..];
        let Some(tag_end) = after.find('>') else { break };
        let tag = &after[..tag_end];
        let body_start = &after[tag_end + 1..];
        let Some(close) = body_start.find("</script>") else { break };
        if !tag.contains(" src=") {
            out.push(&body_start[..close]);
        }
        rest = &body_start[close + "</script>".len()..];
    }
    out
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    info!("shutdown signal received, draining connections");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_of_prerendered_pages() {
        let root = Path::new("/dist");
        let route = |p: &str| route_of(&root.join(p), root);
        assert_eq!(route("features.html").as_deref(), Some("/features"));
        assert_eq!(route("docs/models.html").as_deref(), Some("/docs/models"));
        assert_eq!(
            route("docs/models/index.html").as_deref(),
            Some("/docs/models")
        );
        assert_eq!(route("docs/index.html").as_deref(), Some("/docs"));
        assert_eq!(route("index.html"), None);
        assert_eq!(route("404.html"), None);
        assert_eq!(route("content/en/faq.md"), None);
    }

    #[test]
    fn hashed_assets() {
        assert!(is_hashed_asset("memoire-site-177a4a8df01dbb63_bg.wasm"));
        assert!(is_hashed_asset("memoire-site-177a4a8df01dbb63.js"));
        assert!(is_hashed_asset("main-20b3c7a1bd4a3efc.css"));
        assert!(!is_hashed_asset("favicon.svg"));
        assert!(!is_hashed_asset("index.html"));
        assert!(!is_hashed_asset("og-image.png"));
    }

    #[test]
    fn finds_inline_scripts() {
        let html = r#"<head><script>a()</script><script src="/x.js"></script><script type="module">b()</script></head>"#;
        assert_eq!(inline_scripts(html), vec!["a()", "b()"]);
        let csp = build_csp(html);
        assert!(csp.contains("'wasm-unsafe-eval'"));
        assert_eq!(csp.matches("'sha256-").count(), 2);
    }
}
