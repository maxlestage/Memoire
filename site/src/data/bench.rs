//! Benchmark figures displayed on the home page teaser and on `/performance`.
//!
//! TODO(bench): filled from criterion results
//!
//! Every value below is a PLACEHOLDER. Replace them with the medians reported by
//! `cargo bench` (Criterion) and set `BENCH_IS_PLACEHOLDER` to `false`, which
//! removes the "placeholder figures" notice on the site.
//!
//! Field meaning:
//! * `label`    – what was measured, as `[english, french, spanish]`;
//! * `value`    – Memoire's measured time for the operation (lower is better);
//! * `unit`     – unit for both `value` and `baseline` (e.g. "ns", "µs", "ms");
//! * `baseline` – the reference measured in the same Criterion run (for example a
//!   hand-written implementation of the same operation). Use `0.0` to hide the
//!   baseline bar for an entry.

/// Flip to `false` once the numbers below come from real Criterion runs.
pub const BENCH_IS_PLACEHOLDER: bool = true;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct BenchEntry {
    /// `[en, fr, es]`
    pub label: [&'static str; 3],
    pub value: f64,
    pub unit: &'static str,
    pub baseline: f64,
}

// TODO(bench): filled from criterion results — all numbers below are placeholders.
pub const BENCHMARKS: &[BenchEntry] = &[
    BenchEntry {
        label: ["Insert one row (in-memory)", "Insérer une ligne (en mémoire)", "Insertar una fila (en memoria)"],
        value: 1.0,     // PLACEHOLDER
        unit: "µs",
        baseline: 2.0,  // PLACEHOLDER
    },
    BenchEntry {
        label: ["Select by primary key", "Sélection par clé primaire", "Selección por clave primaria"],
        value: 1.0,     // PLACEHOLDER
        unit: "µs",
        baseline: 2.0,  // PLACEHOLDER
    },
    BenchEntry {
        label: ["Filter + order 10k rows", "Filtrer + trier 10k lignes", "Filtrar + ordenar 10k filas"],
        value: 1.0,     // PLACEHOLDER
        unit: "ms",
        baseline: 2.0,  // PLACEHOLDER
    },
    BenchEntry {
        label: ["Map 1k rows to structs", "Mapper 1k lignes en structs", "Mapear 1k filas a structs"],
        value: 1.0,     // PLACEHOLDER
        unit: "µs",
        baseline: 2.0,  // PLACEHOLDER
    },
    BenchEntry {
        label: ["Build & render a SELECT", "Construire et rendre un SELECT", "Construir y renderizar un SELECT"],
        value: 1.0,     // PLACEHOLDER
        unit: "ns",
        baseline: 2.0,  // PLACEHOLDER
    },
];
