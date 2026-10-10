//! Animated benchmark bars (they grow when scrolled into view).
//! Data comes from `crate::data::bench`.

use yew::prelude::*;

use super::reveal::Reveal;
use crate::data::bench::{BENCHMARKS, BENCH_IS_PLACEHOLDER};
use crate::i18n::use_lang;

#[derive(Properties, PartialEq)]
pub struct BenchBarsProps {
    /// Show only the first N entries (home page teaser).
    #[prop_or_default]
    pub limit: Option<usize>,
}

fn fmt_value(v: f64) -> String {
    if v >= 100.0 {
        format!("{v:.0}")
    } else if v >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}

#[component]
pub fn BenchBars(props: &BenchBarsProps) -> Html {
    let ctx = use_lang();
    let t = ctx.lang.t();
    let li = ctx.lang.idx();
    let entries = &BENCHMARKS[..props.limit.unwrap_or(BENCHMARKS.len()).min(BENCHMARKS.len())];

    html! {
        <div class="bench">
            if BENCH_IS_PLACEHOLDER {
                <p class="bench-note">{ t.bench_placeholder }</p>
            }
            <div class="bench-legend">
                <span class="legend memoire"><i></i>{ t.bench_memoire }</span>
                <span class="legend baseline"><i></i>{ t.bench_baseline }</span>
                <span class="legend-note">{ t.bench_lower_better }</span>
            </div>
            { for entries.iter().enumerate().map(|(i, b)| {
                let max = b.value.max(b.baseline).max(f64::EPSILON);
                let w_mem = (b.value / max * 100.0).clamp(2.0, 100.0);
                let w_base = (b.baseline / max * 100.0).clamp(2.0, 100.0);
                // No speed-up badge on placeholder numbers: they are not measurements.
                let ratio = (!BENCH_IS_PLACEHOLDER && b.baseline > 0.0 && b.value > 0.0)
                    .then(|| b.baseline / b.value);
                html! {
                    <Reveal class="bench-row" delay={(i as u32) * 90}>
                        <div class="bench-label">
                            <span>{ b.label[li] }</span>
                            if let Some(r) = ratio {
                                <span class="bench-ratio">{ format!("×{r:.1}") }</span>
                            }
                        </div>
                        <div class="bar memoire" style={format!("--w:{w_mem:.1}%")}>
                            <span class="bar-fill"></span>
                            <span class="bar-value">{ format!("{} {}", fmt_value(b.value), b.unit) }</span>
                        </div>
                        if b.baseline > 0.0 {
                            <div class="bar baseline" style={format!("--w:{w_base:.1}%")}>
                                <span class="bar-fill"></span>
                                <span class="bar-value">{ format!("{} {}", fmt_value(b.baseline), b.unit) }</span>
                            </div>
                        }
                    </Reveal>
                }
            }) }
        </div>
    }
}
