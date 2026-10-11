//! Documentation manifest: categories, pages and localised titles, shared by the server
//! (which renders `docs/<lang>/<slug>.md`, see `server::docs`) and the browser (sidebar
//! and search island).
//!
//! To add a page: create `docs/en/<slug>.md`, `docs/fr/<slug>.md`, `docs/es/<slug>.md`
//! and add a `page!` line below (the server embeds every listed page at compile time).

use crate::routes::Lang;

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
        DocPage { slug: $slug, title: [$en, $fr, $es] }
    };
}

pub const FIRST_SLUG: &str = "introduction";

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
            page!("events", "Change notifications", "Notifications de changements", "Notificaciones de cambios"),
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
            page!(
                "comparison",
                "Memoire vs Diesel, SeaORM, Nitrite",
                "Memoire face à Diesel, SeaORM, Nitrite",
                "Memoire frente a Diesel, SeaORM, Nitrite"
            ),
            page!("faq", "FAQ", "FAQ", "Preguntas frecuentes"),
        ],
    },
];

/// All pages in reading order, with their category.
pub fn all_pages() -> impl Iterator<Item = (&'static DocCategory, &'static DocPage)> {
    CATEGORIES.iter().flat_map(|c| c.pages.iter().map(move |p| (c, p)))
}

/// Every page slug, in reading order.
pub fn slugs() -> &'static [&'static str] {
    static SLUGS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    SLUGS.get_or_init(|| all_pages().map(|(_, p)| p.slug).collect())
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
        Some(i) => (i.checked_sub(1).map(|j| pages[j]), pages.get(i + 1).copied()),
        None => (None, None),
    }
}

/// Reading time in minutes (≈ 200 words / minute, at least 1).
pub fn reading_minutes(words: usize) -> usize {
    words.div_ceil(200).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_shape() {
        assert_eq!(CATEGORIES.len(), 6);
        assert_eq!(page_count(), 24);
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
}
