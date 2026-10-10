//! `/features`: every feature explained, plus the supported backends.

use yew::prelude::*;

use super::home::CtaBanner;
use crate::components::{
    icons::{Icon, FEATURE_ICONS},
    reveal::Reveal,
    section::SectionHead,
    tilt_card::TiltCard,
};
use crate::data::samples::FEATURE_SNIPPETS;
use crate::highlight::highlight;
use crate::i18n::use_lang;

const BACKEND_ICONS: [Icon; 4] = [Icon::Cpu, Icon::Database, Icon::Database, Icon::Layers];
const BACKEND_CODE: [&str; 4] = [
    "MemoryDb::new()",
    "SqliteDb::open(\"app.db\")?",
    "PgPool::connect(url).await?",
    "WHERE `users`.`age` >= ?",
];

#[component]
pub fn FeaturesPage() -> Html {
    let t = use_lang().lang.t();
    html! {
        <>
            <section class="page-hero container">
                <span class="eyebrow hero-anim">{ t.features_eyebrow }</span>
                <h1 class="page-title hero-anim shimmer" style="--d:80ms">{ t.fp_title }</h1>
                <p class="page-subtitle hero-anim" style="--d:160ms">{ t.fp_subtitle }</p>
            </section>

            <section class="container feature-rows">
                { for t.features.iter().enumerate().map(|(i, f)| {
                    let (lang, code) = FEATURE_SNIPPETS[i];
                    let code_html = highlight(code, lang);
                    html! {
                        <div class={classes!("feature-row", (i % 2 == 1).then_some("reverse"))}>
                            <Reveal class="feature-text" anim={if i % 2 == 1 { "slide-left" } else { "slide-right" }}>
                                <div class={classes!("feature-icon", "lg", format!("fi-{}", i % 4))}>{ FEATURE_ICONS[i].html() }</div>
                                <span class="feature-index">{ format!("{:02}", i + 1) }</span>
                                <h2>{ f.title }</h2>
                                <p class="lead">{ f.desc }</p>
                                <p>{ f.detail }</p>
                            </Reveal>
                            <Reveal class="feature-code" anim="scale-in" delay={120}>
                                <div class="mini-window glass spotlight">
                                    <div class="mini-head"><span class="cw-dots" aria-hidden="true"><i></i><i></i><i></i></span><span class="code-lang">{ lang }</span></div>
                                    <pre><code>{ Html::from_html_unchecked(AttrValue::from(code_html)) }</code></pre>
                                </div>
                            </Reveal>
                        </div>
                    }
                }) }
            </section>

            <section class="section container">
                <SectionHead eyebrow="Backends" title={t.backends_title} subtitle={AttrValue::from(t.backends_subtitle)} />
                <div class="backends-grid">
                    { for t.backends.iter().enumerate().map(|(i, b)| html! {
                        <Reveal anim="scale-in" delay={(i as u32) * 90}>
                            <TiltCard class="backend-card">
                                <div class={classes!("feature-icon", format!("fi-{}", i % 4))}>{ BACKEND_ICONS[i].html() }</div>
                                <h3>{ b.title }</h3>
                                <code class="backend-code">{ BACKEND_CODE[i] }</code>
                                <p>{ b.desc }</p>
                            </TiltCard>
                        </Reveal>
                    }) }
                </div>
            </section>

            <CtaBanner />
        </>
    }
}
