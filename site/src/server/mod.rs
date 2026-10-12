//! The web server (Vitesse): pages in three languages rendered with active, the
//! documentation search index, the PWA manifest and service worker, static files.
//!
//! * binds `0.0.0.0:$PORT` (default 8080) and shuts down gracefully on SIGTERM, which is
//!   how Heroku stops dynos; `GET /healthz` answers `ok`;
//! * static files are read once at startup (`SITE_DIR`: `static/` and `pkg/`), served from
//!   memory, compressed once, and cached forever when the URL carries their `?v=` hash;
//!   pages and search indexes are compressed by Vitesse on the way out;
//! * security headers on every response, and a Content-Security-Policy on pages.

pub mod assets;
pub mod docs;
pub mod export;
pub mod layout;
pub mod markdown;
pub mod pages;
pub mod pwa;
pub mod security;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use vitesse::prelude::*;

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
    pub fn origin(&self, req: &Request) -> String {
        if let Some(url) = &self.site_url {
            return url.clone();
        }
        let header = |name: &str| req.header(name).map(|v| v.split(',').next().unwrap_or("").trim().to_string());
        let host = header("x-forwarded-host").or_else(|| header("host")).unwrap_or_else(|| "localhost".into());
        if !host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':')) {
            return "http://localhost".into();
        }
        let proto = if header("x-forwarded-proto").as_deref() == Some("https") { "https" } else { "http" };
        format!("{proto}://{host}{}", crate::routes::base())
    }
}

/// The visitor's language: the last one they read (cookie set by the browser bundle), then
/// `Accept-Language`, English by default.
pub fn pick_lang(req: &Request) -> Lang {
    lang_of(req.cookie("memoire-lang"), req.header("accept-language").unwrap_or(""))
}

/// [`pick_lang`] on the `memoire-lang` cookie and the `Accept-Language` header.
fn lang_of(cookie: Option<&str>, accept: &str) -> Lang {
    if let Some(lang) = cookie.and_then(Lang::parse) {
        return lang;
    }
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
fn theme(req: &Request) -> Option<&'static str> {
    match req.cookie("memoire-theme") {
        Some("light") => Some("light"),
        Some("dark") => Some("dark"),
        _ => None,
    }
}

fn is_https(req: &Request) -> bool {
    req.header("x-forwarded-proto") == Some("https")
}

/// A redirect to a page in the visitor's language (it depends on the visitor: not cached).
fn to_visitor_lang(req: &Request, page: &Page) -> Response {
    let target = Route::new(pick_lang(req), page.clone()).href();
    let query = req.query_string().map(|q| format!("?{q}")).unwrap_or_default();
    Redirect::to(format!("{target}{query}"))
        .into_response()
        .header("vary", "Accept-Language, Cookie")
        .header("cache-control", "no-cache")
}

fn root(_site: &Site, req: &Request) -> Response {
    to_visitor_lang(req, &Page::Home)
}

fn healthz(_site: &Site, _req: &Request) -> Response {
    res::text("ok").header("cache-control", "no-store")
}

fn html(status: u16, html: String) -> Response {
    let csp = security::csp(&html);
    res::status(status)
        .header("cache-control", "no-cache")
        .header("vary", "Cookie")
        .header("content-security-policy", csp)
        .html(html)
}

/// Every page: `/{lang}/…`, the addresses of the previous site (redirected), or a 404.
fn page(site: &Site, req: &Request) -> Response {
    let path = req.path();
    let slugs = crate::docs::slugs();
    let route = match Route::parse(path, slugs) {
        Some(route) => route,
        None => match Page::legacy(path, slugs) {
            Some(page) => return to_visitor_lang(req, &page),
            None => Route::new(pick_lang(req), Page::NotFound),
        },
    };
    if route.page != Page::NotFound && !path.ends_with('/') {
        let query = req.query_string().map(|q| format!("?{q}")).unwrap_or_default();
        return Redirect::permanent(format!("{path}/{query}")).into_response();
    }
    let origin = site.origin(req);
    let body = pages::render(&Ctx { site, route: &route, origin: &origin, theme: theme(req) });
    let status = if route.page == Page::NotFound { 404 } else { 200 };
    html(status, body)
}

fn offline(site: &Site, req: &Request) -> Response {
    html(200, pages::offline(site, &site.origin(req)))
}

/// `/search/{lang}.txt`: the search index of a language (see [`crate::search::Entry`]).
fn search_index(site: &Site, req: &Request) -> Response {
    let file = req.param("file").unwrap_or("");
    let Some(lang) = file.strip_suffix(".txt").and_then(|code| Lang::ALL.into_iter().find(|l| l.code() == code)) else {
        return res::status(404);
    };
    res::text(site.docs.search_index(lang)).header("cache-control", "public, max-age=300")
}

/// `/{lang}/manifest.webmanifest`.
fn manifest(_site: &Site, req: &Request) -> Response {
    let code = req.param("lang").unwrap_or("");
    let Some(lang) = Lang::ALL.into_iter().find(|l| l.code() == code) else {
        return res::status(404);
    };
    res::send(pwa::manifest(lang).to_json())
        .header("content-type", "application/manifest+json")
        .header("cache-control", "public, max-age=3600")
}

/// `/sw.js`: always revalidated, so that a new deployment reaches visitors at once.
fn service_worker(site: &Site, _req: &Request) -> Response {
    res::send(pwa::service_worker(site))
        .header("content-type", "text/javascript; charset=utf-8")
        .header("cache-control", "no-cache")
}

fn sitemap(site: &Site, req: &Request) -> Response {
    let origin = site.origin(req);
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
    res::send(xml).header("content-type", "application/xml; charset=utf-8")
}

fn robots(site: &Site, req: &Request) -> Response {
    let origin = site.origin(req);
    res::text(format!("User-agent: *\nAllow: /\n\nSitemap: {origin}/sitemap.xml\n"))
}

/// Whether the `Accept-Encoding` header `accept` accepts `coding` (and does not refuse it
/// with `q=0`).
fn accepts(accept: Option<&str>, coding: &str) -> bool {
    accept.is_some_and(|accept| {
        accept.split(',').any(|part| {
            let mut pieces = part.trim().split(';');
            pieces.next().is_some_and(|c| c.trim().eq_ignore_ascii_case(coding))
                && !pieces.any(|p| matches!(p.trim(), "q=0" | "q=0.0" | "q=0.00" | "q=0.000"))
        })
    })
}

/// A static file by its URL, compressed in advance when the browser accepts it. Versioned
/// URLs (`?v=`) never change: browsers keep them a year.
fn asset(site: &Site, url: &str, req: &Request) -> Response {
    let Some(asset) = site.assets.get(url) else {
        return res::status(404).header("cache-control", "no-cache").text("Not found");
    };
    let cache = if req.query_string().is_some_and(|q| q.starts_with("v=")) {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=3600"
    };
    let accept = req.header("accept-encoding");
    let brotli = if accepts(accept, "br") { asset.brotli() } else { None };
    let gzip = if brotli.is_none() && accepts(accept, "gzip") { asset.gzip() } else { None };
    let (body, encoding) = match (brotli, gzip) {
        (Some(br), _) => (br, Some("br")),
        (None, Some(gz)) => (gz, Some("gzip")),
        _ => (&asset.bytes[..], None),
    };
    let response = res::send(body.to_vec())
        .header("content-type", asset.content_type)
        .header("cache-control", cache)
        .header("vary", "Accept-Encoding");
    match encoding {
        Some(encoding) => response.header("content-encoding", encoding),
        None => response,
    }
}

/// `/static/…` and `/pkg/…`.
fn files(site: &Site, req: &Request) -> Response {
    asset(site, req.path(), req)
}

/// Files the previous site served at the root (shared links, crawlers).
fn root_file(site: &Site, req: &Request) -> Response {
    asset(site, &format!("/static{}", req.path()), req)
}

/// Routes a handler that only needs the site and the request.
fn route(app: &mut App, path: &str, site: &Arc<Site>, handler: fn(&Site, &Request) -> Response) {
    let site = site.clone();
    app.get(path, move |req: Request| {
        let site = site.clone();
        async move { handler(&site, &req) }
    });
}

/// The application, ready to serve or to test: pages and indexes compressed for the
/// browsers that accept it, security headers on every response.
pub fn app(site: Arc<Site>) -> App {
    let mut app = App::new();
    app.middleware(middleware::compress());
    app.middleware(|req: Request, next: Next| async move {
        let https = is_https(&req);
        let mut response = next.run(req).await;
        security::headers(response.headers_mut(), https);
        response
    });
    route(&mut app, "/", &site, root);
    route(&mut app, "/healthz", &site, healthz);
    route(&mut app, "/sw.js", &site, service_worker);
    route(&mut app, "/offline", &site, offline);
    route(&mut app, "/:lang/manifest.webmanifest", &site, manifest);
    route(&mut app, "/search/:file", &site, search_index);
    route(&mut app, "/sitemap.xml", &site, sitemap);
    route(&mut app, "/robots.txt", &site, robots);
    route(&mut app, "/static/*path", &site, files);
    route(&mut app, "/pkg/*path", &site, files);
    route(&mut app, "/favicon.svg", &site, root_file);
    route(&mut app, "/og-image.jpg", &site, root_file);
    app.fallback(move |req: Request| {
        let site = site.clone();
        async move { page(&site, &req) }
    });
    app
}

/// The `site/` folder: `SITE_DIR`, or the crate folder when run with cargo.
pub fn site_dir() -> PathBuf {
    std::env::var_os("SITE_DIR").map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from)
}

/// `memoire-site`: the site on `PORT` (Heroku) or 8080, on every core, stopped by Ctrl+C
/// locally and by SIGTERM on Heroku. One log line per request on standard output.
pub fn run() -> io::Result<()> {
    crate::routes::set_base(&std::env::var("BASE_PATH").unwrap_or_default());
    let dir = site_dir();
    let site = Arc::new(Site::load(&dir));
    if site.assets.get("/pkg/memoire_site_bg.wasm").is_none() {
        eprintln!(
            "warning: {} is missing: the pages will not be interactive (see CONTRIBUTING.md)",
            dir.join("pkg").display()
        );
    }
    // Compress the static files in the background, before the first visitors ask.
    let warm = site.clone();
    std::thread::spawn(move || warm.assets.warm());
    let port = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080u16);
    let mut app = app(site);
    app.middleware(middleware::logger());
    println!("Memoire website on http://localhost:{port}");
    app.run(port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_visitor_language() {
        assert_eq!(lang_of(None, "de-DE,es;q=0.8,en;q=0.5"), Lang::Es);
        assert_eq!(lang_of(None, "fr-CA"), Lang::Fr);
        assert_eq!(lang_of(None, ""), Lang::En);
        assert_eq!(lang_of(Some("fr"), "es"), Lang::Fr);
    }

    #[test]
    fn reads_accept_encoding() {
        assert!(accepts(Some("gzip, deflate, br"), "br"));
        assert!(!accepts(Some("gzip, br;q=0"), "br"));
        assert!(!accepts(None, "gzip"));
    }
}
