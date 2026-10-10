//! Internationalisation: every piece of UI text, in English, French and Spanish.
//!
//! The current language travels through a Yew context ([`LangContext`]); components
//! read it with [`use_lang`] and get the matching string table with [`Lang::t`].

use yew::prelude::*;

use crate::dom;

const STORAGE_KEY: &str = "memoire-lang";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Fr,
    Es,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::En, Lang::Fr, Lang::Es];

    /// Index into `[en, fr, es]` arrays (docs titles, contents, bench labels…).
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

    pub fn from_code(code: &str) -> Option<Lang> {
        let code = code.trim().to_ascii_lowercase();
        let prefix = code.split(['-', '_']).next().unwrap_or_default();
        match prefix {
            "en" => Some(Lang::En),
            "fr" => Some(Lang::Fr),
            "es" => Some(Lang::Es),
            _ => None,
        }
    }

    /// Saved choice first, then the browser language, then English.
    pub fn detect() -> Lang {
        if let Some(lang) = dom::storage_get(STORAGE_KEY).and_then(|c| Lang::from_code(&c)) {
            return lang;
        }
        dom::window()
            .navigator()
            .language()
            .and_then(|l| Lang::from_code(&l))
            .unwrap_or(Lang::En)
    }

    /// Persist the choice and update `<html lang>`.
    pub fn apply(self) {
        dom::storage_set(STORAGE_KEY, self.code());
        if let Some(root) = dom::document().document_element() {
            let _ = root.set_attribute("lang", self.code());
        }
    }

    pub fn t(self) -> &'static Texts {
        match self {
            Lang::En => &EN,
            Lang::Fr => &FR,
            Lang::Es => &ES,
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct LangContext {
    pub lang: Lang,
    pub set: Callback<Lang>,
}

/// Current language + setter. Panics if used outside the provider (programming error).
#[hook]
pub fn use_lang() -> LangContext {
    use_context::<LangContext>().expect("LangContext provider missing")
}

/// A feature card: short title + one-liner + longer explanation for /features.
pub struct Feature {
    pub title: &'static str,
    pub desc: &'static str,
    pub detail: &'static str,
}

pub struct Step {
    pub title: &'static str,
    pub desc: &'static str,
}

pub struct Texts {
    // Navigation
    pub nav_home: &'static str,
    pub nav_features: &'static str,
    pub nav_performance: &'static str,
    pub nav_playground: &'static str,
    pub nav_docs: &'static str,
    pub nav_get_started: &'static str,
    pub nav_open_menu: &'static str,
    pub nav_close_menu: &'static str,
    pub skip_to_content: &'static str,
    pub theme_toggle: &'static str,
    pub language: &'static str,

    // Hero
    pub hero_badge: &'static str,
    pub hero_title_1: &'static str,
    pub hero_title_2: &'static str,
    pub hero_subtitle: &'static str,
    pub hero_taglines: &'static [&'static str],
    pub hero_cta_primary: &'static str,
    pub hero_cta_github: &'static str,
    pub copy: &'static str,
    pub copied: &'static str,
    pub copy_install: &'static str,
    pub scroll: &'static str,

    // Stats
    pub stat_rust: &'static str,
    pub stat_backends: &'static str,
    pub stat_dialects: &'static str,
    pub stat_guides: &'static str,

    // Marquee
    pub marquee_items: &'static [&'static str],

    // Features
    pub features_eyebrow: &'static str,
    pub features_title: &'static str,
    pub features_subtitle: &'static str,
    pub features: &'static [Feature; 8],
    pub features_more: &'static str,

    // Code showcase
    pub code_eyebrow: &'static str,
    pub code_title: &'static str,
    pub code_subtitle: &'static str,
    pub code_tabs: [&'static str; 4],
    pub terminal: &'static str,

    // How it works
    pub how_eyebrow: &'static str,
    pub how_title: &'static str,
    pub how_subtitle: &'static str,
    pub how_steps: &'static [Step; 4],

    // Performance teaser & page
    pub perf_eyebrow: &'static str,
    pub perf_title: &'static str,
    pub perf_subtitle: &'static str,
    pub perf_cta: &'static str,
    pub bench_lower_better: &'static str,
    pub bench_placeholder: &'static str,
    pub bench_memoire: &'static str,
    pub bench_baseline: &'static str,
    pub perf_page_title: &'static str,
    pub perf_page_subtitle: &'static str,
    pub perf_why_title: &'static str,
    pub perf_why: &'static [Step; 4],
    pub perf_results_title: &'static str,
    pub perf_method_title: &'static str,
    pub perf_method_text: &'static str,
    pub perf_read_more: &'static str,

    // Features page
    pub fp_title: &'static str,
    pub fp_subtitle: &'static str,
    pub backends_title: &'static str,
    pub backends_subtitle: &'static str,
    pub backends: &'static [Step; 6],

    // CTA
    pub cta_title: &'static str,
    pub cta_subtitle: &'static str,
    pub cta_primary: &'static str,
    pub cta_secondary: &'static str,

    // Footer
    pub footer_tagline: &'static str,
    pub footer_mission: &'static str,
    pub footer_product: &'static str,
    pub footer_docs: &'static str,
    pub footer_community: &'static str,
    pub footer_issues: &'static str,
    pub footer_deploy: &'static str,
    pub footer_rights: &'static str,
    pub footer_built: &'static str,
    pub back_to_top: &'static str,

    // Playground
    pub pg_title: &'static str,
    pub pg_subtitle: &'static str,
    pub pg_controls: &'static str,
    pub pg_min_age: &'static str,
    pub pg_name_contains: &'static str,
    pub pg_name_placeholder: &'static str,
    pub pg_order_by: &'static str,
    pub pg_order_name: &'static str,
    pub pg_order_age: &'static str,
    pub pg_direction: &'static str,
    pub pg_asc: &'static str,
    pub pg_desc: &'static str,
    pub pg_limit: &'static str,
    pub pg_reset: &'static str,
    pub pg_rust: &'static str,
    pub pg_sql: &'static str,
    pub pg_params: &'static str,
    pub pg_result: &'static str,
    pub pg_rows: &'static str,
    pub pg_no_rows: &'static str,
    pub pg_exec: &'static str,
    pub pg_mock_note: &'static str,

    // Docs
    pub docs_title: &'static str,
    pub docs_search: &'static str,
    pub docs_no_results: &'static str,
    pub docs_results: &'static str,
    pub docs_on_this_page: &'static str,
    pub docs_prev: &'static str,
    pub docs_next: &'static str,
    pub docs_edit: &'static str,
    pub docs_min_read: &'static str,
    pub docs_menu: &'static str,
    pub docs_not_found_title: &'static str,
    pub docs_not_found_text: &'static str,
    pub callout_note: &'static str,
    pub callout_tip: &'static str,
    pub callout_warning: &'static str,

    // 404
    pub nf_title: &'static str,
    pub nf_text: &'static str,
    pub nf_home: &'static str,
    pub nf_docs: &'static str,

    // Meta
    pub meta_description: &'static str,
}

// ---------------------------------------------------------------------------
// English
// ---------------------------------------------------------------------------

static EN_FEATURES: [Feature; 8] = [
    Feature {
        title: "Pure Rust",
        desc: "No C toolchain, no FFI surprises. Just cargo add and build.",
        detail: "Memoire is written entirely in Rust. It compiles anywhere Rust compiles, from servers to the browser, and fits naturally into your existing cargo workflow.",
    },
    Feature {
        title: "Compile-time SQL",
        desc: "Derive macros generate your SQL when you compile, not when you run.",
        detail: "The Model derive turns your struct into table metadata and column constants at compile time. Typos in column names become compiler errors instead of production incidents.",
    },
    Feature {
        title: "Type-safe query builder",
        desc: "Compose filters with real Rust expressions, checked by the compiler.",
        detail: "Columns are typed constants: User::AGE.ge(18) only accepts numbers, User::NAME.contains(\"li\") only accepts text. Combine conditions with & and | and let the compiler catch mistakes.",
    },
    Feature {
        title: "Zero-cost row mapping",
        desc: "Rows become your structs directly, without a reflection layer.",
        detail: "Mapping code is generated for each model, so turning a row into a User is plain, monomorphised Rust: no runtime reflection, no dynamic dispatch on the hot path.",
    },
    Feature {
        title: "Multiple backends",
        desc: "SQL + NoSQL: PostgreSQL, MySQL/MariaDB, SQLite, Redis, MongoDB and an in-memory engine (WASM).",
        detail: "Six backends behind one API, SQL and NoSQL alike. Write your models once, use MemoryDb in tests, then PostgreSQL, MySQL/MariaDB, SQLite, Redis or MongoDB in production.",
    },
    Feature {
        title: "In-memory engine, even in WASM",
        desc: "A full engine for tests and demos that also runs in the browser.",
        detail: "MemoryDb needs no server and no file. It makes unit tests instant and is portable enough to run inside WebAssembly, like the playground on this site.",
    },
    Feature {
        title: "Async-first",
        desc: "Every operation is async and fits right into your runtime.",
        detail: "insert, update, query, transactions and migrations are all async, so Memoire slots into modern async Rust applications without blocking threads.",
    },
    Feature {
        title: "Migrations",
        desc: "Versioned, ordered and generated from your models.",
        detail: "Describe your schema history with Migration::new(version, name) and let the Migrator apply what is missing, in order, at startup.",
    },
];

static EN_STEPS: [Step; 4] = [
    Step { title: "Define", desc: "Write a plain Rust struct that describes your table." },
    Step { title: "Derive", desc: "Add #[derive(Model)]: columns, keys and SQL are generated at compile time." },
    Step { title: "Query", desc: "Compose type-safe queries with filters, ordering and limits." },
    Step { title: "Ship", desc: "Run on PostgreSQL, MySQL/MariaDB, SQLite, Redis, MongoDB or in memory, from servers to WebAssembly." },
];

static EN_WHY: [Step; 4] = [
    Step { title: "SQL built at compile time", desc: "Table and column metadata come from the derive macro, so there is nothing to discover at runtime." },
    Step { title: "Monomorphised row mapping", desc: "Each model gets its own mapping code: no reflection, no boxing on the hot path." },
    Step { title: "Lean query builder", desc: "Queries are small values assembled in place and rendered once per dialect." },
    Step { title: "Async without overhead", desc: "Operations are async end to end and never block your executor threads." },
];

static EN_BACKENDS: [Step; 6] = [
    Step { title: "In-memory", desc: "Instant and zero-setup: perfect for tests, demos and WebAssembly." },
    Step { title: "SQLite", desc: "A single file, ideal for apps, command-line tools and the edge." },
    Step { title: "PostgreSQL", desc: "Pooled connections for production workloads." },
    Step { title: "MySQL & MariaDB", desc: "A real driver for MySQL and MariaDB, with backtick identifiers and ? placeholders." },
    Step { title: "Redis", desc: "Keep your models in Redis, the fast NoSQL key-value store." },
    Step { title: "MongoDB", desc: "Map your models to MongoDB collections and documents (NoSQL)." },
];

static EN: Texts = Texts {
    nav_home: "Home",
    nav_features: "Features",
    nav_performance: "Performance",
    nav_playground: "Playground",
    nav_docs: "Docs",
    nav_get_started: "Get started",
    nav_open_menu: "Open menu",
    nav_close_menu: "Close menu",
    skip_to_content: "Skip to content",
    theme_toggle: "Toggle light / dark theme",
    language: "Language",

    hero_badge: "Pure Rust · Async · WebAssembly-ready",
    hero_title_1: "The Rust ORM",
    hero_title_2: "built for speed.",
    hero_subtitle: "Memoire ORM turns plain Rust structs into type-safe, compile-time checked queries for PostgreSQL, MySQL/MariaDB, SQLite, Redis, MongoDB and an in-memory engine that even runs in your browser.",
    hero_taglines: &[
        "Type-safe queries.",
        "SQL generated at compile time.",
        "Zero-cost row mapping.",
        "SQL and NoSQL, one API.",
        "Runs in WebAssembly.",
    ],
    hero_cta_primary: "Get started",
    hero_cta_github: "GitHub",
    copy: "Copy",
    copied: "Copied!",
    copy_install: "Copy the install command",
    scroll: "Scroll",

    stat_rust: "Pure Rust",
    stat_backends: "Backends",
    stat_dialects: "SQL dialects",
    stat_guides: "Documentation guides",

    marquee_items: &[
        "PostgreSQL", "MySQL", "MariaDB", "SQLite", "Redis", "MongoDB", "In-memory", "WebAssembly",
        "async / await", "Migrations", "Transactions", "Derive macros", "Type-safe filters",
    ],

    features_eyebrow: "Features",
    features_title: "Everything you need. Nothing you don't.",
    features_subtitle: "A focused toolkit that lets you model, query and migrate your data with the confidence of the Rust compiler.",
    features: &EN_FEATURES,
    features_more: "Explore all features",

    code_eyebrow: "Developer experience",
    code_title: "Write Rust. Get SQL.",
    code_subtitle: "Models, queries, transactions and migrations read like the Rust you already write.",
    code_tabs: ["Model", "Query", "Transaction", "Migration"],
    terminal: "Terminal",

    how_eyebrow: "How it works",
    how_title: "From struct to production in four steps",
    how_subtitle: "No schema files to keep in sync, no string-built SQL. Your types are the source of truth.",
    how_steps: &EN_STEPS,

    perf_eyebrow: "Performance",
    perf_title: "Fast by design, measured with Criterion",
    perf_subtitle: "The work happens at compile time so your hot path stays lean.",
    perf_cta: "See the benchmarks",
    bench_lower_better: "Lower is better",
    bench_placeholder: "Placeholder figures: real Criterion results will be published here soon.",
    bench_memoire: "Memoire",
    bench_baseline: "Baseline",
    perf_page_title: "Performance",
    perf_page_subtitle: "Why Memoire is fast, and how we measure it.",
    perf_why_title: "Why it's fast",
    perf_why: &EN_WHY,
    perf_results_title: "Benchmark results",
    perf_method_title: "Methodology",
    perf_method_text: "Benchmarks are written with Criterion and live in the repository. Each figure is the median of many iterations on the same machine, and the baseline is measured in the same run so the comparison stays fair.",
    perf_read_more: "Read the performance guide",

    fp_title: "Features",
    fp_subtitle: "Everything Memoire ORM gives you, explained simply.",
    backends_title: "One API, six backends",
    backends_subtitle: "SQL or NoSQL: pick the storage that fits, your models stay the same.",
    backends: &EN_BACKENDS,

    cta_title: "Ready to make your data fly?",
    cta_subtitle: "Add Memoire ORM to your project in one command and follow the quick start.",
    cta_primary: "Read the docs",
    cta_secondary: "Try the playground",

    footer_tagline: "The blazing-fast, pure-Rust ORM.",
    footer_mission: "Memoire ORM helps Rust developers work with databases through types they trust, from a laptop prototype to production.",
    footer_product: "Product",
    footer_docs: "Docs",
    footer_community: "Community",
    footer_issues: "Issues",
    footer_deploy: "Deploy on Heroku",
    footer_rights: "© 2026 Memoire ORM — MIT / Apache-2.0",
    footer_built: "Built with Rust 🦀 + Yew + WebAssembly",
    back_to_top: "Back to top",

    pg_title: "Playground",
    pg_subtitle: "Build a query with the controls and watch the SQL and the results update live, right in your browser.",
    pg_controls: "Query builder",
    pg_min_age: "Minimum age",
    pg_name_contains: "Name contains",
    pg_name_placeholder: "e.g. li",
    pg_order_by: "Order by",
    pg_order_name: "Name",
    pg_order_age: "Age",
    pg_direction: "Direction",
    pg_asc: "Ascending",
    pg_desc: "Descending",
    pg_limit: "Limit",
    pg_reset: "Reset",
    pg_rust: "Rust code",
    pg_sql: "Generated SQL",
    pg_params: "Bound parameters",
    pg_result: "Result",
    pg_rows: "rows",
    pg_no_rows: "No rows match these filters.",
    pg_exec: "Executed in",
    pg_mock_note: "Preview: the playground currently runs a lightweight mock. The real Memoire in-memory engine compiled to WebAssembly is coming soon.",

    docs_title: "Documentation",
    docs_search: "Search the docs…",
    docs_no_results: "No results for",
    docs_results: "results",
    docs_on_this_page: "On this page",
    docs_prev: "Previous",
    docs_next: "Next",
    docs_edit: "Edit this page on GitHub",
    docs_min_read: "min read",
    docs_menu: "Docs menu",
    docs_not_found_title: "Page not found",
    docs_not_found_text: "This documentation page does not exist (yet). Pick a topic from the menu.",
    callout_note: "Note",
    callout_tip: "Tip",
    callout_warning: "Warning",

    nf_title: "Lost in memory",
    nf_text: "The page you are looking for does not exist or has moved.",
    nf_home: "Back home",
    nf_docs: "Browse the docs",

    meta_description: "Memoire ORM: a blazing-fast, pure-Rust ORM for SQL and NoSQL (PostgreSQL, MySQL/MariaDB, SQLite, Redis, MongoDB) with compile-time SQL, a type-safe query builder and an in-memory engine that runs in WebAssembly.",
};

// ---------------------------------------------------------------------------
// Français
// ---------------------------------------------------------------------------

static FR_FEATURES: [Feature; 8] = [
    Feature {
        title: "100 % Rust",
        desc: "Pas de chaîne de compilation C, pas de surprise FFI. Un cargo add et c'est parti.",
        detail: "Memoire est entièrement écrit en Rust. Il compile partout où Rust compile, du serveur au navigateur, et s'intègre naturellement à votre flux cargo.",
    },
    Feature {
        title: "SQL à la compilation",
        desc: "Les macros derive génèrent votre SQL à la compilation, pas à l'exécution.",
        detail: "Le derive Model transforme votre struct en métadonnées de table et en constantes de colonnes dès la compilation. Une faute de frappe dans un nom de colonne devient une erreur du compilateur, pas un incident en production.",
    },
    Feature {
        title: "Requêtes typées",
        desc: "Composez vos filtres avec de vraies expressions Rust, vérifiées par le compilateur.",
        detail: "Les colonnes sont des constantes typées : User::AGE.ge(18) n'accepte que des nombres, User::NAME.contains(\"li\") que du texte. Combinez les conditions avec & et | et laissez le compilateur repérer les erreurs.",
    },
    Feature {
        title: "Mapping sans surcoût",
        desc: "Les lignes deviennent directement vos structs, sans couche de réflexion.",
        detail: "Le code de mapping est généré pour chaque modèle : transformer une ligne en User, c'est du Rust monomorphisé, sans réflexion à l'exécution ni dispatch dynamique sur le chemin critique.",
    },
    Feature {
        title: "Plusieurs bases",
        desc: "SQL + NoSQL : PostgreSQL, MySQL/MariaDB, SQLite, Redis, MongoDB et un moteur en mémoire (WASM).",
        detail: "Six bases derrière une seule API, SQL comme NoSQL. Écrivez vos modèles une fois, utilisez MemoryDb dans les tests, puis PostgreSQL, MySQL/MariaDB, SQLite, Redis ou MongoDB en production.",
    },
    Feature {
        title: "Moteur en mémoire, même en WASM",
        desc: "Un vrai moteur pour les tests et les démos, qui tourne aussi dans le navigateur.",
        detail: "MemoryDb n'a besoin ni de serveur ni de fichier. Vos tests unitaires deviennent instantanés, et il est assez portable pour tourner en WebAssembly, comme le playground de ce site.",
    },
    Feature {
        title: "Async avant tout",
        desc: "Chaque opération est asynchrone et s'intègre à votre runtime.",
        detail: "insert, update, les requêtes, les transactions et les migrations sont toutes async : Memoire s'intègre aux applications Rust modernes sans bloquer de threads.",
    },
    Feature {
        title: "Migrations",
        desc: "Versionnées, ordonnées et générées depuis vos modèles.",
        detail: "Décrivez l'historique de votre schéma avec Migration::new(version, nom) et laissez le Migrator appliquer ce qui manque, dans l'ordre, au démarrage.",
    },
];

static FR_STEPS: [Step; 4] = [
    Step { title: "Définir", desc: "Écrivez une simple struct Rust qui décrit votre table." },
    Step { title: "Dériver", desc: "Ajoutez #[derive(Model)] : colonnes, clés et SQL sont générés à la compilation." },
    Step { title: "Interroger", desc: "Composez des requêtes typées avec filtres, tri et limites." },
    Step { title: "Livrer", desc: "Déployez sur PostgreSQL, MySQL/MariaDB, SQLite, Redis, MongoDB ou en mémoire, du serveur au WebAssembly." },
];

static FR_WHY: [Step; 4] = [
    Step { title: "SQL construit à la compilation", desc: "Les métadonnées de table et de colonnes viennent de la macro derive : rien à découvrir à l'exécution." },
    Step { title: "Mapping monomorphisé", desc: "Chaque modèle a son propre code de mapping : ni réflexion, ni allocation superflue sur le chemin critique." },
    Step { title: "Constructeur de requêtes léger", desc: "Les requêtes sont de petites valeurs assemblées sur place et rendues une fois par dialecte." },
    Step { title: "Async sans surcoût", desc: "Les opérations sont asynchrones de bout en bout et ne bloquent jamais vos threads." },
];

static FR_BACKENDS: [Step; 6] = [
    Step { title: "En mémoire", desc: "Instantané et sans configuration : idéal pour les tests, les démos et le WebAssembly." },
    Step { title: "SQLite", desc: "Un seul fichier, parfait pour les apps, les outils en ligne de commande et l'edge." },
    Step { title: "PostgreSQL", desc: "Un pool de connexions pour la production." },
    Step { title: "MySQL & MariaDB", desc: "Un vrai pilote pour MySQL et MariaDB, avec identifiants entre backticks et paramètres ?." },
    Step { title: "Redis", desc: "Stockez vos modèles dans Redis, la base NoSQL clé-valeur ultra-rapide." },
    Step { title: "MongoDB", desc: "Associez vos modèles aux collections et documents MongoDB (NoSQL)." },
];

static FR: Texts = Texts {
    nav_home: "Accueil",
    nav_features: "Fonctionnalités",
    nav_performance: "Performance",
    nav_playground: "Playground",
    nav_docs: "Docs",
    nav_get_started: "Commencer",
    nav_open_menu: "Ouvrir le menu",
    nav_close_menu: "Fermer le menu",
    skip_to_content: "Aller au contenu",
    theme_toggle: "Basculer entre thème clair et sombre",
    language: "Langue",

    hero_badge: "100 % Rust · Async · Prêt pour WebAssembly",
    hero_title_1: "L'ORM Rust",
    hero_title_2: "taillé pour la vitesse.",
    hero_subtitle: "Memoire ORM transforme de simples structs Rust en requêtes typées et vérifiées à la compilation, pour PostgreSQL, MySQL/MariaDB, SQLite, Redis, MongoDB et un moteur en mémoire qui tourne même dans votre navigateur.",
    hero_taglines: &[
        "Des requêtes typées.",
        "Du SQL généré à la compilation.",
        "Un mapping sans surcoût.",
        "SQL et NoSQL, une seule API.",
        "Compatible WebAssembly.",
    ],
    hero_cta_primary: "Commencer",
    hero_cta_github: "GitHub",
    copy: "Copier",
    copied: "Copié !",
    copy_install: "Copier la commande d'installation",
    scroll: "Défiler",

    stat_rust: "Rust pur",
    stat_backends: "Bases supportées",
    stat_dialects: "Dialectes SQL",
    stat_guides: "Guides de documentation",

    marquee_items: &[
        "PostgreSQL", "MySQL", "MariaDB", "SQLite", "Redis", "MongoDB", "En mémoire", "WebAssembly",
        "async / await", "Migrations", "Transactions", "Macros derive", "Filtres typés",
    ],

    features_eyebrow: "Fonctionnalités",
    features_title: "Tout ce qu'il faut. Rien de superflu.",
    features_subtitle: "Une boîte à outils ciblée pour modéliser, interroger et migrer vos données avec la confiance du compilateur Rust.",
    features: &FR_FEATURES,
    features_more: "Voir toutes les fonctionnalités",

    code_eyebrow: "Expérience développeur",
    code_title: "Écrivez du Rust. Obtenez du SQL.",
    code_subtitle: "Modèles, requêtes, transactions et migrations se lisent comme le Rust que vous écrivez déjà.",
    code_tabs: ["Modèle", "Requête", "Transaction", "Migration"],
    terminal: "Terminal",

    how_eyebrow: "Comment ça marche",
    how_title: "De la struct à la production en quatre étapes",
    how_subtitle: "Pas de fichier de schéma à synchroniser, pas de SQL concaténé à la main. Vos types font foi.",
    how_steps: &FR_STEPS,

    perf_eyebrow: "Performance",
    perf_title: "Rapide par conception, mesuré avec Criterion",
    perf_subtitle: "Le travail est fait à la compilation pour que votre chemin critique reste léger.",
    perf_cta: "Voir les benchmarks",
    bench_lower_better: "Plus bas = meilleur",
    bench_placeholder: "Chiffres provisoires : les vrais résultats Criterion seront publiés ici prochainement.",
    bench_memoire: "Memoire",
    bench_baseline: "Référence",
    perf_page_title: "Performance",
    perf_page_subtitle: "Pourquoi Memoire est rapide, et comment nous le mesurons.",
    perf_why_title: "Pourquoi c'est rapide",
    perf_why: &FR_WHY,
    perf_results_title: "Résultats des benchmarks",
    perf_method_title: "Méthodologie",
    perf_method_text: "Les benchmarks sont écrits avec Criterion et se trouvent dans le dépôt. Chaque chiffre est la médiane de nombreuses itérations sur la même machine, et la référence est mesurée dans la même exécution pour une comparaison équitable.",
    perf_read_more: "Lire le guide performance",

    fp_title: "Fonctionnalités",
    fp_subtitle: "Tout ce que Memoire ORM vous apporte, expliqué simplement.",
    backends_title: "Une API, six bases",
    backends_subtitle: "SQL ou NoSQL : choisissez le stockage adapté, vos modèles ne changent pas.",
    backends: &FR_BACKENDS,

    cta_title: "Prêt à faire décoller vos données ?",
    cta_subtitle: "Ajoutez Memoire ORM à votre projet en une commande et suivez le démarrage rapide.",
    cta_primary: "Lire la documentation",
    cta_secondary: "Essayer le playground",

    footer_tagline: "L'ORM 100 % Rust ultra-rapide.",
    footer_mission: "Memoire ORM aide les développeurs Rust à travailler avec leurs bases de données grâce à des types fiables, du prototype à la production.",
    footer_product: "Produit",
    footer_docs: "Docs",
    footer_community: "Communauté",
    footer_issues: "Tickets",
    footer_deploy: "Déployer sur Heroku",
    footer_rights: "© 2026 Memoire ORM — MIT / Apache-2.0",
    footer_built: "Construit avec Rust 🦀 + Yew + WebAssembly",
    back_to_top: "Retour en haut",

    pg_title: "Playground",
    pg_subtitle: "Construisez une requête avec les contrôles et regardez le SQL et les résultats se mettre à jour en direct, dans votre navigateur.",
    pg_controls: "Constructeur de requête",
    pg_min_age: "Âge minimum",
    pg_name_contains: "Le nom contient",
    pg_name_placeholder: "ex. li",
    pg_order_by: "Trier par",
    pg_order_name: "Nom",
    pg_order_age: "Âge",
    pg_direction: "Ordre",
    pg_asc: "Croissant",
    pg_desc: "Décroissant",
    pg_limit: "Limite",
    pg_reset: "Réinitialiser",
    pg_rust: "Code Rust",
    pg_sql: "SQL généré",
    pg_params: "Paramètres liés",
    pg_result: "Résultat",
    pg_rows: "lignes",
    pg_no_rows: "Aucune ligne ne correspond à ces filtres.",
    pg_exec: "Exécuté en",
    pg_mock_note: "Aperçu : le playground utilise pour l'instant une simulation légère. Le vrai moteur en mémoire de Memoire compilé en WebAssembly arrive bientôt.",

    docs_title: "Documentation",
    docs_search: "Rechercher dans la doc…",
    docs_no_results: "Aucun résultat pour",
    docs_results: "résultats",
    docs_on_this_page: "Sur cette page",
    docs_prev: "Précédent",
    docs_next: "Suivant",
    docs_edit: "Modifier cette page sur GitHub",
    docs_min_read: "min de lecture",
    docs_menu: "Menu de la doc",
    docs_not_found_title: "Page introuvable",
    docs_not_found_text: "Cette page de documentation n'existe pas (encore). Choisissez un sujet dans le menu.",
    callout_note: "Remarque",
    callout_tip: "Astuce",
    callout_warning: "Attention",

    nf_title: "Perdu dans la mémoire",
    nf_text: "La page que vous cherchez n'existe pas ou a été déplacée.",
    nf_home: "Retour à l'accueil",
    nf_docs: "Parcourir la doc",

    meta_description: "Memoire ORM : un ORM 100 % Rust ultra-rapide pour SQL et NoSQL (PostgreSQL, MySQL/MariaDB, SQLite, Redis, MongoDB), avec SQL généré à la compilation, requêtes typées et moteur en mémoire compatible WebAssembly.",
};

// ---------------------------------------------------------------------------
// Español
// ---------------------------------------------------------------------------

static ES_FEATURES: [Feature; 8] = [
    Feature {
        title: "Rust puro",
        desc: "Sin toolchain de C ni sorpresas de FFI. Solo cargo add y a compilar.",
        detail: "Memoire está escrito íntegramente en Rust. Compila allí donde compila Rust, del servidor al navegador, y encaja de forma natural en tu flujo con cargo.",
    },
    Feature {
        title: "SQL en tiempo de compilación",
        desc: "Las macros derive generan tu SQL al compilar, no al ejecutar.",
        detail: "El derive Model convierte tu struct en metadatos de tabla y constantes de columna en tiempo de compilación. Una errata en el nombre de una columna es un error del compilador, no un incidente en producción.",
    },
    Feature {
        title: "Consultas con tipos seguros",
        desc: "Compón filtros con expresiones Rust reales, verificadas por el compilador.",
        detail: "Las columnas son constantes tipadas: User::AGE.ge(18) solo acepta números y User::NAME.contains(\"li\") solo acepta texto. Combina condiciones con & y | y deja que el compilador detecte los errores.",
    },
    Feature {
        title: "Mapeo sin coste",
        desc: "Las filas se convierten directamente en tus structs, sin capa de reflexión.",
        detail: "El código de mapeo se genera para cada modelo: convertir una fila en un User es Rust monomorfizado, sin reflexión en tiempo de ejecución ni despacho dinámico en la ruta crítica.",
    },
    Feature {
        title: "Varias bases de datos",
        desc: "SQL + NoSQL: PostgreSQL, MySQL/MariaDB, SQLite, Redis, MongoDB y un motor en memoria (WASM).",
        detail: "Seis bases de datos tras una sola API, tanto SQL como NoSQL. Escribe tus modelos una vez, usa MemoryDb en los tests y luego PostgreSQL, MySQL/MariaDB, SQLite, Redis o MongoDB en producción.",
    },
    Feature {
        title: "Motor en memoria, incluso en WASM",
        desc: "Un motor completo para tests y demos que también funciona en el navegador.",
        detail: "MemoryDb no necesita servidor ni archivo. Hace que los tests unitarios sean instantáneos y es lo bastante portable para ejecutarse en WebAssembly, como el playground de este sitio.",
    },
    Feature {
        title: "Async por defecto",
        desc: "Cada operación es asíncrona y encaja en tu runtime.",
        detail: "insert, update, las consultas, las transacciones y las migraciones son async, así que Memoire se integra en aplicaciones Rust modernas sin bloquear hilos.",
    },
    Feature {
        title: "Migraciones",
        desc: "Versionadas, ordenadas y generadas a partir de tus modelos.",
        detail: "Describe el historial de tu esquema con Migration::new(versión, nombre) y deja que el Migrator aplique lo que falte, en orden, al arrancar.",
    },
];

static ES_STEPS: [Step; 4] = [
    Step { title: "Definir", desc: "Escribe una struct de Rust normal que describa tu tabla." },
    Step { title: "Derivar", desc: "Añade #[derive(Model)]: columnas, claves y SQL se generan al compilar." },
    Step { title: "Consultar", desc: "Compón consultas con tipos seguros, con filtros, orden y límites." },
    Step { title: "Desplegar", desc: "Funciona con PostgreSQL, MySQL/MariaDB, SQLite, Redis, MongoDB o en memoria, del servidor a WebAssembly." },
];

static ES_WHY: [Step; 4] = [
    Step { title: "SQL construido al compilar", desc: "Los metadatos de tablas y columnas vienen de la macro derive: no hay nada que descubrir en tiempo de ejecución." },
    Step { title: "Mapeo monomorfizado", desc: "Cada modelo tiene su propio código de mapeo: sin reflexión ni asignaciones innecesarias en la ruta crítica." },
    Step { title: "Constructor de consultas ligero", desc: "Las consultas son pequeños valores que se ensamblan en el sitio y se renderizan una vez por dialecto." },
    Step { title: "Async sin sobrecoste", desc: "Las operaciones son asíncronas de principio a fin y nunca bloquean los hilos del ejecutor." },
];

static ES_BACKENDS: [Step; 6] = [
    Step { title: "En memoria", desc: "Instantáneo y sin configuración: perfecto para tests, demos y WebAssembly." },
    Step { title: "SQLite", desc: "Un solo archivo, ideal para apps, herramientas de línea de comandos y edge." },
    Step { title: "PostgreSQL", desc: "Conexiones agrupadas para cargas de producción." },
    Step { title: "MySQL & MariaDB", desc: "Un driver real para MySQL y MariaDB, con identificadores entre backticks y parámetros ?." },
    Step { title: "Redis", desc: "Guarda tus modelos en Redis, el rápido almacén NoSQL clave-valor." },
    Step { title: "MongoDB", desc: "Asocia tus modelos a colecciones y documentos de MongoDB (NoSQL)." },
];

static ES: Texts = Texts {
    nav_home: "Inicio",
    nav_features: "Funciones",
    nav_performance: "Rendimiento",
    nav_playground: "Playground",
    nav_docs: "Docs",
    nav_get_started: "Empezar",
    nav_open_menu: "Abrir menú",
    nav_close_menu: "Cerrar menú",
    skip_to_content: "Saltar al contenido",
    theme_toggle: "Cambiar entre tema claro y oscuro",
    language: "Idioma",

    hero_badge: "Rust puro · Async · Listo para WebAssembly",
    hero_title_1: "El ORM de Rust",
    hero_title_2: "hecho para la velocidad.",
    hero_subtitle: "Memoire ORM convierte structs de Rust normales en consultas con tipos seguros y verificadas al compilar, para PostgreSQL, MySQL/MariaDB, SQLite, Redis, MongoDB y un motor en memoria que funciona incluso en tu navegador.",
    hero_taglines: &[
        "Consultas con tipos seguros.",
        "SQL generado al compilar.",
        "Mapeo de filas sin coste.",
        "SQL y NoSQL, una sola API.",
        "Funciona en WebAssembly.",
    ],
    hero_cta_primary: "Empezar",
    hero_cta_github: "GitHub",
    copy: "Copiar",
    copied: "¡Copiado!",
    copy_install: "Copiar el comando de instalación",
    scroll: "Desplázate",

    stat_rust: "Rust puro",
    stat_backends: "Bases de datos",
    stat_dialects: "Dialectos SQL",
    stat_guides: "Guías de documentación",

    marquee_items: &[
        "PostgreSQL", "MySQL", "MariaDB", "SQLite", "Redis", "MongoDB", "En memoria", "WebAssembly",
        "async / await", "Migraciones", "Transacciones", "Macros derive", "Filtros tipados",
    ],

    features_eyebrow: "Funciones",
    features_title: "Todo lo que necesitas. Nada que sobre.",
    features_subtitle: "Un conjunto de herramientas enfocado para modelar, consultar y migrar tus datos con la confianza del compilador de Rust.",
    features: &ES_FEATURES,
    features_more: "Ver todas las funciones",

    code_eyebrow: "Experiencia de desarrollo",
    code_title: "Escribe Rust. Obtén SQL.",
    code_subtitle: "Modelos, consultas, transacciones y migraciones se leen como el Rust que ya escribes.",
    code_tabs: ["Modelo", "Consulta", "Transacción", "Migración"],
    terminal: "Terminal",

    how_eyebrow: "Cómo funciona",
    how_title: "De la struct a producción en cuatro pasos",
    how_subtitle: "Sin archivos de esquema que sincronizar ni SQL concatenado a mano. Tus tipos son la fuente de verdad.",
    how_steps: &ES_STEPS,

    perf_eyebrow: "Rendimiento",
    perf_title: "Rápido por diseño, medido con Criterion",
    perf_subtitle: "El trabajo se hace al compilar para que tu ruta crítica siga siendo ligera.",
    perf_cta: "Ver los benchmarks",
    bench_lower_better: "Menos es mejor",
    bench_placeholder: "Cifras provisionales: los resultados reales de Criterion se publicarán aquí pronto.",
    bench_memoire: "Memoire",
    bench_baseline: "Referencia",
    perf_page_title: "Rendimiento",
    perf_page_subtitle: "Por qué Memoire es rápido y cómo lo medimos.",
    perf_why_title: "Por qué es rápido",
    perf_why: &ES_WHY,
    perf_results_title: "Resultados de los benchmarks",
    perf_method_title: "Metodología",
    perf_method_text: "Los benchmarks están escritos con Criterion y viven en el repositorio. Cada cifra es la mediana de muchas iteraciones en la misma máquina, y la referencia se mide en la misma ejecución para que la comparación sea justa.",
    perf_read_more: "Leer la guía de rendimiento",

    fp_title: "Funciones",
    fp_subtitle: "Todo lo que te ofrece Memoire ORM, explicado de forma sencilla.",
    backends_title: "Una API, seis bases de datos",
    backends_subtitle: "SQL o NoSQL: elige el almacenamiento que te convenga, tus modelos no cambian.",
    backends: &ES_BACKENDS,

    cta_title: "¿Listo para que tus datos vuelen?",
    cta_subtitle: "Añade Memoire ORM a tu proyecto con un solo comando y sigue el inicio rápido.",
    cta_primary: "Leer la documentación",
    cta_secondary: "Probar el playground",

    footer_tagline: "El ORM ultrarrápido en Rust puro.",
    footer_mission: "Memoire ORM ayuda a los desarrolladores de Rust a trabajar con bases de datos mediante tipos fiables, del prototipo a producción.",
    footer_product: "Producto",
    footer_docs: "Docs",
    footer_community: "Comunidad",
    footer_issues: "Incidencias",
    footer_deploy: "Desplegar en Heroku",
    footer_rights: "© 2026 Memoire ORM — MIT / Apache-2.0",
    footer_built: "Hecho con Rust 🦀 + Yew + WebAssembly",
    back_to_top: "Volver arriba",

    pg_title: "Playground",
    pg_subtitle: "Construye una consulta con los controles y mira cómo el SQL y los resultados se actualizan en directo, en tu navegador.",
    pg_controls: "Constructor de consultas",
    pg_min_age: "Edad mínima",
    pg_name_contains: "El nombre contiene",
    pg_name_placeholder: "p. ej. li",
    pg_order_by: "Ordenar por",
    pg_order_name: "Nombre",
    pg_order_age: "Edad",
    pg_direction: "Dirección",
    pg_asc: "Ascendente",
    pg_desc: "Descendente",
    pg_limit: "Límite",
    pg_reset: "Restablecer",
    pg_rust: "Código Rust",
    pg_sql: "SQL generado",
    pg_params: "Parámetros enlazados",
    pg_result: "Resultado",
    pg_rows: "filas",
    pg_no_rows: "Ninguna fila coincide con estos filtros.",
    pg_exec: "Ejecutado en",
    pg_mock_note: "Vista previa: por ahora el playground usa una simulación ligera. El motor en memoria real de Memoire compilado a WebAssembly llegará pronto.",

    docs_title: "Documentación",
    docs_search: "Buscar en la documentación…",
    docs_no_results: "Sin resultados para",
    docs_results: "resultados",
    docs_on_this_page: "En esta página",
    docs_prev: "Anterior",
    docs_next: "Siguiente",
    docs_edit: "Editar esta página en GitHub",
    docs_min_read: "min de lectura",
    docs_menu: "Menú de la documentación",
    docs_not_found_title: "Página no encontrada",
    docs_not_found_text: "Esta página de documentación no existe (todavía). Elige un tema en el menú.",
    callout_note: "Nota",
    callout_tip: "Consejo",
    callout_warning: "Atención",

    nf_title: "Perdido en la memoria",
    nf_text: "La página que buscas no existe o se ha movido.",
    nf_home: "Volver al inicio",
    nf_docs: "Ver la documentación",

    meta_description: "Memoire ORM: un ORM ultrarrápido en Rust puro para SQL y NoSQL (PostgreSQL, MySQL/MariaDB, SQLite, Redis, MongoDB), con SQL generado al compilar, consultas con tipos seguros y un motor en memoria que funciona en WebAssembly.",
};
