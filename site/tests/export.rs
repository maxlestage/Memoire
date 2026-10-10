//! The static export for GitHub Pages: every page as a file, links behind the base path.
#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;

use memoire_site::server::export::export;

fn read(dir: &Path, path: &str) -> String {
    std::fs::read_to_string(dir.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[tokio::test]
async fn exports_every_page_behind_the_base_path() {
    let dir = std::env::temp_dir().join(format!("memoire-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let files = export(&dir, "/Memoire/", Some("https://user.github.io".into())).await.expect("export");
    assert!(files > 100, "{files} files");

    // Pages: links behind the base path, absolute URLs on the public address.
    let page = read(&dir, "fr/docs/models/index.html");
    assert!(page.contains(r#"<html lang="fr" data-base="/Memoire">"#), "{}", &page[..200]);
    assert!(page.contains(r#"<link rel="canonical" href="https://user.github.io/Memoire/fr/docs/models/"/>"#));
    assert!(page.contains(r#"href="/Memoire/fr/docs/queries/""#));
    assert!(page.contains(r#"href="/Memoire/static/app.css?v="#));
    assert!(page.contains(r#"<link rel="manifest" href="/Memoire/fr/manifest.webmanifest"/>"#));
    assert!(page.contains(r#"navigator.serviceWorker.register("/Memoire/sw.js")"#));
    assert!(page.contains(r#"import init from "/Memoire/pkg/memoire_site.js"#) || !dir.join("pkg").exists());
    assert!(!page.contains(r#"href="/fr/"#), "a link without the base path");
    for path in ["en/index.html", "es/playground/index.html", "fr/features/index.html", "en/performance/index.html"] {
        assert!(dir.join(path).exists(), "{path}");
    }

    // PWA: the manifest and the worker know the base path.
    let manifest: serde_json::Value = serde_json::from_str(&read(&dir, "es/manifest.webmanifest")).unwrap();
    assert_eq!(manifest["start_url"], "/Memoire/es/");
    assert_eq!(manifest["scope"], "/Memoire/");
    assert!(manifest["icons"].as_array().unwrap().iter().all(|i| i["src"].as_str().unwrap().starts_with("/Memoire/")));
    let worker = read(&dir, "sw.js");
    let precache = worker.split("const PRECACHE = ").nth(1).and_then(|r| r.split(";\n").next()).unwrap();
    let urls: Vec<String> = serde_json::from_str(precache).unwrap();
    assert!(urls.iter().all(|u| u.starts_with("/Memoire/")), "{urls:?}");
    for url in &urls {
        // Every precached URL is a file of the export.
        let file = url.trim_start_matches("/Memoire/").split('?').next().unwrap().to_string();
        let file = if file.is_empty() || file.ends_with('/') { format!("{file}index.html") } else { file };
        assert!(dir.join(&file).exists(), "{url} → {file}");
    }
    assert!(worker.contains(r#"const OFFLINE = "/Memoire/offline/";"#));

    // What a static host cannot compute.
    let root = read(&dir, "index.html");
    assert!(root.contains(r#"data-redirect="/""#));
    assert!(root.contains(r#"content="0; url=/Memoire/en/""#));
    let legacy = read(&dir, "docs/models/index.html");
    assert!(legacy.contains(r#"data-redirect="/docs/models/""#));
    assert!(legacy.contains(r#"href="/Memoire/es/docs/models/""#));
    assert!(read(&dir, "404.html").contains(r#"<meta name="robots" content="noindex"#));
    assert!(read(&dir, "sitemap.xml").contains("<loc>https://user.github.io/Memoire/es/docs/models/</loc>"));
    assert!(read(&dir, "robots.txt").contains("Sitemap: https://user.github.io/Memoire/sitemap.xml"));
    assert!(read(&dir, "search/es.txt").lines().count() > 20);
    assert!(dir.join("static/app.css").exists());
    assert!(dir.join("static/icon-maskable-512.png").exists());
    assert!(dir.join("offline/index.html").exists());
    assert!(dir.join(".nojekyll").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
