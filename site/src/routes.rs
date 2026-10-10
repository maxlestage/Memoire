//! URLs of the site: `/{lang}/`, `/{lang}/features/`, `/{lang}/performance/`,
//! `/{lang}/playground/`, `/{lang}/docs/` and `/{lang}/docs/{slug}/`, all behind an
//! optional base path (`/Memoire` when the site is published on GitHub Pages).

use std::sync::OnceLock;

static BASE: OnceLock<String> = OnceLock::new();

/// Sets the base path once, before anything is rendered: `""` (the default) or `/Memoire`.
/// The server takes it from `BASE_PATH`, the browser from `<html data-base>`.
pub fn set_base(base: &str) {
    let base = base.trim().trim_end_matches('/');
    let base = match base {
        "" => String::new(),
        b if b.starts_with('/') => b.to_string(),
        b => format!("/{b}"),
    };
    let _ = BASE.set(base);
}

/// The base path, without trailing slash (empty by default).
pub fn base() -> &'static str {
    BASE.get().map_or("", String::as_str)
}

/// A link to `path` (which starts with `/`), behind the base path.
pub fn link(path: &str) -> String {
    format!("{}{path}", base())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    En,
    Fr,
    Es,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::En, Lang::Fr, Lang::Es];

    /// Index into `[en, fr, es]` arrays (documentation titles, benchmark labels…).
    pub fn idx(self) -> usize {
        match self {
            Lang::En => 0,
            Lang::Fr => 1,
            Lang::Es => 2,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Fr => "fr",
            Lang::Es => "es",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Lang::En => "EN",
            Lang::Fr => "FR",
            Lang::Es => "ES",
        }
    }

    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Fr => "Français",
            Lang::Es => "Español",
        }
    }

    /// `og:locale`.
    pub fn locale(self) -> &'static str {
        match self {
            Lang::En => "en_US",
            Lang::Fr => "fr_FR",
            Lang::Es => "es_ES",
        }
    }

    /// `"fr"`, `"fr-FR"`, `"FR_ca"`… → the language, if the site has it.
    pub fn parse(code: &str) -> Option<Lang> {
        let code = code.trim().to_ascii_lowercase();
        match code.split(['-', '_']).next().unwrap_or_default() {
            "en" => Some(Lang::En),
            "fr" => Some(Lang::Fr),
            "es" => Some(Lang::Es),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    Home,
    Features,
    Performance,
    Playground,
    /// `/docs/`: the first page of the documentation.
    Docs,
    Doc(String),
    NotFound,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub lang: Lang,
    pub page: Page,
}

impl Route {
    pub fn new(lang: Lang, page: Page) -> Route {
        Route { lang, page }
    }

    /// The route of `path` (without the base path), `None` when it does not start with a
    /// language. `slugs` lists the documentation pages that exist.
    pub fn parse(path: &str, slugs: &[&str]) -> Option<Route> {
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        let first = parts.first()?;
        // Exactly a language code: `/fr/`, not `/fr-FR/` nor `/french/`.
        let lang = Lang::ALL.into_iter().find(|l| l.code() == *first)?;
        Some(Route { lang, page: Page::parse(&parts[1..], slugs) })
    }

    /// The path of the page, without the base path (absolute URLs are built from the
    /// public address, which already contains it).
    pub fn path(&self) -> String {
        format!("/{}{}", self.lang.code(), self.page.path())
    }

    /// A link to the page, behind the base path.
    pub fn href(&self) -> String {
        link(&self.path())
    }

    /// The same page in another language.
    pub fn with_lang(&self, lang: Lang) -> Route {
        Route { lang, page: self.page.clone() }
    }

    /// The canonical path: `/docs/` shows the first documentation page, whose own
    /// address is canonical.
    pub fn canonical_path(&self) -> String {
        match &self.page {
            Page::Docs => Route::new(self.lang, Page::Doc(crate::docs::FIRST_SLUG.into())).path(),
            _ => self.path(),
        }
    }
}

impl Page {
    /// The page of the path segments after the language (`["docs", "models"]`).
    pub fn parse(parts: &[&str], slugs: &[&str]) -> Page {
        match parts {
            [] => Page::Home,
            ["features"] => Page::Features,
            ["performance"] => Page::Performance,
            ["playground"] => Page::Playground,
            ["docs"] => Page::Docs,
            ["docs", slug] if slugs.contains(slug) => Page::Doc(slug.to_string()),
            _ => Page::NotFound,
        }
    }

    /// The path after the language, with slashes on both ends (`/docs/models/`).
    pub fn path(&self) -> String {
        match self {
            Page::Home => "/".into(),
            Page::Features => "/features/".into(),
            Page::Performance => "/performance/".into(),
            Page::Playground => "/playground/".into(),
            Page::Docs => "/docs/".into(),
            Page::Doc(slug) => format!("/docs/{slug}/"),
            Page::NotFound => "/404/".into(),
        }
    }

    /// The addresses of the previous version of the site, which had no language prefix
    /// (`/features`, `/docs/models`): they now lead to the page in the visitor's language.
    pub fn legacy(path: &str, slugs: &[&str]) -> Option<Page> {
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        match Page::parse(&parts, slugs) {
            Page::Home | Page::NotFound => None,
            page => Some(page),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_builds_urls() {
        let slugs = ["models", "faq"];
        let doc = Route::parse("/fr/docs/models/", &slugs).unwrap();
        assert_eq!(doc, Route::new(Lang::Fr, Page::Doc("models".into())));
        assert_eq!(doc.href(), "/fr/docs/models/");
        assert_eq!(doc.with_lang(Lang::Es).href(), "/es/docs/models/");
        assert_eq!(Route::parse("/en/", &slugs).unwrap().page, Page::Home);
        assert_eq!(Route::parse("/en/docs", &slugs).unwrap().page, Page::Docs);
        assert_eq!(Route::parse("/es/playground/", &slugs).unwrap().page, Page::Playground);
        assert_eq!(Route::parse("/en/docs/nope/", &slugs).unwrap().page, Page::NotFound);
        assert_eq!(Route::parse("/de/", &slugs), None);
        assert_eq!(Route::parse("/fr-FR/", &slugs), None);
        assert_eq!(Route::new(Lang::En, Page::Docs).canonical_path(), "/en/docs/introduction/");
    }

    #[test]
    fn legacy_addresses() {
        let slugs = ["models"];
        assert_eq!(Page::legacy("/features", &slugs), Some(Page::Features));
        assert_eq!(Page::legacy("/docs/models/", &slugs), Some(Page::Doc("models".into())));
        assert_eq!(Page::legacy("/", &slugs), None);
        assert_eq!(Page::legacy("/docs/nope", &slugs), None);
    }

    #[test]
    fn languages() {
        assert_eq!(Lang::parse("fr-CA"), Some(Lang::Fr));
        assert_eq!(Lang::parse("ES"), Some(Lang::Es));
        assert_eq!(Lang::parse("de"), None);
    }
}
