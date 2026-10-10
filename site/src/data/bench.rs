//! Benchmark figures displayed on the home page teaser and on `/performance`.
//!
//! Real medians reported by `cargo bench` (Criterion). Update them when the
//! benchmarks change; `BENCH_IS_PLACEHOLDER = true` shows a notice instead.
//!
//! Field meaning:
//! * `label`    – what was measured, as `[english, french, spanish]`;
//! * `value`    – Memoire's measured time for the operation (lower is better);
//! * `unit`     – unit for both `value` and `baseline` (e.g. "ns", "µs", "ms");
//! * `baseline` – the reference measured in the same Criterion run (for example a
//!   hand-written implementation of the same operation). Use `0.0` to hide the
//!   baseline bar for an entry.

/// Flip to `false` once the numbers below come from real Criterion runs.
pub const BENCH_IS_PLACEHOLDER: bool = false;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct BenchEntry {
    /// `[en, fr, es]`
    pub label: [&'static str; 3],
    pub value: f64,
    pub unit: &'static str,
    pub baseline: f64,
}

// Medians from `cargo bench -p memoire-orm --features memory,sqlite` (Criterion,
// Intel Xeon 2.8 GHz, Rust 1.97) — see crates/memoire-orm/benches/RESULTS.md.
// `baseline` is hand-written rusqlite code doing the same work (0.0 = no baseline).
pub const BENCHMARKS: &[BenchEntry] = &[
    BenchEntry {
        label: [
            "Insert 1 000 rows in a transaction (SQLite)",
            "Insérer 1 000 lignes en transaction (SQLite)",
            "Insertar 1 000 filas en una transacción (SQLite)",
        ],
        value: 1.80,
        unit: "ms",
        baseline: 7.98,
    },
    BenchEntry {
        label: [
            "Select + decode 1 000 rows (SQLite)",
            "Sélectionner + décoder 1 000 lignes (SQLite)",
            "Seleccionar + decodificar 1 000 filas (SQLite)",
        ],
        value: 343.0,
        unit: "µs",
        baseline: 324.0,
    },
    BenchEntry {
        label: ["Find by primary key (SQLite)", "Recherche par clé primaire (SQLite)", "Búsqueda por clave primaria (SQLite)"],
        value: 987.0,
        unit: "ns",
        baseline: 840.0,
    },
    BenchEntry {
        label: [
            "Find by key in 10 000 rows (in-memory)",
            "Recherche par clé parmi 10 000 lignes (en mémoire)",
            "Búsqueda por clave entre 10 000 filas (en memoria)",
        ],
        value: 231.0,
        unit: "ns",
        baseline: 0.0,
    },
    BenchEntry {
        label: [
            "Filter + sort 10 000 rows, top 20 (in-memory)",
            "Filtrer + trier 10 000 lignes, top 20 (en mémoire)",
            "Filtrar + ordenar 10 000 filas, top 20 (en memoria)",
        ],
        value: 241.0,
        unit: "µs",
        baseline: 0.0,
    },
    BenchEntry {
        label: ["Build & render a SELECT to SQL", "Construire et rendre un SELECT en SQL", "Construir y renderizar un SELECT a SQL"],
        value: 529.0,
        unit: "ns",
        baseline: 0.0,
    },
];
