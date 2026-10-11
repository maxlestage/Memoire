//! The production server, end to end (without the network, through Vitesse's test
//! client): routes, SEO tags, PWA, caching and security headers.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::{Arc, OnceLock};

use memoire_site::server::{Site, app, site_dir};
use vitesse::test::{TestClient, TestResponse};
use vitesse::{StatusCode, header};

/// The site, loaded once for every test.
fn site() -> Arc<Site> {
    static SITE: OnceLock<Arc<Site>> = OnceLock::new();
    SITE.get_or_init(|| Arc::new(Site::load_with(&site_dir(), None))).clone()
}

async fn get(path: &str, headers: &[(&str, &str)]) -> TestResponse {
    let client = TestClient::new(app(site()));
    let mut request = client.get(path).header("host", "example.test");
    for (name, value) in headers {
        request = request.header(name, value);
    }
    request.await
}

async fn text(response: TestResponse) -> String {
    response.text()
}

fn header_of(response: &TestResponse, name: impl AsRef<str>) -> &str {
    response.header(name.as_ref()).unwrap_or("")
}

#[tokio::test]
async fn root_redirects_to_the_visitor_language() {
    let response = get("/", &[("accept-language", "de-DE,es;q=0.8,en;q=0.5")]).await;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(header_of(&response, header::LOCATION), "/es/");
    assert_eq!(header_of(&get("/", &[]).await, header::LOCATION), "/en/");
    let remembered = get("/", &[("accept-language", "es"), ("cookie", "memoire-lang=fr")]).await;
    assert_eq!(header_of(&remembered, header::LOCATION), "/fr/");
}

#[tokio::test]
async fn every_page_has_complete_seo_tags_in_every_language() {
    for lang in ["en", "fr", "es"] {
        for page in ["", "features/", "performance/", "playground/", "docs/", "docs/models/"] {
            let path = format!("/{lang}/{page}");
            let response = get(&path, &[("x-forwarded-proto", "https")]).await;
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert!(header_of(&response, header::STRICT_TRANSPORT_SECURITY).contains("max-age"));
            let html = text(response).await;
            assert!(html.starts_with(&format!(r#"<!DOCTYPE html><html lang="{lang}""#)), "{path}");
            assert_eq!(html.matches("<title>").count(), 1, "{path}");
            assert_eq!(html.matches(r#"<meta name="description""#).count(), 1, "{path}");
            for alt in ["en", "fr", "es", "x-default"] {
                assert!(html.contains(&format!(r#"hreflang="{alt}""#)), "{path} {alt}");
            }
            assert!(html.contains(r#"<meta property="og:image" content="https://example.test/static/og-image.png"/>"#));
            assert!(html.contains(r#"<link rel="canonical" href="https://example.test/"#), "{path}");
            assert!(html.contains(&format!(r#"<link rel="manifest" href="/{lang}/manifest.webmanifest"/>"#)));
            assert!(html.contains(r##"<meta name="theme-color" content="#07070d"/>"##));
            assert!(html.contains(r#"navigator.serviceWorker.register("/sw.js")"#));
            assert!(
                html.contains(r#"as="fetch" type="application/wasm""#),
                "{path}: no browser bundle (build site/pkg)"
            );
        }
    }
}

#[tokio::test]
async fn every_page_ends_with_the_signature_footer() {
    for (lang, rights, here) in [
        ("en", "All rights reserved", "You are here"),
        ("fr", "Tous droits réservés", "Vous êtes ici"),
        ("es", "Todos los derechos reservados", "Estás aquí"),
    ] {
        let html = text(get(&format!("/{lang}/"), &[]).await).await;
        let footer = html.split("<footer").nth(1).expect("a footer");
        // The author's signature, the legal line and the license.
        assert!(footer.contains(r#"class="credits-name""#), "{lang}");
        assert!(footer.contains("Maxime Nathan Lestage"), "{lang}");
        assert!(footer.contains(rights), "{lang}");
        assert!(footer.contains(r#"href="/static/LICENSE.txt""#), "{lang}");
        // No link to GitHub anywhere: the project is not open source.
        assert!(!html.contains("github.com"), "{lang}");
        assert!(!footer.contains("MIT"), "{lang}");
        // The install command and the version of the crate.
        assert!(footer.contains("cargo add memoire-orm"), "{lang}");
        assert!(footer.contains(&format!("v{}", memoire_orm::VERSION)), "{lang}");
        // The six projects of the ecosystem, this one marked as the current page.
        for project in ["Active", "Vitesse", "Securite", "Memoire", "Paquets", "FerroSwift"] {
            assert!(footer.contains(&format!(r#"href="https://maxlestage.github.io/{project}/""#)), "{lang} {project}");
        }
        assert!(footer.contains(r#"aria-current="page""#), "{lang}");
        assert!(footer.contains(here), "{lang}");
    }
    // The author is named in the metadata too.
    let html = text(get("/en/", &[]).await).await;
    assert!(html.contains(r#"<meta name="author" content="Maxime Nathan Lestage"/>"#));
    assert!(html.contains(r#""author":{"@type":"Person","name":"Maxime Nathan Lestage""#));
    // The license is served by the site, identical to the repository's.
    let response = get("/static/LICENSE.txt", &[]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(text(response).await, include_str!("../../LICENSE"));
}

#[tokio::test]
async fn the_home_page_is_rendered_on_the_server() {
    let html = text(get("/fr/", &[]).await).await;
    assert!(html.contains("<title>Memoire ORM — "));
    assert!(html.contains(r#""@type":"SoftwareSourceCode""#));
    assert!(html.contains(r#"<active-island data-island="typing" data-props="fr">"#));
    assert!(html.contains(r#"<active-island data-island="showcase" data-props="fr">"#));
    assert!(html.contains(r#"data-count="22""#), "the number of guides");
    assert!(html.contains(r#"class="lang-btn active" href="/fr/""#));
    assert!(html.contains(r#"<canvas class="particles""#));
}

#[tokio::test]
async fn the_playground_runs_the_orm_on_the_server_first() {
    let html = text(get("/en/playground/", &[]).await).await;
    assert!(html.contains(r#"<active-island data-island="playground" data-props="en">"#));
    // Rows returned by the in-memory engine, and the SQL it generated.
    assert!(html.contains("<td>Alice</td>"));
    assert!(html.contains("users"));
    assert!(html.contains("Executed in"));
    assert!(html.contains(" · server"));
}

#[tokio::test]
async fn docs_pages_render_markdown_with_their_navigation() {
    let response = get("/fr/docs/queries/", &[]).await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(html.contains(r#"<div class="markdown-body"><h1"#));
    assert!(html.contains(r#""@type":"BreadcrumbList""#));
    assert!(html.contains(r#"<meta property="og:type" content="article"/>"#));
    assert!(html.contains(r#"<active-island data-island="docs-sidebar" data-props="fr:queries">"#));
    assert!(html.contains(r#"class="docs-link active" href="/fr/docs/queries/""#) || html.contains("docs-link active"));
    assert!(html.contains(r#"class="toc""#));
    assert!(html.contains(r#"rel="next""#) && html.contains(r#"rel="prev""#));
    // `/docs/` shows the first page; its own address is canonical.
    let index = text(get("/es/docs/", &[]).await).await;
    assert!(index.contains(r#"<link rel="canonical" href="http://example.test/es/docs/introduction/"/>"#));
}

#[tokio::test]
async fn directories_get_a_trailing_slash() {
    let response = get("/en/docs?x=1", &[]).await;
    assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
    assert_eq!(header_of(&response, header::LOCATION), "/en/docs/?x=1");
}

#[tokio::test]
async fn addresses_of_the_previous_site_lead_to_the_visitor_language() {
    let response = get("/docs/models", &[("accept-language", "fr-FR")]).await;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(header_of(&response, header::LOCATION), "/fr/docs/models/");
    assert_eq!(header_of(&get("/playground", &[]).await, header::LOCATION), "/en/playground/");
}

#[tokio::test]
async fn unknown_pages_answer_404_in_the_right_language() {
    let response = get("/es/nope", &[]).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let html = text(response).await;
    assert!(html.contains(r#"<html lang="es""#));
    assert!(html.contains(r#"<meta name="robots" content="noindex"#));
    let unknown = get("/nope", &[("accept-language", "fr")]).await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    assert!(text(unknown).await.contains(r#"<html lang="fr""#));
    assert_eq!(get("/en/docs/nope/", &[]).await.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_theme_chosen_by_the_visitor_is_rendered() {
    let html = text(get("/en/", &[("cookie", "memoire-theme=light")]).await).await;
    assert!(html.contains(r#"data-theme="light""#));
    assert!(html.contains(r##"<meta name="theme-color" content="#f6f6fb"/>"##));
    assert!(!text(get("/en/", &[]).await).await.contains("data-theme="));
}

#[tokio::test]
async fn pages_have_a_content_security_policy_allowing_their_inline_scripts() {
    let response = get("/en/", &[]).await;
    let csp = header_of(&response, header::CONTENT_SECURITY_POLICY).to_string();
    assert_eq!(header_of(&response, header::X_FRAME_OPTIONS), "DENY");
    let html = text(response).await;
    let scripts = memoire_site::server::security::inline_scripts(&html);
    assert!(scripts.len() >= 2, "the WebAssembly loader and the service worker registration");
    assert_eq!(csp.matches("'sha256-").count(), scripts.len());
    assert!(csp.contains("'wasm-unsafe-eval'") && csp.contains("worker-src 'self'"));
}

#[tokio::test]
async fn the_manifest_and_the_service_worker() {
    let response = get("/es/manifest.webmanifest", &[]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_of(&response, header::CONTENT_TYPE), "application/manifest+json");
    let manifest: serde_json::Value = serde_json::from_str(&text(response).await).expect("valid JSON");
    assert_eq!(manifest["name"], "Memoire");
    assert_eq!(manifest["start_url"], "/es/");
    assert_eq!(manifest["scope"], "/");
    assert_eq!(manifest["display"], "standalone");
    assert_eq!(manifest["theme_color"], "#07070d");
    let icons = manifest["icons"].as_array().unwrap();
    for icon in icons {
        let src = icon["src"].as_str().unwrap();
        let file = get(src, &[]).await;
        assert_eq!(file.status(), StatusCode::OK, "{src}");
        assert_eq!(header_of(&file, header::CONTENT_TYPE), icon["type"].as_str().unwrap());
    }
    assert!(icons.iter().any(|i| i["purpose"] == "maskable" && i["sizes"] == "512x512"));
    assert_eq!(get("/de/manifest.webmanifest", &[]).await.status(), StatusCode::NOT_FOUND);

    let response = get("/sw.js", &[]).await;
    assert_eq!(header_of(&response, header::CONTENT_TYPE), "text/javascript; charset=utf-8");
    assert_eq!(header_of(&response, header::CACHE_CONTROL), "no-cache");
    let worker = text(response).await;
    let precache = worker.split("const PRECACHE = ").nth(1).and_then(|r| r.split(";\n").next()).unwrap();
    let urls: Vec<String> = serde_json::from_str(precache).unwrap();
    for wanted in ["/en/", "/fr/playground/", "/es/docs/", "/offline/", "/static/fonts/inter-latin.woff2"] {
        assert!(urls.iter().any(|u| u == wanted), "{wanted} is not precached: {urls:?}");
    }
    assert!(urls.iter().any(|u| u.starts_with("/static/app.css?v=")));
    assert!(urls.iter().any(|u| u.starts_with("/pkg/memoire_site_bg.wasm?v=")));
    assert!(worker.contains(r#"const OFFLINE = "/offline/";"#));
    // Every precached file exists, else the worker would not install.
    for url in &urls {
        assert_eq!(get(url, &[]).await.status(), StatusCode::OK, "{url}");
    }
}

#[tokio::test]
async fn versioned_assets_are_cached_forever_and_compressed() {
    let html = text(get("/en/", &[]).await).await;
    let css = html.split(r#"href="/static/app.css?v="#).nth(1).and_then(|r| r.split('"').next()).unwrap();
    let response = get(&format!("/static/app.css?v={css}"), &[("accept-encoding", "gzip, br")]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(header_of(&response, header::CACHE_CONTROL).contains("immutable"));
    assert_eq!(header_of(&response, header::CONTENT_ENCODING), "br");
    let gzip = get("/static/app.css", &[("accept-encoding", "gzip")]).await;
    assert_eq!(header_of(&gzip, header::CONTENT_ENCODING), "gzip");
    assert_eq!(get("/static/../Cargo.toml", &[]).await.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn sitemap_and_robots_use_absolute_urls() {
    let sitemap = text(get("/sitemap.xml", &[]).await).await;
    assert!(sitemap.contains("<loc>http://example.test/fr/docs/models/</loc>"));
    assert!(sitemap.contains(r#"hreflang="x-default""#));
    assert!(!sitemap.contains("<loc>http://example.test/en/docs/</loc>"));
    let robots = text(get("/robots.txt", &[]).await).await;
    assert!(robots.contains("Sitemap: http://example.test/sitemap.xml"));
    assert_eq!(text(get("/healthz", &[]).await).await, "ok");
}

#[tokio::test]
async fn the_search_index_is_searched_like_in_the_browser() {
    let index = text(get("/search/fr.txt", &[]).await).await;
    let entries: Vec<_> =
        index.lines().map(|l| memoire_site::search::Entry::parse(l).expect("an index line")).collect();
    assert_eq!(entries.len(), memoire_site::docs::page_count());
    let hits = memoire_site::search::search(&entries, "deployer heroku");
    assert!(hits.iter().any(|h| h.entry.slug == "deploy-heroku-mobile"));
    assert_eq!(get("/search/de.txt", &[]).await.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_offline_page_speaks_every_language() {
    let response = get("/offline/", &[]).await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(
        html.contains("You are offline")
            && html.contains("Vous êtes hors ligne")
            && html.contains("Estás sin conexión")
    );
    assert!(html.contains(r#"href="/fr/playground/""#));
}
