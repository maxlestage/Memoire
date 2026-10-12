//! What every page shares: the document (`<head>` with SEO, PWA and the browser bundle),
//! the animated background, the navbar, the footer and the small reusable blocks.

use active::html::svg;
use active::prelude::*;

use super::Site;
use super::pwa;
use crate::docs;
use crate::i18n::t;
use crate::icons::{Icon, icon};
use crate::routes::{Lang, Page, Route, base, link};

/// What a request needs to render a page.
pub struct Ctx<'a> {
    pub site: &'a Site,
    pub route: &'a Route,
    /// Absolute origin for canonical and Open Graph URLs (`https://example.com`, with the
    /// base path).
    pub origin: &'a str,
    /// The theme chosen by the visitor (cookie), `None` to follow the system.
    pub theme: Option<&'static str>,
}

pub const CRATES_IO: &str = "https://github.com/maxlestage/Memoire";
pub const DOCS_RS: &str = "https://docs.rs/memoire-orm";
/// The license, served by the site itself (a copy of the repository's LICENSE): proprietary,
/// all rights reserved.
pub const LICENSE_PATH: &str = "/static/LICENSE.txt";
/// Who designed and built Memoire and its sister projects.
pub const AUTHOR: &str = "Maxime Nathan Lestage";
/// The sister projects of Memoire, by the same author: (name, website). The footer links
/// them in this order; `i18n::Texts::footer_projects` describes each one in the same order.
pub const ECOSYSTEM: [(&str, &str); 7] = [
    ("Active", "https://maxlestage.github.io/Active/"),
    ("Vitesse", "https://maxlestage.github.io/Vitesse/"),
    ("Securite", "https://maxlestage.github.io/Securite/"),
    ("Memoire", "https://maxlestage.github.io/Memoire/"),
    ("Paquets", "https://maxlestage.github.io/Paquets/"),
    ("FerroSwift", "https://maxlestage.github.io/FerroSwift/"),
    ("Solidify", "https://maxlestage.github.io/Solidify/"),
];

/// A link to a page of `lang`.
pub fn href(lang: Lang, page: Page) -> String {
    Route::new(lang, page).href()
}

pub fn doc_href(lang: Lang, slug: &str) -> String {
    href(lang, Page::Doc(slug.to_string()))
}

/// `<a target="_blank">` to another site.
pub fn external(url: impl Into<String>) -> Element {
    a().href(url.into()).attr("target", "_blank").attr("rel", "noopener noreferrer")
}

/// The Memoire mark: a memory chip with a stylised "M".
pub fn logo_mark() -> Element {
    let gradient = |id: &'static str, stops: &[(&'static str, &'static str)]| {
        stops.iter().fold(
            linear_gradient().id(id).attr("x1", "0").attr("y1", "0").attr("x2", "1").attr("y2", "1"),
            |g, (offset, color)| g.child(stop().attr("offset", *offset).attr("stop-color", *color)),
        )
    };
    svg()
        .class("logo-mark")
        .attr("viewBox", "0 0 64 64")
        .attr("width", "34")
        .attr("height", "34")
        .attr("aria-hidden", "true")
        .child(
            defs()
                .child(gradient("mlg-a", &[("0", "#f74c00"), (".55", "#ffb347"), ("1", "#7c5cff")]))
                .child(gradient("mlg-b", &[("0", "#7c5cff"), ("1", "#22d3ee")])),
        )
        .child(
            g().class("logo-pins")
                .attr("stroke", "url(#mlg-b)")
                .attr("stroke-width", "3")
                .attr("stroke-linecap", "round")
                .child(path().attr(
                    "d",
                    "M21 4v6M32 4v6M43 4v6M21 54v6M32 54v6M43 54v6M4 21h6M4 32h6M4 43h6M54 21h6M54 32h6M54 43h6",
                )),
        )
        .child(
            rect()
                .attr("x", "10")
                .attr("y", "10")
                .attr("width", "44")
                .attr("height", "44")
                .attr("rx", "12")
                .attr("fill", "#0d0d18")
                .attr("stroke", "url(#mlg-a)")
                .attr("stroke-width", "3"),
        )
        .child(
            path()
                .class("logo-m")
                .attr("d", "M20.5 42V22.5L32 34l11.5-11.5V42")
                .attr("fill", "none")
                .attr("stroke", "url(#mlg-a)")
                .attr("stroke-width", "4.6")
                .attr("stroke-linecap", "round")
                .attr("stroke-linejoin", "round"),
        )
}

/// The logo, linking to the home page.
pub fn logo(lang: Lang) -> Element {
    a().class("logo")
        .href(href(lang, Page::Home))
        .attr("aria-label", "Memoire ORM")
        .child(logo_mark())
        .child(span().class("logo-text").text("Memoire").child(span().class("logo-accent").text(" ORM")))
}

/// FR / EN / ES: links to the same page in each language, with a sliding indicator.
pub fn lang_switch(route: &Route) -> Element {
    div()
        .class("lang-switch")
        .attr("role", "group")
        .attr("aria-label", t(route.lang).language)
        .style(format!("--idx:{}", route.lang.idx()))
        .child(span().class("lang-indicator").attr("aria-hidden", "true"))
        .children(Lang::ALL.map(|l| {
            // From a missing page: the home page of the language.
            let target = if route.page == Page::NotFound { Route::new(l, Page::Home) } else { route.with_lang(l) };
            let link = a()
                .class("lang-btn")
                .href(target.href())
                .attr("hreflang", l.code())
                .attr("lang", l.code())
                .attr("title", l.native_name())
                .text(l.short());
            if l == route.lang { link.class("active").attr("aria-current", "true") } else { link }
        }))
}

/// Sun / moon button; the browser bundle switches the theme.
pub fn theme_toggle(lang: Lang) -> Element {
    let label = t(lang).theme_toggle;
    button()
        .attr("type", "button")
        .class("theme-toggle")
        .attr("aria-label", label)
        .attr("title", label)
        .child(span().class("tt-icon tt-sun").child(icon(Icon::Sun)))
        .child(span().class("tt-icon tt-moon").child(icon(Icon::Moon)))
}

/// `cargo add memoire-orm --git https://github.com/maxlestage/Memoire`: click to copy, with a "Copied!" tooltip.
pub fn copy_pill(lang: Lang, text: &'static str) -> Element {
    let tx = t(lang);
    button()
        .attr("type", "button")
        .class("copy-pill magnetic")
        .attr("aria-label", tx.copy_install)
        .attr("title", tx.copy_install)
        .attr("data-copy", text)
        .attr("data-copied", tx.copied)
        .child(span().class("copy-prompt").attr("aria-hidden", "true").text("$"))
        .child(command(text))
        .child(copy_icons().class("copy-icon"))
        .child(span().class("copy-tip").attr("role", "status"))
}

/// A shell command in `<code>`, whose URL may break after its slashes on a narrow screen:
/// `<wbr>` adds nothing to what is selected and copied.
fn command(text: &str) -> Element {
    let mut code = code();
    let mut part = String::new();
    let mut chars = text.chars().peekable();
    let mut previous = ' ';
    while let Some(c) = chars.next() {
        part.push(c);
        let next = chars.peek().copied();
        if c == '/' && previous != '/' && next.is_some_and(|n| n != '/') {
            code = code.child(Node::from(std::mem::take(&mut part))).child(wbr());
        }
        previous = c;
    }
    code.child(Node::from(part))
}

/// The copy icon, and the check mark shown once copied (`.is-copied`).
pub fn copy_icons() -> Element {
    span().child(icon(Icon::Copy).class("when-idle")).child(icon(Icon::Check).class("when-copied"))
}

/// A block revealed when it scrolls into view (`anim`: `fade-up`, `scale-in`,
/// `slide-left`, `slide-right`), `delay` in milliseconds.
pub fn reveal(anim: &str, class: &str, delay: u32) -> Element {
    let el = div().class(format!("reveal {anim} {class}").trim_end().to_string());
    if delay > 0 { el.style(format!("--d:{delay}ms")) } else { el }
}

/// Eyebrow, gradient title and subtitle.
pub fn section_head(eyebrow: &str, title: &str, subtitle: Option<&str>, center: bool) -> Element {
    reveal("fade-up", if center { "section-head center" } else { "section-head" }, 0)
        .child(span().class("eyebrow").text(eyebrow.to_string()))
        .child(h2().class("section-title").text(title.to_string()))
        .child(subtitle.map(|s| p().class("section-subtitle").text(s.to_string())))
}

/// Glass card with 3D tilt and a glow following the pointer.
pub fn tilt_card(class: &str, content: Vec<Node>) -> Element {
    div().class(format!("tilt spotlight card {class}")).child(div().class("tilt-inner").children(content))
}

/// The large heading block of the inner pages.
pub fn page_hero(eyebrow: &str, title: &str, subtitle: &str) -> Element {
    section()
        .class("page-hero container")
        .child(span().class("eyebrow hero-anim").text(eyebrow.to_string()))
        .child(h1().class("page-title hero-anim shimmer").style("--d:80ms").text(title.to_string()))
        .child(p().class("page-subtitle hero-anim").style("--d:160ms").text(subtitle.to_string()))
}

/// A small code window (`lang` label, highlighted code).
pub fn mini_window(label: &str, code_html: String, wrap: bool) -> Element {
    div()
        .class("mini-window glass")
        .child(div().class("mini-head").child(dots()).child(span().class("code-lang").text(label.to_string())))
        .child(if wrap { pre().class("wrap") } else { pre() }.child(code().html(code_html)))
}

pub fn dots() -> Element {
    span().class("cw-dots").attr("aria-hidden", "true").child(i()).child(i()).child(i())
}

/// Fixed animated backdrop: gradient mesh, floating blurred blobs and a subtle grid.
fn background() -> Element {
    div()
        .class("bg")
        .attr("aria-hidden", "true")
        .child(div().class("bg-mesh"))
        .children(["b1", "b2", "b3", "b4"].map(|b| div().class(format!("blob-wrap {b}")).child(div().class("blob"))))
        .child(div().class("bg-grid"))
        .child(div().class("bg-vignette"))
}

fn navbar(route: &Route) -> Element {
    let lang = route.lang;
    let tx = t(lang);
    let is_docs = matches!(route.page, Page::Docs | Page::Doc(_));
    let links = [
        (Page::Features, tx.nav_features, route.page == Page::Features),
        (Page::Performance, tx.nav_performance, route.page == Page::Performance),
        (Page::Playground, tx.nav_playground, route.page == Page::Playground),
        (Page::Docs, tx.nav_docs, is_docs),
    ];
    let start = doc_href(lang, "quick-start");
    header()
        .class("navbar")
        .child(
            div()
                .class("nav-inner container")
                .child(logo(lang))
                .child(nav().class("nav-links").attr("aria-label", "Main").children(links.clone().map(
                    |(page, label, active)| {
                        let a = a().class("nav-link").href(href(lang, page)).text(label);
                        if active { a.class("active").attr("aria-current", "page") } else { a }
                    },
                )))
                .child(
                    div()
                        .class("nav-actions")
                        .child(lang_switch(route))
                        .child(theme_toggle(lang))
                        .child(
                            a().class("btn btn-primary btn-sm magnetic nav-cta")
                                .href(start.clone())
                                .child(span().text(tx.nav_get_started)),
                        )
                        .child(
                            button()
                                .attr("type", "button")
                                .class("burger")
                                .attr("aria-label", tx.nav_open_menu)
                                .attr("aria-expanded", "false")
                                .attr("aria-controls", "mobile-menu")
                                .attr("data-open-label", tx.nav_open_menu)
                                .attr("data-close-label", tx.nav_close_menu)
                                .child(span())
                                .child(span())
                                .child(span()),
                        ),
                ),
        )
        .child(div().class("scroll-progress").attr("aria-hidden", "true"))
        .child(
            div()
                .id("mobile-menu")
                .class("mobile-menu")
                .attr("inert", "")
                .child(
                    nav()
                        .class("mobile-links")
                        .attr("aria-label", "Mobile")
                        .child({
                            let a = a().class("mobile-link").href(href(lang, Page::Home));
                            let a = if route.page == Page::Home { a.class("active") } else { a };
                            a.child(span().class("ml-index").text("01")).text(tx.nav_home)
                        })
                        .children(links.into_iter().enumerate().map(|(i, (page, label, active))| {
                            let a = a().class("mobile-link").href(href(lang, page));
                            let a = if active { a.class("active") } else { a };
                            a.child(span().class("ml-index").text(format!("{:02}", i + 2))).text(label)
                        })),
                )
                .child(div().class("mobile-extras").child(lang_switch(route)).child(theme_toggle(lang)))
                .child(
                    a().class("btn btn-primary mobile-cta")
                        .href(start)
                        .child(span().text(tx.nav_get_started))
                        .child(icon(Icon::ArrowRight)),
                ),
        )
}

/// The signature footer, shared in spirit by every site of the ecosystem: the call to
/// action and the install command, the link columns, the sister projects, the author's
/// signature, the giant wordmark and the legal line.
fn page_footer(route: &Route) -> Element {
    let lang = route.lang;
    let tx = t(lang);
    let doc = |slug: &str, label: &'static str| li().child(a().href(doc_href(lang, slug)).text(label));
    let page = |page: Page, label: &'static str| li().child(a().href(href(lang, page)).text(label));
    let ext = |url: String, label: &'static str| {
        li().child(external(url).text(label).child(span().class("ext").child(icon(Icon::External))))
    };
    let social = |url: &'static str, label: &'static str, i: Icon| {
        external(url).class("icon-btn").attr("aria-label", label).child(icon(i))
    };
    let cta = div()
        .class("container footer-cta")
        .child(
            a().class("footer-cta-link")
                .href(doc_href(lang, "quick-start"))
                .child(span().text(tx.footer_start))
                .child(Node::from(" "))
                .child(em().text("Memoire"))
                .child(icon(Icon::ArrowRight)),
        )
        .child(
            el("code")
                .class("footer-install")
                .attr(
                    "aria-label",
                    format!("{}: cargo add memoire-orm --git https://github.com/maxlestage/Memoire", tx.footer_install),
                )
                .attr("title", tx.footer_install)
                .text("cargo add memoire-orm --git https://github.com/maxlestage/Memoire"),
        );
    let brand = div()
        .class("footer-brand")
        .child(logo(lang))
        .child(p().class("footer-tagline").text(tx.footer_tagline))
        .child(p().class("footer-mission").text(tx.footer_mission))
        .child(
            ul().class("footer-badges")
                .attr("aria-label", "Memoire ORM")
                .child(li().class("hot").text(format!("v{}", memoire_orm::VERSION)))
                .child(li().text(tx.footer_rust))
                .child(li().child(external(CRATES_IO).text("GitHub"))),
        )
        .child(div().class("footer-social").child(social(CRATES_IO, "GitHub", Icon::Github)).child(social(
            DOCS_RS,
            "docs.rs",
            Icon::Book,
        )));
    let ecosystem = nav()
        .class("container footer-eco")
        .attr("aria-label", tx.footer_ecosystem)
        .child(
            div()
                .class("footer-eco-head")
                .child(h3().text(tx.footer_ecosystem))
                .child(p().text(tx.footer_ecosystem_text)),
        )
        .child(ul().class("eco-grid").children(ECOSYSTEM.iter().zip(tx.footer_projects).map(|((name, url), text)| {
            let here = *name == "Memoire";
            let mut title = span().class("eco-name").text(*name);
            if here {
                title = title.child(span().class("eco-here").text(tx.footer_here));
            }
            let link = if here { a().href(*url).attr("aria-current", "page") } else { external(*url) };
            li().child(
                link.class(if here { "eco current" } else { "eco" })
                    .child(title)
                    .child(span().class("eco-text").text(text))
                    .child(icon(Icon::ArrowRight)),
            )
        })));
    let credits = div()
        .class("container footer-credits")
        .child(p().class("credits-by").text(tx.footer_by))
        .child(p().class("credits-name").children(
            AUTHOR.split(' ').enumerate().map(|(i, word)| span().style(format!("--i:{i}")).text(word.to_string())),
        ))
        .child(p().class("credits-note").text(tx.footer_crafted));
    let wordmark = div()
        .class("wordmark")
        .attr("aria-hidden", "true")
        .children("Memoire".chars().enumerate().map(|(i, c)| span().style(format!("--i:{i}")).text(c.to_string())));
    footer()
        .class("footer")
        .child(div().class("footer-border").attr("aria-hidden", "true"))
        .child(cta)
        .child(
            div()
                .class("container footer-grid")
                .child(brand)
                .child(
                    div().class("footer-col").child(h3().text(tx.footer_product)).child(
                        ul().child(page(Page::Features, tx.nav_features))
                            .child(page(Page::Performance, tx.nav_performance))
                            .child(page(Page::Playground, tx.nav_playground))
                            .child(page(Page::Docs, tx.nav_docs)),
                    ),
                )
                .child(
                    div().class("footer-col").child(h3().text(tx.footer_docs)).child(
                        ul().child(doc("quick-start", docs::title("quick-start", lang)))
                            .child(doc("models", docs::title("models", lang)))
                            .child(doc("queries", docs::title("queries", lang)))
                            .child(doc("deploy-heroku-mobile", tx.footer_deploy)),
                    ),
                )
                .child(
                    div().class("footer-col").child(h3().text(tx.footer_community)).child(
                        ul().child(ext(CRATES_IO.into(), "GitHub"))
                            .child(ext(DOCS_RS.into(), "docs.rs"))
                            .child(ext(link(LICENSE_PATH), tx.footer_license)),
                    ),
                )
                .child(
                    div()
                        .class("footer-col footer-lang")
                        .child(h3().class("with-icon").child(icon(Icon::Globe)).text(tx.language))
                        .child(lang_switch(route)),
                ),
        )
        .child(ecosystem)
        .child(credits)
        .child(wordmark)
        .child(
            div()
                .class("container footer-bottom")
                .child(span().text(tx.footer_rights))
                .child(external(link(LICENSE_PATH)).text(tx.footer_proprietary))
                .child(span().class("footer-built").text(tx.footer_built))
                .child(
                    button()
                        .attr("type", "button")
                        .class("footer-top")
                        .attr("data-to-top", "")
                        .text(tx.back_to_top)
                        .child(icon(Icon::ArrowUp)),
                ),
        )
}

/// Floating "back to top" button with a circular scroll-progress ring.
fn back_to_top(lang: Lang) -> Element {
    let label = t(lang).back_to_top;
    let ring = svg()
        .class("to-top-ring")
        .attr("viewBox", "0 0 48 48")
        .attr("aria-hidden", "true")
        .child(circle().class("ring-bg").attr("cx", "24").attr("cy", "24").attr("r", "21"))
        .child(circle().class("ring-fg").attr("cx", "24").attr("cy", "24").attr("r", "21").attr("pathLength", "100"));
    button()
        .attr("type", "button")
        .class("to-top magnetic")
        .attr("aria-label", label)
        .attr("title", label)
        .attr("data-to-top", "")
        .child(ring)
        .child(icon(Icon::ArrowUp))
}

/// `hreflang` alternates of a page in every language.
pub fn alternates(seo: Seo, route: &Route) -> Seo {
    let seo = Lang::ALL.into_iter().fold(seo, |seo, l| seo.alternate(l.code(), route.with_lang(l).path()));
    seo.alternate("x-default", route.with_lang(Lang::En).path())
}

/// A JSON string literal (for JSON-LD).
pub fn json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The `<head>` and `<html>` attributes every document shares: stylesheet, icon, fonts,
/// theme, base path, service worker and the browser bundle.
pub fn base_document(site: &Site, body: Node, lang: Lang, theme: Option<&'static str>) -> Document {
    let assets = &site.assets;
    let mut doc = Document::new(body)
        .lang(lang.code())
        .stylesheet(assets.url("/static/app.css"))
        .head(format!(
            concat!(
                r#"<meta name="color-scheme" content="dark light"/>"#,
                r#"<meta name="author" content="{author}"/>"#,
                r#"<link rel="icon" href="{base}/static/favicon.svg" type="image/svg+xml"/>"#,
                // Same URL as in app.css, so that the preload is used.
                r#"<link rel="preload" href="{base}/static/fonts/inter-latin.woff2" as="font" type="font/woff2" crossorigin=""/>"#,
            ),
            base = base(),
            author = AUTHOR
        ))
        .service_worker(&link("/sw.js"));
    if assets.get("/pkg/memoire_site.js").is_some() {
        doc = doc.client(assets.url("/pkg/memoire_site.js"), assets.url("/pkg/memoire_site_bg.wasm"));
    }
    if let Some(theme) = theme {
        doc = doc.html_attr("data-theme", theme);
    }
    if !base().is_empty() {
        // Read by the browser bundle to build its URLs.
        doc = doc.html_attr("data-base", base());
    }
    doc
}

/// A complete page around `content`.
pub fn document(ctx: &Ctx<'_>, content: Node) -> String {
    let route = ctx.route;
    let lang = route.lang;
    let tx = t(lang);
    let body = fragment([
        Node::from(a().class("skip-link").href("#main").text(tx.skip_to_content)),
        background().into(),
        div().class("cursor-ring").attr("aria-hidden", "true").into(),
        div().class("cursor-dot").attr("aria-hidden", "true").into(),
        navbar(route).into(),
        main().id("main").class("main").attr("tabindex", "-1").child(div().class("page").child(content)).into(),
        page_footer(route).into(),
        back_to_top(lang).into(),
    ]);
    let site = Seo::new()
        .site_name("Memoire ORM")
        .title_template("%s · Memoire ORM")
        .description(tx.meta_description)
        .base_url(ctx.origin)
        .image("/static/og-image.png")
        .locale(lang.locale())
        .meta_property("og:image:width", "1200")
        .meta_property("og:image:height", "630");
    let manifest = pwa::manifest(lang);
    let head_manifest =
        if ctx.theme == Some("light") { manifest.clone().theme_color(pwa::THEME_LIGHT) } else { manifest };
    let other_locales: String = Lang::ALL
        .into_iter()
        .filter(|l| *l != lang)
        .map(|l| format!(r#"<meta property="og:locale:alternate" content="{}"/>"#, l.locale()))
        .collect();
    base_document(ctx.site, body, lang, ctx.theme)
        .seo(site)
        .head(other_locales)
        .manifest(&pwa::manifest_url(lang), &head_manifest)
        .render()
}
