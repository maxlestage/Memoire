//! The documentation: `docs/{en,fr,es}/<slug>.md`, embedded in the server at compile time
//! and rendered once at startup (HTML, table of contents, summary, search index).

use std::collections::HashMap;

use super::markdown::{self, Labels, TocEntry};
use crate::docs::{self as manifest, all_pages};
use crate::i18n::t;
use crate::routes::Lang;
use crate::search::Entry;

/// `[en, fr, es]` sources of every page, in reading order (the same list as
/// `crate::docs::CATEGORIES`: a test checks it).
macro_rules! sources {
    ($($slug:literal),* $(,)?) => {
        &[$(($slug, [
            include_str!(concat!("../../../docs/en/", $slug, ".md")),
            include_str!(concat!("../../../docs/fr/", $slug, ".md")),
            include_str!(concat!("../../../docs/es/", $slug, ".md")),
        ])),*]
    };
}

pub const SOURCES: &[(&str, [&str; 3])] = sources!(
    "introduction",
    "installation",
    "quick-start",
    "models",
    "queries",
    "crud",
    "lifecycle",
    "relations",
    "transactions",
    "events",
    "query-log",
    "migrations",
    "raw-sql",
    "memory",
    "sqlite",
    "postgres",
    "mysql",
    "redis",
    "mongodb",
    "browser-storage",
    "performance",
    "benchmarks",
    "deploy-heroku-mobile",
    "publish-crates-io",
    "cheatsheet",
    "comparison",
    "faq",
);

/// A rendered page.
pub struct Doc {
    pub html: String,
    pub toc: Vec<TocEntry>,
    pub words: usize,
    /// The first paragraph: `<meta name="description">`.
    pub summary: String,
    /// Plain text, for the search index.
    pub plain: String,
}

pub struct Docs {
    pages: HashMap<(Lang, &'static str), Doc>,
}

impl Docs {
    /// Renders every page in every language (links behind the base path set before).
    pub fn load() -> Docs {
        let mut pages = HashMap::new();
        for (slug, sources) in SOURCES {
            for lang in Lang::ALL {
                let tx = t(lang);
                let labels =
                    Labels { copy: tx.copy, note: tx.callout_note, tip: tx.callout_tip, warning: tx.callout_warning };
                let source = sources[lang.idx()];
                let rendered = markdown::render(source, &labels, lang);
                pages.insert(
                    (lang, *slug),
                    Doc {
                        html: rendered.html,
                        toc: rendered.toc,
                        words: rendered.words,
                        summary: markdown::summary(source),
                        plain: markdown::plain_text(source),
                    },
                );
            }
        }
        Docs { pages }
    }

    pub fn get(&self, lang: Lang, slug: &str) -> Option<&Doc> {
        let (_, page) = manifest::find(slug)?;
        self.pages.get(&(lang, page.slug))
    }

    /// `/search/{lang}.txt`: every page of the language, one line each (see
    /// [`crate::search::Entry`]).
    pub fn search_index(&self, lang: Lang) -> String {
        let li = lang.idx();
        all_pages()
            .filter_map(|(category, page)| {
                let doc = self.get(lang, page.slug)?;
                Some(Entry::new(page.slug, page.title[li], category.title[li], &doc.plain).line())
            })
            .collect()
    }

    /// A short hash of every source: part of the service worker's version.
    pub fn version() -> u64 {
        SOURCES
            .iter()
            .flat_map(|(_, sources)| sources.iter())
            .fold(0xcbf2_9ce4_8422_2325_u64, |h, source| super::assets::fnv(h, source.as_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_pages_are_the_manifest() {
        let embedded: Vec<&str> = SOURCES.iter().map(|(slug, _)| *slug).collect();
        assert_eq!(embedded, manifest::slugs());
    }

    #[test]
    fn every_page_renders_with_working_links() {
        let docs = Docs::load();
        for lang in Lang::ALL {
            for slug in manifest::slugs() {
                let doc = docs.get(lang, slug).unwrap();
                assert!(!doc.summary.is_empty(), "{slug} has no summary");
                assert!(doc.html.contains("<h1"), "{slug} has no title");
                // Links to other pages point to pages that exist, in the same language.
                for link in doc.html.split("href=\"/").skip(1) {
                    let target = link.split('"').next().unwrap_or_default();
                    if let Some(rest) = target.strip_prefix(&format!("{}/docs/", lang.code())) {
                        let page = rest.split(['/', '#']).next().unwrap_or_default();
                        assert!(manifest::find(page).is_some(), "{lang:?}/{slug}: broken link to {target}");
                    }
                }
            }
        }
        let index = docs.search_index(Lang::Fr);
        assert_eq!(index.lines().count(), manifest::page_count());
    }
}
