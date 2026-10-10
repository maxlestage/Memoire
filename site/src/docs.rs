//! Documentation manifest: categories, pages and localised titles.
//!
//! The markdown sources are not compiled into the .wasm (they would weigh as
//! much as the whole app): Trunk copies `docs/<lang>/<slug>.md` to
//! `content/<lang>/<slug>.md` next to it, and each page is fetched the first
//! time it is shown, then kept in memory.
//!
//! To add a page: create `docs/en/<slug>.md`, `docs/fr/<slug>.md`, `docs/es/<slug>.md`
//! and add a `page!` line below.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::dom;
use crate::i18n::Lang;
use crate::markdown;

/// One string per language, in `[en, fr, es]` order (see [`Lang::idx`]).
pub type Tr = [&'static str; 3];

pub struct DocPage {
    pub slug: &'static str,
    pub title: Tr,
}

pub struct DocCategory {
    pub title: Tr,
    pub pages: &'static [DocPage],
}

macro_rules! page {
    ($slug:literal, $en:literal, $fr:literal, $es:literal) => {
        DocPage {
            slug: $slug,
            title: [$en, $fr, $es],
        }
    };
}

pub const FIRST_SLUG: &str = "introduction";
pub const REPO_URL: &str = "https://github.com/maxlestage/Memoire";

pub static CATEGORIES: &[DocCategory] = &[
    DocCategory {
        title: ["Getting started", "Bien démarrer", "Primeros pasos"],
        pages: &[
            page!("introduction", "Introduction", "Introduction", "Introducción"),
            page!("installation", "Installation", "Installation", "Instalación"),
            page!("quick-start", "Quick start", "Démarrage rapide", "Inicio rápido"),
        ],
    },
    DocCategory {
        title: ["Core concepts", "Concepts clés", "Conceptos clave"],
        pages: &[
            page!("models", "Models", "Modèles", "Modelos"),
            page!("queries", "Query builder", "Constructeur de requêtes", "Constructor de consultas"),
            page!(
                "crud",
                "Create, read, update, delete",
                "Créer, lire, modifier, supprimer",
                "Crear, leer, actualizar, eliminar"
            ),
            page!("relations", "Joins & relations", "Jointures et relations", "Uniones y relaciones"),
            page!("transactions", "Transactions", "Transactions", "Transacciones"),
            page!("migrations", "Migrations", "Migrations", "Migraciones"),
            page!("raw-sql", "Raw SQL", "SQL brut", "SQL sin procesar"),
        ],
    },
    DocCategory {
        title: ["Backends", "Bases de données", "Bases de datos"],
        pages: &[
            page!("memory", "In-memory engine", "Moteur en mémoire", "Motor en memoria"),
            page!("sqlite", "SQLite", "SQLite", "SQLite"),
            page!("postgres", "PostgreSQL", "PostgreSQL", "PostgreSQL"),
            page!("mysql", "MySQL & MariaDB", "MySQL & MariaDB", "MySQL & MariaDB"),
            page!("redis", "Redis", "Redis", "Redis"),
            page!("mongodb", "MongoDB (NoSQL)", "MongoDB (NoSQL)", "MongoDB (NoSQL)"),
        ],
    },
    DocCategory {
        title: ["Performance", "Performance", "Rendimiento"],
        pages: &[
            page!("performance", "Why it's fast", "Pourquoi c'est rapide", "Por qué es rápido"),
            page!("benchmarks", "Benchmarks", "Benchmarks", "Benchmarks"),
        ],
    },
    DocCategory {
        title: ["Deployment", "Déploiement", "Despliegue"],
        pages: &[
            page!(
                "deploy-heroku-mobile",
                "Deploy to Heroku from your phone",
                "Déployer sur Heroku depuis son téléphone",
                "Desplegar en Heroku desde el móvil"
            ),
            page!("publish-crates-io", "Publish to crates.io", "Publier sur crates.io", "Publicar en crates.io"),
        ],
    },
    DocCategory {
        title: ["Reference", "Référence", "Referencia"],
        pages: &[
            page!("cheatsheet", "Cheat sheet", "Aide-mémoire", "Chuleta"),
            page!("faq", "FAQ", "FAQ", "Preguntas frecuentes"),
        ],
    },
];

/// All pages in reading order, with their category.
pub fn all_pages() -> impl Iterator<Item = (&'static DocCategory, &'static DocPage)> {
    CATEGORIES
        .iter()
        .flat_map(|c| c.pages.iter().map(move |p| (c, p)))
}

pub fn page_count() -> usize {
    CATEGORIES.iter().map(|c| c.pages.len()).sum()
}

pub fn find(slug: &str) -> Option<(&'static DocCategory, &'static DocPage)> {
    all_pages().find(|(_, p)| p.slug == slug)
}

pub fn title(slug: &str, lang: Lang) -> &'static str {
    find(slug).map(|(_, p)| p.title[lang.idx()]).unwrap_or_default()
}

/// Previous / next pages in reading order.
pub fn neighbours(slug: &str) -> (Option<&'static DocPage>, Option<&'static DocPage>) {
    let pages: Vec<&'static DocPage> = all_pages().map(|(_, p)| p).collect();
    match pages.iter().position(|p| p.slug == slug) {
        Some(i) => (
            i.checked_sub(1).map(|j| pages[j]),
            pages.get(i + 1).copied(),
        ),
        None => (None, None),
    }
}

pub fn edit_url(lang: Lang, slug: &str) -> String {
    format!("{REPO_URL}/edit/master/docs/{}/{slug}.md", lang.code())
}

/// Reading time in minutes (≈ 200 words / minute, at least 1).
pub fn reading_minutes(words: usize) -> usize {
    words.div_ceil(200).max(1)
}

// ---------------------------------------------------------------------------
// Markdown sources, fetched on demand
// ---------------------------------------------------------------------------

type Key = (&'static str, usize);
type Waiter = Box<dyn FnOnce(Result<(), String>)>;

thread_local! {
    static SOURCES: RefCell<HashMap<Key, Rc<str>>> = RefCell::new(HashMap::new());
    /// Downloads in flight, with the callbacks waiting for each of them.
    static LOADING: RefCell<HashMap<Key, Vec<Waiter>>> = RefCell::new(HashMap::new());
}

/// The markdown of a page, if it has been fetched already.
pub fn source(slug: &str, lang: Lang) -> Option<Rc<str>> {
    let (_, page) = find(slug)?;
    SOURCES.with(|s| s.borrow().get(&(page.slug, lang.idx())).cloned())
}

/// Makes sure a page's markdown is available, fetching it at most once, then
/// calls `done` (right away when it is cached, with an error message if the
/// download failed).
pub fn load(slug: &str, lang: Lang, done: impl FnOnce(Result<(), String>) + 'static) {
    let Some((_, page)) = find(slug) else {
        return done(Err(format!("unknown page `{slug}`")));
    };
    let key = (page.slug, lang.idx());
    if SOURCES.with(|s| s.borrow().contains_key(&key)) {
        return done(Ok(()));
    }
    let first = LOADING.with(|l| {
        let mut l = l.borrow_mut();
        let waiters = l.entry(key).or_default();
        waiters.push(Box::new(done));
        waiters.len() == 1
    });
    if !first {
        return;
    }
    let url = format!("{}/content/{}/{}.md", dom::base_path(), lang.code(), page.slug);
    yew::platform::spawn_local(async move {
        let result = dom::fetch_text(&url).await.map(|text| {
            SOURCES.with(|s| s.borrow_mut().insert(key, Rc::from(text)));
        });
        for waiter in LOADING.with(|l| l.borrow_mut().remove(&key)).unwrap_or_default() {
            waiter(result.clone());
        }
    });
}

/// How many pages of a language have been fetched (search needs them all).
pub fn loaded_count(lang: Lang) -> usize {
    SOURCES.with(|s| s.borrow().keys().filter(|(_, l)| *l == lang.idx()).count())
}

#[cfg(test)]
fn insert_source(slug: &'static str, lang: Lang, text: &str) {
    SOURCES.with(|s| s.borrow_mut().insert((slug, lang.idx()), Rc::from(text)));
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
pub struct SearchHit {
    pub slug: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub title_match: bool,
    /// (before, matched, after) excerpt around the first content match.
    pub snippet: Option<(String, String, String)>,
}

/// (plain text, lowercased plain text), keyed by (slug, language index).
type PlainCache = HashMap<(&'static str, usize), Rc<(String, String)>>;

thread_local! {
    static PLAIN: RefCell<PlainCache> = RefCell::new(HashMap::new());
}

/// (plain text, lowercased plain text) for a page, cached; `None` until the
/// page has been fetched.
fn plain(page: &'static DocPage, lang: Lang) -> Option<Rc<(String, String)>> {
    let key = (page.slug, lang.idx());
    if let Some(hit) = PLAIN.with(|cache| cache.borrow().get(&key).cloned()) {
        return Some(hit);
    }
    let text = markdown::plain_text(&source(page.slug, lang)?);
    let lower = text.to_lowercase();
    let entry = Rc::new((text, lower));
    PLAIN.with(|cache| cache.borrow_mut().insert(key, entry.clone()));
    Some(entry)
}

/// Case-insensitive search over titles and contents in the given language.
/// Title matches come first. Contents are searched once they are fetched.
pub fn search(query: &str, lang: Lang) -> Vec<SearchHit> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let li = lang.idx();
    let mut hits: Vec<SearchHit> = Vec::new();
    for (cat, page) in all_pages() {
        let title = page.title[li];
        let title_match = title.to_lowercase().contains(&q);
        let snippet = plain(page, lang).and_then(|doc| {
            let (text, lower) = (&doc.0, &doc.1);
            lower.find(&q).and_then(|pos| excerpt(text, lower, pos, q.len()))
        });
        if title_match || snippet.is_some() {
            hits.push(SearchHit {
                slug: page.slug,
                title,
                category: cat.title[li],
                title_match,
                snippet,
            });
        }
    }
    hits.sort_by_key(|h| !h.title_match);
    hits
}

/// Cut a short excerpt around a match. Positions come from the lowercased text,
/// which can differ in byte length from the original for a few characters, so
/// fall back gracefully when boundaries don't line up.
fn excerpt(text: &str, lower: &str, pos: usize, len: usize) -> Option<(String, String, String)> {
    let source = if text.len() == lower.len() { text } else { lower };
    let end = pos + len;
    if !source.is_char_boundary(pos) || !source.is_char_boundary(end.min(source.len())) {
        return None;
    }
    let mut start = pos.saturating_sub(48);
    while !source.is_char_boundary(start) {
        start -= 1;
    }
    let mut stop = (end + 72).min(source.len());
    while !source.is_char_boundary(stop) {
        stop += 1;
    }
    let before = format!("{}{}", if start > 0 { "…" } else { "" }, &source[start..pos]);
    let after = format!("{}{}", &source[end..stop], if stop < source.len() { "…" } else { "" });
    Some((before, source[pos..end].to_string(), after))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_shape() {
        assert_eq!(CATEGORIES.len(), 6);
        assert_eq!(page_count(), 22);
        assert!(find("quick-start").is_some());
        let (prev, next) = neighbours("introduction");
        assert!(prev.is_none());
        assert_eq!(next.map(|p| p.slug), Some("installation"));
    }

    #[test]
    fn every_page_exists_in_every_language() {
        let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs");
        for (_, page) in all_pages() {
            for lang in ["en", "fr", "es"] {
                let path = docs.join(lang).join(format!("{}.md", page.slug));
                assert!(path.is_file(), "missing {}", path.display());
            }
        }
    }

    #[test]
    fn search_finds_titles() {
        let hits = search("heroku", Lang::En);
        assert!(hits.iter().any(|h| h.slug == "deploy-heroku-mobile"));
    }

    #[test]
    fn search_finds_fetched_contents() {
        assert!(search("zebra", Lang::Fr).is_empty());
        insert_source("faq", Lang::Fr, "# FAQ\n\nUn zèbre et un zebra.");
        assert_eq!(loaded_count(Lang::Fr), 1);
        let hits = search("zebra", Lang::Fr);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].slug, "faq");
        assert!(!hits[0].title_match);
        assert_eq!(hits[0].snippet.as_ref().map(|s| s.1.as_str()), Some("zebra"));
        assert!(source("faq", Lang::Fr).is_some() && source("faq", Lang::En).is_none());
    }
}
