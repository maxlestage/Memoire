//! `/performance`: why Memoire is fast + benchmark bars (data in `data/bench.rs`).

use yew::prelude::*;
use yew_router::prelude::*;

use crate::app::Route;
use crate::components::{
    bench_bars::BenchBars, icons::Icon, reveal::Reveal, section::SectionHead, tilt_card::TiltCard,
};
use crate::data::samples;
use crate::docs;
use crate::highlight::highlight;
use crate::i18n::use_lang;

#[component]
pub fn PerformancePage() -> Html {
    let lang = use_lang().lang;
    let t = lang.t();
    let rust = highlight(
        "User::query()\n    .filter(User::AGE.ge(18) & User::NAME.contains(\"li\"))\n    .order_by(User::NAME.asc())\n    .limit(20)",
        "rust",
    );
    let sql = highlight(samples::SQL_EXAMPLE, "sql");
    html! {
        <>
            <section class="page-hero container">
                <span class="eyebrow hero-anim">{ t.perf_eyebrow }</span>
                <h1 class="page-title hero-anim shimmer" style="--d:80ms">{ t.perf_page_title }</h1>
                <p class="page-subtitle hero-anim" style="--d:160ms">{ t.perf_page_subtitle }</p>
            </section>

            <section class="section container">
                <SectionHead eyebrow={t.perf_eyebrow} title={t.perf_why_title} />
                <div class="why-grid">
                    { for t.perf_why.iter().enumerate().map(|(i, w)| html! {
                        <Reveal anim="fade-up" delay={(i as u32) * 90}>
                            <TiltCard class="why-card">
                                <span class="why-num">{ format!("{:02}", i + 1) }</span>
                                <h3>{ w.title }</h3>
                                <p>{ w.desc }</p>
                            </TiltCard>
                        </Reveal>
                    }) }
                </div>
            </section>

            <section class="section container">
                <Reveal class="rust-to-sql">
                    <div class="mini-window glass">
                        <div class="mini-head"><span class="cw-dots" aria-hidden="true"><i></i><i></i><i></i></span><span class="code-lang">{ "rust" }</span></div>
                        <pre><code>{ Html::from_html_unchecked(AttrValue::from(rust)) }</code></pre>
                    </div>
                    <div class="rts-arrow" aria-hidden="true">{ Icon::ArrowRight.html() }</div>
                    <div class="mini-window glass">
                        <div class="mini-head"><span class="cw-dots" aria-hidden="true"><i></i><i></i><i></i></span><span class="code-lang">{ "PostgreSQL" }</span></div>
                        <pre class="wrap"><code>{ Html::from_html_unchecked(AttrValue::from(sql)) }</code></pre>
                    </div>
                </Reveal>
            </section>

            <section class="section container">
                <SectionHead eyebrow="Criterion" title={t.perf_results_title} />
                <Reveal class="perf-card glass spotlight wide" anim="scale-in">
                    <BenchBars />
                </Reveal>
            </section>

            <section class="section container narrow">
                <Reveal class="method glass">
                    <h2>{ t.perf_method_title }</h2>
                    <p>{ t.perf_method_text }</p>
                    <div class="method-links">
                        <Link<Route> to={Route::Doc { slug: "performance".into() }} classes={classes!("btn", "btn-ghost", "magnetic")}>
                            { Icon::Book.html() }<span>{ t.perf_read_more }</span>
                        </Link<Route>>
                        <Link<Route> to={Route::Doc { slug: "benchmarks".into() }} classes={classes!("btn", "btn-ghost", "magnetic")}>
                            { Icon::Activity.html() }<span>{ docs::title("benchmarks", lang) }</span>
                        </Link<Route>>
                    </div>
                </Reveal>
            </section>
        </>
    }
}
