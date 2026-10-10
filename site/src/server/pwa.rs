//! Installable and offline: the web app manifest of each language and the service worker,
//! both built with `active::pwa`.
//!
//! The service worker precaches the home page, the playground and the first documentation
//! page of every language, the offline page, the stylesheet, the fonts, the icons and the
//! WebAssembly bundle: once a page has been opened, the site (and the ORM running in the
//! playground) works without network. Pages are fetched from the network first, so
//! visitors always get the latest version when online.

use active::pwa::{Manifest, ServiceWorker};

use super::Site;
use super::assets::fnv;
use crate::i18n::t;
use crate::routes::{Lang, Page, Route, base, link};

/// Theme and splash color: the dark background of the site.
pub use crate::{THEME_DARK as THEME, THEME_LIGHT};

/// `/{lang}/manifest.webmanifest`. One identity (`id`) for every language; each opens on
/// its own home page.
pub fn manifest(lang: Lang) -> Manifest {
    let base = base();
    Manifest::new("Memoire")
        .short_name("Memoire")
        .description(t(lang).meta_description)
        .id(format!("{base}/"))
        .start_url(format!("{base}/{}/", lang.code()))
        .scope(format!("{base}/"))
        .theme_color(THEME)
        .background_color(THEME)
        .lang(lang.code())
        .categories(["developer", "education", "productivity"])
        .icon(link("/static/icon-192.png"), "192x192", "image/png")
        .icon(link("/static/icon-512.png"), "512x512", "image/png")
        .maskable_icon(link("/static/icon-maskable-512.png"), "512x512", "image/png")
        .icon(link("/static/favicon.svg"), "any", "image/svg+xml")
}

/// The address of a language's manifest.
pub fn manifest_url(lang: Lang) -> String {
    link(&format!("/{}/manifest.webmanifest", lang.code()))
}

/// The offline page, shown for pages never opened while the network is down.
pub fn offline_url() -> String {
    link("/offline/")
}

/// Every URL downloaded when the service worker installs, exactly as the pages request
/// them (versioned files with their `?v=`).
pub fn precache(site: &Site) -> Vec<String> {
    let mut urls = Vec::new();
    for lang in Lang::ALL {
        for page in [Page::Home, Page::Playground, Page::Docs] {
            urls.push(Route::new(lang, page).href());
        }
    }
    let assets = &site.assets;
    for (url, _) in assets.iter() {
        let bundle = url.starts_with("/pkg/") && (url.ends_with(".js") || url.ends_with(".wasm"));
        if url == "/static/app.css" || bundle {
            urls.push(assets.url(url));
        } else if url.starts_with("/static/fonts/") && url.ends_with(".woff2")
            || url.starts_with("/static/icon-")
            || url == "/static/favicon.svg"
        {
            urls.push(link(url));
        }
    }
    urls
}

/// A version that changes with every deployment that changes a file, the documentation
/// or the server.
pub fn version(site: &Site) -> String {
    let mut h = fnv(0xcbf2_9ce4_8422_2325, env!("CARGO_PKG_VERSION").as_bytes());
    for (url, asset) in site.assets.iter() {
        h = fnv(h, url.as_bytes());
        h = fnv(h, asset.version.as_bytes());
    }
    h = fnv(h, &super::docs::Docs::version().to_le_bytes());
    format!("{h:016x}")[..12].to_string()
}

/// `/sw.js`.
pub fn service_worker(site: &Site) -> String {
    ServiceWorker::new("memoire-site", version(site)).precache(precache(site)).offline_page(offline_url()).to_js()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_opens_each_language_on_its_home_page() {
        let json = manifest(Lang::Fr).to_json();
        assert!(json.starts_with(r#"{"name":"Memoire","short_name":"Memoire""#), "{json}");
        assert!(json.contains(r#""start_url":"/fr/""#));
        assert!(json.contains(r#""scope":"/""#));
        assert!(json.contains(r##""theme_color":"#07070d""##));
        assert!(json.contains(r#""purpose":"maskable""#));
    }
}
