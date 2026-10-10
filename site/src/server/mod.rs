//! The web server (Axum): pages in three languages rendered with active, the
//! documentation search index, the PWA manifest and service worker, static files.
//!
//! * binds `0.0.0.0:$PORT` (default 8080) and shuts down gracefully on SIGTERM, which is
//!   how Heroku stops dynos; `GET /healthz` answers `ok`;
//! * static files are read once at startup (`SITE_DIR`: `static/` and `pkg/`), served from
//!   memory, compressed once, and cached forever when the URL carries their `?v=` hash;
//! * security headers on every response, and a Content-Security-Policy on pages.

pub mod assets;
pub mod docs;
pub mod export;
pub mod layout;
pub mod markdown;
pub mod pages;
pub mod pwa;
pub mod security;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path as Param, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};

use crate::routes::{Lang, Page, Route};
use assets::Assets;
use docs::Docs;
use layout::Ctx;

pub struct Site {
    pub docs: Docs,
    pub assets: Assets,
    /// Public address if configured (`SITE_URL`), else taken from each request.
    pub site_url: Option<String>,
}

impl Site {
    /// Reads the static files and the browser bundle from `dir` (the `site/` folder) and
    /// renders the documentation; the public address comes from `SITE_URL`.
    pub fn load(dir: &Path) -> Site {
        Site::load_with(dir, std::env::var("SITE_URL").ok())
    }

    /// Like [`load`](Site::load), with the public address given (`None`: taken from each
    /// request). The base path (`routes::set_base`) is appended to it if missing.
    pub fn load_with(dir: &Path, site_url: Option<String>) -> Site {
        let mut assets = Assets::default();
        assets.load(&dir.join("static"), "/static");
        assets.load(&dir.join("pkg"), "/pkg");
        Site {
            docs: Docs::load(),
            assets,
            site_url: site_url.map(|u| u.trim_end_matches('/').to_string()).filter(|u| !u.is_empty()).map(|u| {
                let base = crate::routes::base();
                if u.ends_with(base) { u } else { format!("{u}{base}") }
            }),
        }
    }

    /// Every page of the site, in every language.
    pub fn routes(&self) -> Vec<Route> {
        let mut routes = Vec::new();
        for lang in Lang::ALL {
            for page in [Page::Home, Page::Features, Page::Performance, Page::Playground, Page::Docs] {
                routes.push(Route::new(lang, page));
            }
            routes.extend(crate::docs::slugs().iter().map(|s| Route::new(lang, Page::Doc(s.to_string()))));
        }
        routes
    }

    /// The public origin, with the base path (`https://example.com/Memoire`).
    pub fn origin(&self, headers: &HeaderMap) -> String {
        if let Some(url) = &self.site_url {
            return url.clone();
        }
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.split(',').next().unwrap_or("").trim().to_string())
        };
        let host = header("x-forwarded-host").or_else(|| header("host")).unwrap_or_else(|| "localhost".into());
        if !host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':')) {
            return "http://localhost".into();
        }
        let proto = if header("x-forwarded-proto").as_deref() == Some("https") { "https" } else { "http" };
        format!("{proto}://{host}{}", crate::routes::base())
    }
}

/// A cookie's value.
fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get_all(header::COOKIE).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(';')).find_map(|pair| {
        let (key, value) = pair.trim().split_once('=')?;
        (key == name).then_some(value)
    })
}

/// The visitor's language: the last one they read (cookie set by the browser bundle), then
/// `Accept-Language`, English by default.
pub fn pick_lang(headers: &HeaderMap) -> Lang {
    if let Some(lang) = cookie(headers, "memoire-lang").and_then(Lang::parse) {
        return lang;
    }
    let accept = headers.get(header::ACCEPT_LANGUAGE).and_then(|v| v.to_str().ok()).unwrap_or("");
    let mut ranked: Vec<(Lang, f32)> = accept
        .split(',')
        .filter_map(|part| {
            let mut pieces = part.trim().split(";q=");
            let lang = Lang::parse(pieces.next()?)?;
            let q = pieces.next().and_then(|q| q.trim().parse().ok()).unwrap_or(1.0);
            Some((lang, q))
        })
        .filter(|(_, q)| *q > 0.0)
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    ranked.first().map_or(Lang::En, |r| r.0)
}

/// The theme the visitor chose (cookie set by the browser bundle).
fn theme(headers: &HeaderMap) -> Option<&'static str> {
    match cookie(headers, "memoire-theme") {
        Some("light") => Some("light"),
        Some("dark") => Some("dark"),
        _ => None,
    }
}

fn is_https(headers: &HeaderMap) -> bool {
    headers.get("x-forwarded-proto").is_some_and(|v| v == "https")
}

type Shared = State<Arc<Site>>;

/// A redirect to a page in the visitor's language (it depends on the visitor: not cached).
fn to_visitor_lang(headers: &HeaderMap, page: &Page, query: Option<&str>) -> Response {
    let target = Route::new(pick_lang(headers), page.clone()).href();
    let query = query.map(|q| format!("?{q}")).unwrap_or_default();
    (
        StatusCode::FOUND,
        [
            (header::LOCATION, format!("{target}{query}")),
            (header::VARY, "Accept-Language, Cookie".into()),
            (header::CACHE_CONTROL, "no-cache".into()),
        ],
    )
        .into_response()
}

async fn root(headers: HeaderMap, uri: Uri) -> Response {
    to_visitor_lang(&headers, &Page::Home, uri.query())
}

async fn healthz() -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], "ok")
}

fn html(status: StatusCode, html: String) -> Response {
    let csp = security::csp(&html);
    let mut response = (status, html).into_response();
    let h = response.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    h.insert(header::VARY, HeaderValue::from_static("Cookie"));
    if let Ok(value) = HeaderValue::from_str(&csp) {
        h.insert(header::CONTENT_SECURITY_POLICY, value);
    }
    response
}

/// Every page: `/{lang}/…`, the addresses of the previous site (redirected), or a 404.
async fn page(State(site): Shared, uri: Uri, headers: HeaderMap) -> Response {
    let path = uri.path();
    let slugs = crate::docs::slugs();
    let route = match Route::parse(path, slugs) {
        Some(route) => route,
        None => match Page::legacy(path, slugs) {
            Some(page) => return to_visitor_lang(&headers, &page, uri.query()),
            None => Route::new(pick_lang(&headers), Page::NotFound),
        },
    };
    if route.page != Page::NotFound && !path.ends_with('/') {
        let query = uri.query().map(|q| format!("?{q}")).unwrap_or_default();
        return (StatusCode::MOVED_PERMANENTLY, [(header::LOCATION, format!("{path}/{query}"))]).into_response();
    }
    let origin = site.origin(&headers);
    let body = pages::render(&Ctx { site: &site, route: &route, origin: &origin, theme: theme(&headers) });
    let status = if route.page == Page::NotFound { StatusCode::NOT_FOUND } else { StatusCode::OK };
    html(status, body)
}

async fn offline(State(site): Shared, headers: HeaderMap) -> Response {
    html(StatusCode::OK, pages::offline(&site, &site.origin(&headers)))
}

/// `/search/{lang}.txt`: the search index of a language (see [`crate::search::Entry`]).
async fn search_index(State(site): Shared, Param(file): Param<String>) -> Response {
    let Some(lang) = file.strip_suffix(".txt").and_then(|code| Lang::ALL.into_iter().find(|l| l.code() == code)) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=300")],
        site.docs.search_index(lang),
    )
        .into_response()
}

/// `/{lang}/manifest.webmanifest`.
async fn manifest(Param(code): Param<String>) -> Response {
    let Some(lang) = Lang::ALL.into_iter().find(|l| l.code() == code) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    (
        [(header::CONTENT_TYPE, "application/manifest+json"), (header::CACHE_CONTROL, "public, max-age=3600")],
        pwa::manifest(lang).to_json(),
    )
        .into_response()
}

/// `/sw.js`: always revalidated, so that a new deployment reaches visitors at once.
async fn service_worker(State(site): Shared) -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")],
        pwa::service_worker(&site),
    )
        .into_response()
}

async fn sitemap(State(site): Shared, headers: HeaderMap) -> Response {
    let origin = site.origin(&headers);
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\" xmlns:xhtml=\"http://www.w3.org/1999/xhtml\">\n",
    );
    // `/docs/` shows the first page, whose own address is canonical.
    for route in site.routes().into_iter().filter(|r| r.page != Page::Docs) {
        xml.push_str(&format!("<url><loc>{origin}{}</loc>", route.path()));
        for lang in Lang::ALL {
            xml.push_str(&format!(
                r#"<xhtml:link rel="alternate" hreflang="{}" href="{origin}{}"/>"#,
                lang.code(),
                route.with_lang(lang).path()
            ));
        }
        xml.push_str(&format!(
            r#"<xhtml:link rel="alternate" hreflang="x-default" href="{origin}{}"/></url>"#,
            route.with_lang(Lang::En).path()
        ));
        xml.push('\n');
    }
    xml.push_str("</urlset>\n");
    ([(header::CONTENT_TYPE, "application/xml; charset=utf-8")], xml).into_response()
}

async fn robots(State(site): Shared, headers: HeaderMap) -> Response {
    let origin = site.origin(&headers);
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        format!("User-agent: *\nAllow: /\n\nSitemap: {origin}/sitemap.xml\n"),
    )
        .into_response()
}

/// Whether `Accept-Encoding` accepts `coding` (and does not refuse it with `q=0`).
fn accepts(headers: &HeaderMap, coding: &str) -> bool {
    headers.get(header::ACCEPT_ENCODING).and_then(|v| v.to_str().ok()).is_some_and(|accept| {
        accept.split(',').any(|part| {
            let mut pieces = part.trim().split(';');
            pieces.next().is_some_and(|c| c.trim().eq_ignore_ascii_case(coding))
                && !pieces.any(|p| matches!(p.trim(), "q=0" | "q=0.0" | "q=0.00" | "q=0.000"))
        })
    })
}

fn asset(site: &Site, url: &str, uri: &Uri, headers: &HeaderMap) -> Response {
    let Some(asset) = site.assets.get(url) else {
        return (StatusCode::NOT_FOUND, [(header::CACHE_CONTROL, "no-cache")], "Not found").into_response();
    };
    let cache = if uri.query().is_some_and(|q| q.starts_with("v=")) {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=3600"
    };
    let (body, encoding) = match (accepts(headers, "br").then(|| asset.brotli()).flatten(), asset.gzip()) {
        (Some(br), _) => (br, Some("br")),
        (None, Some(gz)) if accepts(headers, "gzip") => (gz, Some("gzip")),
        _ => (&asset.bytes[..], None),
    };
    let mut response = (
        [(header::CONTENT_TYPE, asset.content_type), (header::CACHE_CONTROL, cache), (header::VARY, "Accept-Encoding")],
        body.to_vec(),
    )
        .into_response();
    if let Some(encoding) = encoding {
        response.headers_mut().insert(header::CONTENT_ENCODING, HeaderValue::from_static(encoding));
    }
    response
}

async fn files(State(site): Shared, uri: Uri, headers: HeaderMap) -> Response {
    asset(&site, uri.path(), &uri, &headers)
}

/// Files the previous site served at the root (shared links, crawlers).
async fn root_file(State(site): Shared, uri: Uri, headers: HeaderMap) -> Response {
    asset(&site, &format!("/static{}", uri.path()), &uri, &headers)
}

/// Security headers on every response (pages add their Content-Security-Policy).
async fn secure(request: Request<Body>, next: Next) -> Response {
    let https = is_https(request.headers());
    let mut response = next.run(request).await;
    security::headers(response.headers_mut(), https);
    response
}

/// The application, ready to serve or to test.
pub fn app(site: Arc<Site>) -> Router {
    // Static files are compressed once, in advance; fonts and images are compressed already.
    let compress = DefaultPredicate::new()
        .and(NotForContentType::const_new("font/"))
        .and(NotForContentType::const_new("image/png"));
    Router::new()
        .route("/", get(root))
        .route("/healthz", get(healthz))
        .route("/sw.js", get(service_worker))
        .route("/offline/", get(offline))
        .route("/{lang}/manifest.webmanifest", get(manifest))
        .route("/search/{file}", get(search_index))
        .route("/sitemap.xml", get(sitemap))
        .route("/robots.txt", get(robots))
        .route("/static/{*path}", get(files))
        .route("/pkg/{*path}", get(files))
        .route("/favicon.svg", get(root_file))
        .route("/og-image.png", get(root_file))
        .fallback(get(page))
        .layer(middleware::from_fn(secure))
        .layer(CompressionLayer::new().compress_when(compress))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(tracing::Level::INFO))
                .on_response(DefaultOnResponse::new().level(tracing::Level::INFO)),
        )
        .with_state(site)
}

/// The `site/` folder: `SITE_DIR`, or the crate folder when run with cargo.
pub fn site_dir() -> PathBuf {
    std::env::var_os("SITE_DIR").map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from)
}

/// Ctrl+C locally, SIGTERM on Heroku.
async fn shutdown() {
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {},
                    _ = term.recv() => {},
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received, draining connections");
}

pub async fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        // Plain text when not attached to a terminal (Heroku log drains).
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .init();
    crate::routes::set_base(&std::env::var("BASE_PATH").unwrap_or_default());
    let dir = site_dir();
    let site = Arc::new(Site::load(&dir));
    if site.assets.get("/pkg/memoire_site_bg.wasm").is_none() {
        tracing::warn!(dir = %dir.display(), "pkg/ is missing: the pages will not be interactive (see CONTRIBUTING.md)");
    }
    // Compress the static files in the background, before the first visitors ask.
    let warm = site.clone();
    std::thread::spawn(move || warm.assets.warm());
    let port = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080u16);
    let listener = match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(port, %error, "cannot bind");
            std::process::exit(1);
        }
    };
    tracing::info!("Memoire website on http://localhost:{port}");
    if let Err(error) = axum::serve(listener, app(site)).with_graceful_shutdown(shutdown()).await {
        tracing::error!(%error, "server error");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(*name, HeaderValue::from_static(value));
        }
        map
    }

    #[test]
    fn picks_the_visitor_language() {
        assert_eq!(pick_lang(&headers(&[("accept-language", "de-DE,es;q=0.8,en;q=0.5")])), Lang::Es);
        assert_eq!(pick_lang(&headers(&[("accept-language", "fr-CA")])), Lang::Fr);
        assert_eq!(pick_lang(&headers(&[])), Lang::En);
        let both = headers(&[("accept-language", "es"), ("cookie", "a=1; memoire-lang=fr")]);
        assert_eq!(pick_lang(&both), Lang::Fr);
    }

    #[test]
    fn reads_accept_encoding() {
        assert!(accepts(&headers(&[("accept-encoding", "gzip, deflate, br")]), "br"));
        assert!(!accepts(&headers(&[("accept-encoding", "gzip, br;q=0")]), "br"));
        assert!(!accepts(&headers(&[]), "gzip"));
    }
}
