//! Landing page.

use yew::prelude::*;
use yew_router::prelude::*;

use crate::app::Route;
use crate::components::{
    bench_bars::BenchBars,
    code_window::CodeShowcase,
    copy_pill::CopyPill,
    counter::Counter,
    icons::{Icon, FEATURE_ICONS},
    marquee::Marquee,
    particles::Particles,
    reveal::Reveal,
    section::SectionHead,
    tilt_card::TiltCard,
    typing::Typing,
};
use crate::data::samples;
use crate::docs::{self, REPO_URL};
use crate::i18n::{use_lang, Texts};

#[component]
pub fn HomePage() -> Html {
    let t = use_lang().lang.t();
    html! {
        <>
            <Hero />
            <Stats />
            <Marquee items={t.marquee_items} />
            <FeaturesSection />
            <section class="section container">
                <SectionHead eyebrow={t.code_eyebrow} title={t.code_title} subtitle={AttrValue::from(t.code_subtitle)} />
                <Reveal anim="fade-up"><CodeShowcase /></Reveal>
            </section>
            <HowItWorks />
            <PerfTeaser />
            <CtaBanner />
        </>
    }
}

#[component]
fn Hero() -> Html {
    let t = use_lang().lang.t();
    html! {
        <section class="hero">
            <Particles />
            <div class="hero-glow" aria-hidden="true"></div>
            <div class="container hero-inner">
                <span class="badge hero-anim" style="--d:0ms">
                    <span class="badge-dot" aria-hidden="true"></span>{ t.hero_badge }
                </span>
                <h1 class="hero-title">
                    <span class="hero-line hero-anim" style="--d:80ms">{ t.hero_title_1 }</span>
                    <span class="hero-line hero-anim shimmer" style="--d:180ms">{ t.hero_title_2 }</span>
                </h1>
                <p class="hero-typing hero-anim" style="--d:280ms">
                    <span class="typing-prefix" aria-hidden="true">{ "›" }</span>
                    <Typing lines={t.hero_taglines} />
                </p>
                <p class="hero-subtitle hero-anim" style="--d:360ms">{ t.hero_subtitle }</p>
                <div class="hero-ctas hero-anim" style="--d:440ms">
                    <Link<Route> to={Route::Doc { slug: "quick-start".into() }} classes={classes!("btn", "btn-primary", "btn-lg", "magnetic")}>
                        <span>{ t.hero_cta_primary }</span>{ Icon::ArrowRight.html() }
                    </Link<Route>>
                    <a class="btn btn-ghost btn-lg magnetic" href={REPO_URL} target="_blank" rel="noopener noreferrer">
                        { Icon::Github.html() }<span>{ t.hero_cta_github }</span>
                    </a>
                </div>
                <div class="hero-anim" style="--d:520ms">
                    <CopyPill text={samples::INSTALL} />
                </div>
            </div>
            <div class="scroll-hint" aria-hidden="true">
                <span class="mouse"><span class="wheel"></span></span>
                <span>{ t.scroll }</span>
            </div>
        </section>
    }
}

#[component]
fn Stats() -> Html {
    let t = use_lang().lang.t();
    let stats: [(f64, &str, &str); 4] = [
        (100.0, "%", t.stat_rust),
        (6.0, "", t.stat_backends),
        (3.0, "", t.stat_dialects),
        (docs::page_count() as f64, "", t.stat_guides),
    ];
    html! {
        <section class="container stats" aria-label="Stats">
            { for stats.iter().enumerate().map(|(i, (value, suffix, label))| html! {
                <Reveal class="stat spotlight" anim="scale-in" delay={(i as u32) * 90}>
                    <span class="stat-value"><Counter value={*value} suffix={AttrValue::from(*suffix)} /></span>
                    <span class="stat-label">{ *label }</span>
                </Reveal>
            }) }
        </section>
    }
}

pub fn feature_cards(t: &'static Texts) -> Html {
    html! {
        <div class="features-grid">
            { for t.features.iter().enumerate().map(|(i, f)| html! {
                <Reveal anim="scale-in" delay={((i % 4) as u32) * 80}>
                    <TiltCard class="feature-card">
                        <div class={classes!("feature-icon", format!("fi-{}", i % 4))}>{ FEATURE_ICONS[i].html() }</div>
                        <h3>{ f.title }</h3>
                        <p>{ f.desc }</p>
                    </TiltCard>
                </Reveal>
            }) }
        </div>
    }
}

#[component]
fn FeaturesSection() -> Html {
    let t = use_lang().lang.t();
    html! {
        <section class="section container" id="features">
            <SectionHead eyebrow={t.features_eyebrow} title={t.features_title} subtitle={AttrValue::from(t.features_subtitle)} />
            { feature_cards(t) }
            <Reveal class="center-row">
                <Link<Route> to={Route::Features} classes={classes!("btn", "btn-ghost", "magnetic")}>
                    <span>{ t.features_more }</span>{ Icon::ArrowRight.html() }
                </Link<Route>>
            </Reveal>
        </section>
    }
}

#[component]
fn HowItWorks() -> Html {
    let t = use_lang().lang.t();
    html! {
        <section class="section container how">
            <SectionHead eyebrow={t.how_eyebrow} title={t.how_title} subtitle={AttrValue::from(t.how_subtitle)} />
            <Reveal class="timeline" anim="timeline-anim">
                <div class="timeline-track" aria-hidden="true"><span class="timeline-progress"></span></div>
                { for t.how_steps.iter().enumerate().map(|(i, s)| html! {
                    <div class="timeline-step" style={format!("--i:{i}")}>
                        <span class="step-num">{ format!("{:02}", i + 1) }</span>
                        <h3>{ s.title }</h3>
                        <p>{ s.desc }</p>
                    </div>
                }) }
            </Reveal>
        </section>
    }
}

#[component]
fn PerfTeaser() -> Html {
    let t = use_lang().lang.t();
    html! {
        <section class="section container perf-teaser">
            <div class="perf-copy">
                <SectionHead eyebrow={t.perf_eyebrow} title={t.perf_title} subtitle={AttrValue::from(t.perf_subtitle)} center={false} />
                <Reveal>
                    <Link<Route> to={Route::Performance} classes={classes!("btn", "btn-ghost", "magnetic")}>
                        <span>{ t.perf_cta }</span>{ Icon::ArrowRight.html() }
                    </Link<Route>>
                </Reveal>
            </div>
            <Reveal class="perf-card glass spotlight" anim="slide-left">
                <BenchBars limit={Some(3)} />
            </Reveal>
        </section>
    }
}

#[component]
pub fn CtaBanner() -> Html {
    let t = use_lang().lang.t();
    html! {
        <section class="section container">
            <Reveal class="cta-banner" anim="scale-in">
                <div class="cta-glow" aria-hidden="true"></div>
                <h2>{ t.cta_title }</h2>
                <p>{ t.cta_subtitle }</p>
                <div class="cta-actions">
                    <Link<Route> to={Route::Doc { slug: "quick-start".into() }} classes={classes!("btn", "btn-primary", "btn-lg", "magnetic")}>
                        { Icon::Book.html() }<span>{ t.cta_primary }</span>
                    </Link<Route>>
                    <Link<Route> to={Route::Playground} classes={classes!("btn", "btn-ghost", "btn-lg", "magnetic")}>
                        { Icon::Play.html() }<span>{ t.cta_secondary }</span>
                    </Link<Route>>
                </div>
                <CopyPill text={samples::INSTALL} />
            </Reveal>
        </section>
    }
}
