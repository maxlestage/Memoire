//! Every page of the site, written with active.

use active::prelude::*;

use super::Site;
use super::layout::{
    AUTHOR, Ctx, LICENSE_PATH, alternates, base_document, copy_pill, doc_href, document, dots, href, json, logo_mark,
    mini_window, page_hero, reveal, section_head, tilt_card,
};
use crate::data::bench::{BENCH_IS_PLACEHOLDER, BENCHMARKS};
use crate::data::samples;
use crate::docs::{self, CATEGORIES};
use crate::highlight::highlight;
use crate::i18n::{Texts, t};
use crate::icons::{FEATURE_ICONS, Icon, icon};
use crate::routes::{Lang, Page, Route, base, link};

/// Renders the page of `ctx.route`.
pub fn render(ctx: &Ctx<'_>) -> String {
    let lang = ctx.route.lang;
    let content = match &ctx.route.page {
        Page::Home => home(ctx),
        Page::Features => features(ctx),
        Page::Performance => performance(ctx),
        Page::Playground => playground(ctx),
        Page::Docs => doc_page(ctx, docs::FIRST_SLUG),
        Page::Doc(slug) => doc_page(ctx, slug),
        Page::NotFound => not_found(lang),
    };
    document(ctx, content)
}

/// The SEO of an ordinary page: title, description, canonical and alternates.
fn page_seo(ctx: &Ctx<'_>, title: &str, description: &str) -> Seo {
    alternates(
        Seo::new().title(title.to_string()).description(description.to_string()).canonical(ctx.route.canonical_path()),
        ctx.route,
    )
}

// ---------------------------------------------------------------------------
// Home
// ---------------------------------------------------------------------------

fn home(ctx: &Ctx<'_>) -> Node {
    let lang = ctx.route.lang;
    let tx = t(lang);
    let seo = page_seo(ctx, &format!("Memoire ORM — {}", tx.footer_tagline), tx.meta_description)
        .title_template("%s")
        .json_ld(format!(
            r#"{{"@context":"https://schema.org","@type":"SoftwareSourceCode","name":"Memoire ORM","description":{},"programmingLanguage":"Rust","license":"{}{LICENSE_PATH}","author":{{"@type":"Person","name":"{AUTHOR}"}},"url":{},"inLanguage":"{}"}}"#,
            json(tx.meta_description),
            ctx.origin,
            json(&format!("{}{}", ctx.origin, ctx.route.path())),
            lang.code()
        ));
    fragment([
        Node::from(seo),
        hero(lang).into(),
        stats(lang).into(),
        marquee(tx.marquee_items).into(),
        section()
            .class("section container")
            .id("features")
            .child(section_head(tx.features_eyebrow, tx.features_title, Some(tx.features_subtitle), true))
            .child(feature_cards(tx))
            .child(
                reveal("fade-up", "center-row", 0).child(
                    a().class("btn btn-ghost magnetic")
                        .href(href(lang, Page::Features))
                        .child(span().text(tx.features_more))
                        .child(icon(Icon::ArrowRight)),
                ),
            )
            .into(),
        section()
            .class("section container")
            .child(section_head(tx.code_eyebrow, tx.code_title, Some(tx.code_subtitle), true))
            .child(reveal("fade-up", "", 0).child(island("showcase", lang.code(), crate::islands::showcase)))
            .into(),
        how_it_works(tx).into(),
        perf_teaser(lang).into(),
        cta_banner(lang).into(),
    ])
}

fn hero(lang: Lang) -> Element {
    let tx = t(lang);
    section()
        .class("hero")
        .child(canvas().class("particles").attr("aria-hidden", "true"))
        .child(div().class("hero-glow").attr("aria-hidden", "true"))
        .child(
            div()
                .class("container hero-inner")
                .child(
                    span()
                        .class("badge hero-anim")
                        .style("--d:0ms")
                        .child(span().class("badge-dot").attr("aria-hidden", "true"))
                        .text(tx.hero_badge),
                )
                .child(
                    h1().class("hero-title")
                        .child(span().class("hero-line hero-anim").style("--d:80ms").text(tx.hero_title_1))
                        .child(span().class("hero-line hero-anim shimmer").style("--d:180ms").text(tx.hero_title_2)),
                )
                .child(
                    p().class("hero-typing hero-anim")
                        .style("--d:280ms")
                        .child(span().class("typing-prefix").attr("aria-hidden", "true").text("›"))
                        .child(island("typing", lang.code(), crate::islands::typing)),
                )
                .child(p().class("hero-subtitle hero-anim").style("--d:360ms").text(tx.hero_subtitle))
                .child(
                    div().class("hero-ctas hero-anim").style("--d:440ms").child(
                        a().class("btn btn-primary btn-lg magnetic")
                            .href(doc_href(lang, "quick-start"))
                            .child(span().text(tx.hero_cta_primary))
                            .child(icon(Icon::ArrowRight)),
                    ),
                )
                .child(div().class("hero-anim").style("--d:520ms").child(copy_pill(lang, samples::INSTALL))),
        )
        .child(
            div()
                .class("scroll-hint")
                .attr("aria-hidden", "true")
                .child(span().class("mouse").child(span().class("wheel")))
                .child(span().text(tx.scroll)),
        )
}

/// Numbers that count up when they scroll into view (the browser bundle animates
/// `data-count`; the page shows the final value).
fn stats(lang: Lang) -> Element {
    let tx = t(lang);
    let stats: [(usize, &str, &str); 4] = [
        (100, "%", tx.stat_rust),
        (6, "", tx.stat_backends),
        (3, "", tx.stat_dialects),
        (docs::page_count(), "", tx.stat_guides),
    ];
    section().class("container stats").attr("aria-label", "Stats").children(stats.iter().enumerate().map(
        |(i, (value, suffix, label))| {
            reveal("scale-in", "stat spotlight", i as u32 * 90)
                .child(
                    span().class("stat-value").child(
                        span()
                            .class("counter")
                            .attr("data-count", value.to_string())
                            .attr("data-suffix", *suffix)
                            .text(format!("{value}{suffix}")),
                    ),
                )
                .child(span().class("stat-label").text(*label))
        },
    ))
}

/// Infinite horizontal marquee (pure CSS animation, content duplicated once).
fn marquee(items: &'static [&'static str]) -> Element {
    let group = |hidden: bool| {
        let list = ul().class("marquee-group").children(items.iter().map(|item| {
            li().class("marquee-item")
                .child(span().class("marquee-star").attr("aria-hidden", "true").text("✦"))
                .text(*item)
        }));
        if hidden { list.attr("aria-hidden", "true") } else { list }
    };
    div().class("marquee").child(div().class("marquee-track").child(group(false)).child(group(true)))
}

fn feature_cards(tx: &Texts) -> Element {
    div().class("features-grid").children(tx.features.iter().enumerate().map(|(i, f)| {
        reveal("scale-in", "", (i % 4) as u32 * 80).child(tilt_card(
            "feature-card",
            vec![
                div().class(format!("feature-icon fi-{}", i % 4)).child(icon(FEATURE_ICONS[i])).into(),
                h3().text(f.title).into(),
                p().text(f.desc).into(),
            ],
        ))
    }))
}

fn how_it_works(tx: &Texts) -> Element {
    section()
        .class("section container how")
        .child(section_head(tx.how_eyebrow, tx.how_title, Some(tx.how_subtitle), true))
        .child(
            reveal("timeline-anim", "timeline", 0)
                .child(
                    div().class("timeline-track").attr("aria-hidden", "true").child(span().class("timeline-progress")),
                )
                .children(tx.how_steps.iter().enumerate().map(|(i, s)| {
                    div()
                        .class("timeline-step")
                        .style(format!("--i:{i}"))
                        .child(span().class("step-num").text(format!("{:02}", i + 1)))
                        .child(h3().text(s.title))
                        .child(p().text(s.desc))
                })),
        )
}

fn perf_teaser(lang: Lang) -> Element {
    let tx = t(lang);
    section()
        .class("section container perf-teaser")
        .child(
            div()
                .class("perf-copy")
                .child(section_head(tx.perf_eyebrow, tx.perf_title, Some(tx.perf_subtitle), false))
                .child(
                    reveal("fade-up", "", 0).child(
                        a().class("btn btn-ghost magnetic")
                            .href(href(lang, Page::Performance))
                            .child(span().text(tx.perf_cta))
                            .child(icon(Icon::ArrowRight)),
                    ),
                ),
        )
        .child(reveal("slide-left", "perf-card glass spotlight", 0).child(bench_bars(lang, Some(3))))
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

/// Benchmark bars (they grow when scrolled into view).
fn bench_bars(lang: Lang, limit: Option<usize>) -> Element {
    let tx = t(lang);
    let li = lang.idx();
    let entries = &BENCHMARKS[..limit.unwrap_or(BENCHMARKS.len()).min(BENCHMARKS.len())];
    div()
        .class("bench")
        .child(BENCH_IS_PLACEHOLDER.then(|| p().class("bench-note").text(tx.bench_placeholder)))
        .child(
            div()
                .class("bench-legend")
                .child(span().class("legend memoire").child(i()).text(tx.bench_memoire))
                .child(span().class("legend baseline").child(i()).text(tx.bench_baseline))
                .child(span().class("legend-note").text(tx.bench_lower_better)),
        )
        .children(entries.iter().enumerate().map(|(i, b)| {
            let max = b.value.max(b.baseline).max(f64::EPSILON);
            let w_mem = (b.value / max * 100.0).clamp(2.0, 100.0);
            let w_base = (b.baseline / max * 100.0).clamp(2.0, 100.0);
            // No speed-up badge on placeholder numbers: they are not measurements.
            let ratio = (!BENCH_IS_PLACEHOLDER && b.baseline > 0.0 && b.value > 0.0).then(|| b.baseline / b.value);
            let bar = |class: &str, width: f64, value: f64| {
                div()
                    .class(format!("bar {class}"))
                    .style(format!("--w:{width:.1}%"))
                    .child(span().class("bar-fill"))
                    .child(span().class("bar-value").text(format!("{} {}", fmt_value(value), b.unit)))
            };
            reveal("fade-up", "bench-row", i as u32 * 90)
                .child(
                    div()
                        .class("bench-label")
                        .child(span().text(b.label[li]))
                        .child(ratio.map(|r| span().class("bench-ratio").text(format!("×{r:.1}")))),
                )
                .child(bar("Memoire", w_mem, b.value))
                .child((b.baseline > 0.0).then(|| bar("baseline", w_base, b.baseline)))
        }))
}

fn cta_banner(lang: Lang) -> Element {
    let tx = t(lang);
    section().class("section container").child(
        reveal("scale-in", "cta-banner", 0)
            .child(div().class("cta-glow").attr("aria-hidden", "true"))
            .child(h2().text(tx.cta_title))
            .child(p().text(tx.cta_subtitle))
            .child(
                div()
                    .class("cta-actions")
                    .child(
                        a().class("btn btn-primary btn-lg magnetic")
                            .href(doc_href(lang, "quick-start"))
                            .child(icon(Icon::Book))
                            .child(span().text(tx.cta_primary)),
                    )
                    .child(
                        a().class("btn btn-ghost btn-lg magnetic")
                            .href(href(lang, Page::Playground))
                            .child(icon(Icon::Play))
                            .child(span().text(tx.cta_secondary)),
                    ),
            )
            .child(copy_pill(lang, samples::INSTALL)),
    )
}

// ---------------------------------------------------------------------------
// Features, performance, playground
// ---------------------------------------------------------------------------

/// Same order as `Texts::backends`.
const BACKEND_ICONS: [Icon; 6] = [Icon::Cpu, Icon::Database, Icon::Database, Icon::Database, Icon::Zap, Icon::Layers];

const BACKEND_HINTS: [&str; 6] = [
    "MemoryDb::new()",
    "SqliteDb::open(\"app.db\")?",
    "PgPool::connect(url).await?",
    "MySqlPool::connect(url).await?",
    "RedisDb::connect(url).await?",
    "MongoDb::connect(url).await?",
];

fn features(ctx: &Ctx<'_>) -> Node {
    let lang = ctx.route.lang;
    let tx = t(lang);
    // "Backends" / "Bases de données" / "Bases de datos": the docs category's wording.
    let backends_eyebrow = CATEGORIES[2].title[lang.idx()];
    fragment([
        Node::from(page_seo(ctx, tx.fp_title, tx.fp_subtitle)),
        page_hero(tx.features_eyebrow, tx.fp_title, tx.fp_subtitle).into(),
        section()
            .class("container feature-rows")
            .children(tx.features.iter().enumerate().map(|(i, f)| {
                let (code_lang, snippet) = samples::FEATURE_SNIPPETS[i];
                let odd = i % 2 == 1;
                div()
                    .class(if odd { "feature-row reverse" } else { "feature-row" })
                    .child(
                        reveal(if odd { "slide-left" } else { "slide-right" }, "feature-text", 0)
                            .child(div().class(format!("feature-icon lg fi-{}", i % 4)).child(icon(FEATURE_ICONS[i])))
                            .child(span().class("feature-index").text(format!("{:02}", i + 1)))
                            .child(h2().text(f.title))
                            .child(p().class("lead").text(f.desc))
                            .child(p().text(f.detail)),
                    )
                    .child(
                        reveal("scale-in", "feature-code", 120).child(
                            div()
                                .class("mini-window glass spotlight")
                                .child(
                                    div()
                                        .class("mini-head")
                                        .child(dots())
                                        .child(span().class("code-lang").text(code_lang)),
                                )
                                .child(pre().child(code().html(highlight(snippet, code_lang)))),
                        ),
                    )
            }))
            .into(),
        section()
            .class("section container")
            .child(section_head(backends_eyebrow, tx.backends_title, Some(tx.backends_subtitle), true))
            .child(div().class("backends-grid").children(tx.backends.iter().enumerate().map(|(i, b)| {
                reveal("scale-in", "", i as u32 * 90).child(tilt_card(
                    "backend-card",
                    vec![
                        div().class(format!("feature-icon fi-{}", i % 4)).child(icon(BACKEND_ICONS[i])).into(),
                        h3().text(b.title).into(),
                        code().class("backend-code").text(BACKEND_HINTS[i]).into(),
                        p().text(b.desc).into(),
                    ],
                ))
            })))
            .into(),
        cta_banner(lang).into(),
    ])
}

fn performance(ctx: &Ctx<'_>) -> Node {
    let lang = ctx.route.lang;
    let tx = t(lang);
    let rust = highlight(
        "User::query()\n    .filter(User::AGE.ge(18) & User::NAME.contains(\"li\"))\n    .order_by(User::NAME.asc())\n    .limit(20)",
        "rust",
    );
    fragment([
        Node::from(page_seo(ctx, tx.perf_page_title, tx.perf_page_subtitle)),
        page_hero(tx.perf_eyebrow, tx.perf_page_title, tx.perf_page_subtitle).into(),
        section()
            .class("section container")
            .child(section_head(tx.perf_eyebrow, tx.perf_why_title, None, true))
            .child(div().class("why-grid").children(tx.perf_why.iter().enumerate().map(|(i, w)| {
                reveal("fade-up", "", i as u32 * 90).child(tilt_card(
                    "why-card",
                    vec![
                        span().class("why-num").text(format!("{:02}", i + 1)).into(),
                        h3().text(w.title).into(),
                        p().text(w.desc).into(),
                    ],
                ))
            })))
            .into(),
        section()
            .class("section container")
            .child(
                reveal("fade-up", "rust-to-sql", 0)
                    .child(mini_window("rust", rust, false))
                    .child(div().class("rts-arrow").attr("aria-hidden", "true").child(icon(Icon::ArrowRight)))
                    .child(mini_window("PostgreSQL", highlight(samples::SQL_EXAMPLE, "sql"), true)),
            )
            .into(),
        section()
            .class("section container")
            .child(section_head("Criterion", tx.perf_results_title, None, true))
            .child(reveal("scale-in", "perf-card glass spotlight wide", 0).child(bench_bars(lang, None)))
            .into(),
        section()
            .class("section container narrow")
            .child(
                reveal("fade-up", "method glass", 0)
                    .child(h2().text(tx.perf_method_title))
                    .child(p().text(tx.perf_method_text))
                    .child(
                        div()
                            .class("method-links")
                            .child(
                                a().class("btn btn-ghost magnetic")
                                    .href(doc_href(lang, "performance"))
                                    .child(icon(Icon::Book))
                                    .child(span().text(tx.perf_read_more)),
                            )
                            .child(
                                a().class("btn btn-ghost magnetic")
                                    .href(doc_href(lang, "benchmarks"))
                                    .child(icon(Icon::Activity))
                                    .child(span().text(docs::title("benchmarks", lang))),
                            ),
                    ),
            )
            .into(),
    ])
}

fn playground(ctx: &Ctx<'_>) -> Node {
    let lang = ctx.route.lang;
    let tx = t(lang);
    fragment([
        Node::from(page_seo(ctx, tx.pg_title, tx.pg_subtitle)),
        page_hero("WebAssembly", tx.pg_title, tx.pg_subtitle).into(),
        island("playground", lang.code(), crate::playground::playground),
    ])
}

// ---------------------------------------------------------------------------
// Documentation
// ---------------------------------------------------------------------------

fn doc_page(ctx: &Ctx<'_>, slug: &str) -> Node {
    let lang = ctx.route.lang;
    let tx = t(lang);
    let idx = lang.idx();
    let (Some((category, page)), Some(doc)) = (docs::find(slug), ctx.site.docs.get(lang, slug)) else {
        return not_found(lang);
    };
    let title = page.title[idx];
    let docs_home = href(lang, Page::Docs);
    let seo = page_seo(ctx, title, &doc.summary)
        .og_type("article")
        .json_ld(format!(
            r#"{{"@context":"https://schema.org","@type":"TechArticle","headline":{},"description":{},"inLanguage":"{}","url":{}}}"#,
            json(title),
            json(&doc.summary),
            lang.code(),
            json(&format!("{}{}", ctx.origin, Route::new(lang, Page::Doc(page.slug.into())).path())),
        ))
        .json_ld(format!(
            r#"{{"@context":"https://schema.org","@type":"BreadcrumbList","itemListElement":[{{"@type":"ListItem","position":1,"name":{},"item":"{o}/{l}/docs/"}},{{"@type":"ListItem","position":2,"name":{},"item":"{o}/{l}/docs/{s}/"}}]}}"#,
            json(tx.nav_docs),
            json(title),
            o = ctx.origin,
            l = lang.code(),
            s = page.slug,
        ));
    let (prev, next) = docs::neighbours(page.slug);
    let chevron = || span().class("sep").child(icon(Icon::ChevronRight));
    let pager = nav()
        .class("doc-pager")
        .attr("aria-label", "Pagination")
        .child(match prev {
            Some(p) => Node::from(
                a().class("pager prev spotlight")
                    .href(doc_href(lang, p.slug))
                    .attr("rel", "prev")
                    .child(span().class("pager-label").child(icon(Icon::ArrowLeft)).text(tx.docs_prev))
                    .child(span().class("pager-title").text(p.title[idx])),
            ),
            None => span().into(),
        })
        .child(next.map(|n| {
            a().class("pager next spotlight")
                .href(doc_href(lang, n.slug))
                .attr("rel", "next")
                .child(span().class("pager-label").text(tx.docs_next).child(icon(Icon::ArrowRight)))
                .child(span().class("pager-title").text(n.title[idx]))
        }));
    let article = article()
        .class("docs-article")
        .child(
            nav()
                .class("breadcrumb")
                .attr("aria-label", "Breadcrumb")
                .child(a().href(docs_home).text(tx.nav_docs))
                .child(chevron())
                .child(span().text(category.title[idx]))
                .child(chevron())
                .child(span().class("current").attr("aria-current", "page").text(title)),
        )
        .child(div().class("doc-meta").child(span().child(icon(Icon::Clock)).text(format!(
            "{} {}",
            docs::reading_minutes(doc.words),
            tx.docs_min_read
        ))))
        .child(div().class("markdown-body").html(doc.html.clone()))
        .child(pager);
    let toc = (!doc.toc.is_empty()).then(|| {
        div().class("toc").child(p().class("toc-title").text(tx.docs_on_this_page)).child(ul().children(
            doc.toc.iter().map(|entry| {
                li().class(format!("toc-l{}", entry.level)).child(
                    a().href(format!("#{}", entry.id)).attr("data-toc", entry.id.clone()).text(entry.text.clone()),
                )
            }),
        ))
    });
    div()
        .class("docs container-wide")
        .child(seo)
        .child(island("docs-sidebar", format!("{}:{}", lang.code(), page.slug), crate::islands::docs_sidebar))
        .child(div().class("docs-main").child(article))
        .child(aside().class("docs-toc").child(toc))
        .into()
}

// ---------------------------------------------------------------------------
// Not found, offline, language choice
// ---------------------------------------------------------------------------

fn not_found(lang: Lang) -> Node {
    let tx = t(lang);
    section()
        .class("not-found container")
        .child(Seo::new().title(tx.nf_title).description(tx.nf_text).noindex(true))
        .child(div().class("nf-code glitch").attr("data-text", "404").attr("aria-hidden", "true").text("404"))
        .child(h1().class("hero-anim").text(tx.nf_title))
        .child(p().class("hero-anim").style("--d:100ms").text(tx.nf_text))
        .child(
            div()
                .class("hero-ctas hero-anim")
                .style("--d:200ms")
                .child(
                    a().class("btn btn-primary magnetic")
                        .href(href(lang, Page::Home))
                        .child(icon(Icon::ArrowLeft))
                        .child(span().text(tx.nf_home)),
                )
                .child(
                    a().class("btn btn-ghost magnetic")
                        .href(href(lang, Page::Docs))
                        .child(icon(Icon::Book))
                        .child(span().text(tx.nf_docs)),
                ),
        )
        .into()
}

/// The page the service worker shows for a page never opened, when the network is down:
/// the same message in the three languages, with links to what works offline.
pub fn offline(site: &Site, origin: &str) -> String {
    let route = Route::new(Lang::En, Page::NotFound);
    let content = section()
        .class("not-found offline container")
        .child(Seo::new().title("Offline · Hors ligne · Sin conexión").noindex(true))
        .child(div().class("nf-code").attr("aria-hidden", "true").child(logo_mark()))
        .children(Lang::ALL.map(|lang| {
            let tx = t(lang);
            div()
                .class("offline-lang hero-anim")
                .attr("lang", lang.code())
                .style(format!("--d:{}ms", lang.idx() * 100))
                .child(h1().text(tx.offline_title))
                .child(p().text(tx.offline_text))
                .child(
                    div()
                        .class("hero-ctas")
                        .child(
                            a().class("btn btn-primary magnetic")
                                .href(href(lang, Page::Playground))
                                .child(icon(Icon::Play))
                                .child(span().text(tx.nav_playground)),
                        )
                        .child(
                            a().class("btn btn-ghost magnetic")
                                .href(href(lang, Page::Home))
                                .child(span().text(tx.nav_home)),
                        ),
                )
        }));
    let ctx = Ctx { site, route: &route, origin, theme: None };
    document(&ctx, content.into())
}

/// The page of a static export where a server would redirect to the visitor's language
/// (`/` and the addresses of the previous site, like `/docs/models`): the browser bundle
/// sends the visitor to the page in their language at once; without JavaScript the page
/// opens in English, and the three languages are linked.
pub fn language_choice(site: &Site, origin: &str, page: &Page) -> String {
    let target = page.path();
    let english = Route::new(Lang::En, page.clone());
    let seo = Seo::new()
        .site_name("Memoire ORM")
        .title("Memoire ORM")
        .description(t(Lang::En).meta_description)
        .base_url(origin)
        .canonical(english.path())
        .noindex(true);
    let body = main()
        .class("main")
        .child(
            div()
                .class("bg")
                .attr("aria-hidden", "true")
                .child(div().class("bg-mesh"))
                .child(div().class("bg-grid"))
                .child(div().class("bg-vignette")),
        )
        .child(
            section()
                .class("not-found container")
                .child(div().class("nf-code").attr("aria-hidden", "true").child(logo_mark()))
                .child(h1().text("Memoire ORM"))
                .child(p().text(Lang::ALL.map(|l| t(l).choose_language).join(" · ")))
                .child(div().class("hero-ctas").children(Lang::ALL.map(|l| {
                    a().class("btn btn-ghost magnetic")
                        .href(link(&format!("/{}{target}", l.code())))
                        .attr("hreflang", l.code())
                        .attr("lang", l.code())
                        .text(l.native_name())
                }))),
        );
    base_document(site, body.into(), Lang::En, None)
        .seo(seo)
        .html_attr("data-redirect", target.clone())
        .head(language_redirect(&target))
        .render()
}

/// A JavaScript string literal that cannot close a `<script>` element.
fn js_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '<' => out.push_str("\\u003c"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The script of the pages a static host serves without knowing the visitor's language: goes
/// to `/{lang}{target}` for the language of the address (`/fr/…` on a missing page), else the
/// one saved by the switcher, else the browser's, else English. It runs before the browser
/// bundle (which does the same); without JavaScript, a `<meta>` refresh goes to English.
fn language_redirect(target: &str) -> String {
    let codes: Vec<String> = Lang::ALL.iter().map(|l| js_string(l.code())).collect();
    format!(
        concat!(
            "<script>(function(){{var b={base},L=[{codes}],c=null;",
            "var s=location.pathname.slice(b.length).split('/').filter(Boolean)[0];",
            "if(L.indexOf(s)>=0)c=s;",
            "var m=document.cookie.match(/(?:^|; )memoire-lang=([a-z]{{2}})/);",
            "if(!c&&m&&L.indexOf(m[1])>=0)c=m[1];",
            "try{{var k=localStorage.getItem('memoire-lang');if(!c&&k&&L.indexOf(k)>=0)c=k}}catch(e){{}}",
            "var l=navigator.languages||[navigator.language||''];",
            "for(var i=0;i<l.length&&!c;i++){{var x=String(l[i]).slice(0,2).toLowerCase();",
            "if(L.indexOf(x)>=0)c=x}}",
            "location.replace(b+'/'+(c||'en')+{target}+location.search+location.hash)}})()</script>",
            r#"<noscript><meta http-equiv="refresh" content="0; url={fallback}"/></noscript>"#,
        ),
        base = js_string(base()),
        codes = codes.join(","),
        target = js_string(target),
        fallback = link(&format!("/en{target}")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::layout::copy_icons;

    #[test]
    fn copy_pill_and_icons() {
        let html = Node::from(copy_pill(Lang::Fr, "cargo add memoire-orm")).render();
        assert!(html.contains(r#"data-copy="cargo add memoire-orm""#));
        assert!(html.contains("Copié"));
        let icons = Node::from(copy_icons()).render();
        assert!(icons.contains("when-idle") && icons.contains("when-copied"));
    }
}
