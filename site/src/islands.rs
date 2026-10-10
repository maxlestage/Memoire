//! The interactive parts of the site. Each island renders on the server like any view and,
//! compiled to WebAssembly, hydrates in the browser from the same code. What only the
//! browser can do (timers, observers, downloads) lives in `client`, called from mount
//! callbacks and event handlers, which never run on the server.

use active::prelude::*;

use crate::data::samples::{self, TermLine};
use crate::docs::{self, CATEGORIES};
use crate::highlight::highlight;
use crate::i18n::t;
use crate::icons::{Icon, icon};
use crate::routes::{Lang, link};
use crate::web;

/// Every island, registered under the name used by [`island`].
pub const ISLANDS: [(&str, active::IslandFn); 4] = [
    ("playground", crate::playground::playground),
    ("showcase", showcase),
    ("typing", typing),
    ("docs-sidebar", docs_sidebar),
];

fn lang(props: &str) -> Lang {
    Lang::parse(props.split(':').next().unwrap_or_default()).unwrap_or(Lang::En)
}

/// The hero's rotating tagline: types, pauses, deletes, moves to the next line. The server
/// (and a browser without JavaScript) shows the first line.
pub fn typing(props: &str) -> Node {
    let lines = t(lang(props)).hero_taglines;
    let text = use_state(lines.first().copied().unwrap_or_default().to_string());
    span()
        .class("typing")
        .on_mount(move |_| {
            #[cfg(target_arch = "wasm32")]
            crate::client::typing::start(text, lines);
        })
        .child(span().class("sr-only").text(lines.join(" ")))
        .child(span().class("typing-text").attr("aria-hidden", "true").text_dyn(move || text.get()))
        .child(span().class("typing-caret").attr("aria-hidden", "true"))
        .into()
}

const FILES: [&str; 4] = ["models/user.rs", "src/queries.rs", "src/transaction.rs", "src/migrate.rs"];
const CODES: [&str; 4] = [samples::MODEL, samples::QUERY, samples::TRANSACTION, samples::MIGRATION];

/// The home page's code window (Model / Query / Transaction / Migration tabs) and the
/// terminal that types the matching output when it scrolls into view.
pub fn showcase(props: &str) -> Node {
    let tx = t(lang(props));
    let tab = use_state(0usize);
    let copied = use_state(false);
    // (lines completed, characters typed in the current line): fully typed at first.
    let progress = use_state((samples::TERMINAL[0].len(), 0usize));
    let timer = web::Timer::default();

    let tabs =
        div().class("cw-tabs").attr("role", "tablist").children(tx.code_tabs.iter().enumerate().map(|(i, name)| {
            let timer = timer.clone();
            button()
                .attr("type", "button")
                .attr("role", "tab")
                .class("cw-tab")
                .class_if("active", move || tab.get() == i)
                .attr_dyn("aria-selected", move || (tab.get() == i).to_string())
                .on_click(move |_| {
                    if tab.get() == i {
                        return;
                    }
                    tab.set(i);
                    if web::reduced_motion() {
                        progress.set((samples::TERMINAL[i].len(), 0));
                    } else {
                        progress.set((0, 0));
                        #[cfg(target_arch = "wasm32")]
                        crate::client::showcase::type_script(progress, samples::TERMINAL[i], &timer);
                    }
                    let _ = &timer;
                })
                .text(*name)
        }));

    let window = div()
        .class("code-window")
        .child(
            div().class("cw-head").child(dots()).child(tabs).child(
                button()
                    .attr("type", "button")
                    .class("cw-copy")
                    .class_if("is-copied", move || copied.get())
                    .attr("aria-label", tx.copy)
                    .on_click(move |_| {
                        web::copy_text(CODES[tab.get()]);
                        copied.set(true);
                        web::after(1600, move || copied.set(false));
                    })
                    .children_dyn(move || vec![icon(if copied.get() { Icon::Check } else { Icon::Copy }).into()]),
            ),
        )
        .child(div().class("cw-file").text_dyn(move || FILES[tab.get()].to_string()))
        // Every tab is in the page; showing one replays its entrance animation.
        .children((0..CODES.len()).map(|k| {
            let lines = CODES[k].lines().count();
            div()
                .class("cw-body")
                .bool_attr("hidden", move || tab.get() != k)
                .child(
                    pre()
                        .class("cw-gutter")
                        .attr("aria-hidden", "true")
                        .text((1..=lines).map(|n| format!("{n}\n")).collect::<String>()),
                )
                .child(pre().class("cw-code").child(code().html(highlight(CODES[k], "rust"))))
        }));

    let terminal = div()
        .class("terminal")
        .attr("aria-label", tx.terminal)
        .on_mount({
            let timer = timer.clone();
            move |_el| {
                #[cfg(target_arch = "wasm32")]
                crate::client::showcase::watch_terminal(_el, progress, tab, &timer);
                let _ = &timer;
            }
        })
        .child(
            div()
                .class("term-head")
                .child(dots())
                .child(span().class("term-title").child(icon(Icon::Terminal)).text(tx.terminal)),
        )
        .children(samples::TERMINAL.iter().enumerate().map(|(k, script)| {
            let shown = move |i: usize| tab.get() == k && i < progress.get().0;
            div()
                .class("term-body")
                .bool_attr("hidden", move || tab.get() != k)
                .children(script.iter().enumerate().map(move |(i, line)| {
                    match *line {
                        TermLine::Cmd(cmd) => div()
                            .class("term-line cmd")
                            .bool_attr("hidden", move || !(shown(i) || (tab.get() == k && progress.get().0 == i)))
                            .child(span().class("term-prompt").text("❯ "))
                            .text_dyn(move || {
                                let (done, typed) = progress.get();
                                if i < done { cmd.to_string() } else { cmd.chars().take(typed).collect() }
                            })
                            .child(span().class("term-caret").bool_attr("hidden", move || progress.get().0 != i)),
                        TermLine::Out(out) => {
                            div().class("term-line out").bool_attr("hidden", move || !shown(i)).text(out)
                        }
                        TermLine::Ok(out) => {
                            div().class("term-line ok").bool_attr("hidden", move || !shown(i)).text(out)
                        }
                    }
                }))
                .child(
                    div()
                        .class("term-line cmd")
                        .bool_attr("hidden", move || progress.get().0 < script.len())
                        .child(span().class("term-prompt").text("❯ "))
                        .child(span().class("term-caret")),
                )
        }));

    div().class("showcase").child(window).child(terminal).into()
}

fn dots() -> Element {
    span().class("cw-dots").attr("aria-hidden", "true").child(i()).child(i()).child(i())
}

/// One search result: `(slug, title, category, snippet)`.
pub type HitView = (String, String, String, Option<(String, String, String)>);

/// The documentation sidebar (`props`: `lang:slug`): search over every page of the
/// language, the categories (collapsible) and, on phones, the drawer that holds them.
pub fn docs_sidebar(props: &str) -> Node {
    let lang = lang(props);
    let current = props.split_once(':').map_or(docs::FIRST_SLUG, |(_, s)| s).to_string();
    let tx = t(lang);
    let idx = lang.idx();
    let drawer = use_state(false);
    let collapsed = use_state(0u32);
    let query = use_state(String::new());
    let results = use_state(Vec::<HitView>::new());
    let loaded = use_state(false);

    #[cfg(target_arch = "wasm32")]
    effect(move || crate::client::set_body_class("drawer-open", drawer.get()));

    let searching = move || query.with(|q| !q.trim().is_empty());
    let current_title = docs::title(&current, lang);

    let nav = nav().class("docs-nav").attr("aria-label", tx.docs_title).bool_attr("hidden", searching).children(
        CATEGORIES.iter().enumerate().map(|(ci, cat)| {
            let bit = 1u32 << ci;
            div()
                .class("docs-cat")
                .class_if("collapsed", move || collapsed.get() & bit != 0)
                .child(
                    button()
                        .attr("type", "button")
                        .class("docs-cat-title")
                        .attr_dyn("aria-expanded", move || (collapsed.get() & bit == 0).to_string())
                        .on_click(move |_| collapsed.update(|c| *c ^= bit))
                        .child(span().text(cat.title[idx]))
                        .child(span().class("chev").child(icon(Icon::ChevronRight))),
                )
                .child(ul().class("docs-cat-pages").children(cat.pages.iter().map(|page| {
                    let a = a()
                        .class("docs-link")
                        .href(link(&format!("/{}/docs/{}/", lang.code(), page.slug)))
                        .text(page.title[idx]);
                    li().child(if page.slug == current { a.class("active").attr("aria-current", "page") } else { a })
                })))
        }),
    );

    let current_slug = current.clone();
    let found = move || {
        let q = query.get();
        if q.trim().is_empty() {
            return Vec::new();
        }
        let list = results.get();
        if !loaded.get() {
            return vec![p().class("search-empty").text("…").into()];
        }
        if list.is_empty() {
            return vec![p().class("search-empty").text(format!("{} “{}”", tx.docs_no_results, q.trim())).into()];
        }
        let count = p().class("search-count").text(format!("{} {}", list.len(), tx.docs_results));
        let hits = list.into_iter().map(|(slug, title, category, snippet)| {
            let hit = a()
                .class("search-hit")
                .href(link(&format!("/{}/docs/{slug}/", lang.code())))
                .child(span().class("hit-cat").text(category))
                .child(span().class("hit-title").text(title))
                .child(snippet.map(|(before, matched, after)| {
                    span().class("hit-snippet").text(before).child(mark().text(matched)).text(after)
                }));
            Node::from(if slug == current_slug { hit.class("active") } else { hit })
        });
        vec![div().class("search-results").child(count).children(hits).into()]
    };

    let search = div()
        .class("docs-search")
        .child(icon(Icon::Search))
        .child(
            input()
                .attr("type", "search")
                .attr("placeholder", tx.docs_search)
                .attr("aria-label", tx.docs_search)
                .attr("autocomplete", "off")
                .attr("spellcheck", "false")
                .attr_dyn("value", move || query.get())
                .on_mount(|_input| {
                    #[cfg(target_arch = "wasm32")]
                    crate::client::search::shortcut(_input);
                })
                .on_input(move |e| {
                    let q = e.value();
                    query.set(q.clone());
                    #[cfg(target_arch = "wasm32")]
                    crate::client::search::query(lang, q, results, loaded);
                    let _ = q;
                }),
        )
        .child(kbd().attr("aria-hidden", "true").bool_attr("hidden", move || !query.with(String::is_empty)).text("/"))
        .child(
            button()
                .attr("type", "button")
                .class("search-clear")
                .attr("aria-label", tx.docs_clear)
                .bool_attr("hidden", move || query.with(String::is_empty))
                .on_click(move |_e| {
                    query.set(String::new());
                    #[cfg(target_arch = "wasm32")]
                    crate::client::search::focus_input(&_e);
                })
                .child(icon(Icon::Close)),
        );

    fragment([
        Node::from(
            button()
                .attr("type", "button")
                .class("docs-menu-btn")
                .attr_dyn("aria-expanded", move || drawer.get().to_string())
                .on_click(move |_| drawer.update(|d| *d = !*d))
                .child(icon(Icon::Menu))
                .child(span().text(tx.docs_menu))
                .child((!current_title.is_empty()).then(|| span().class("docs-menu-current").text(current_title))),
        ),
        div()
            .class("docs-overlay")
            .class_if("is-open", move || drawer.get())
            .attr("aria-hidden", "true")
            .on_click(move |_| drawer.set(false))
            .into(),
        aside()
            .class("docs-sidebar")
            .class_if("is-open", move || drawer.get())
            .child(
                div()
                    .class("docs-sidebar-head")
                    .child(span().class("docs-sidebar-title").child(icon(Icon::Book)).text(tx.docs_title))
                    .child(
                        button()
                            .attr("type", "button")
                            .class("icon-btn docs-close")
                            .attr("aria-label", tx.nav_close_menu)
                            .on_click(move |_| drawer.set(false))
                            .child(icon(Icon::Close)),
                    ),
            )
            .child(search)
            .child(nav)
            .children_dyn(found)
            .into(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn islands_render_on_the_server() {
        for (name, island_fn) in ISLANDS {
            let html = active::island(name, "es:models", island_fn).render();
            assert!(
                html.starts_with(&format!(r#"<active-island data-island="{name}" data-props="es:models">"#)),
                "{name}"
            );
        }
    }

    #[test]
    fn showcase_and_sidebar_content() {
        let html = active::island("showcase", "en", showcase).render();
        assert!(html.contains("models/user.rs"));
        // Every tab is rendered, the first one shown; the terminal is fully typed.
        assert_eq!(html.matches(r#"<div class="cw-body""#).count(), 4);
        assert!(html.contains("cargo build --release"));
        let side = active::island("docs-sidebar", "fr:queries", docs_sidebar).render();
        assert!(
            side.contains(r#"href="/fr/docs/queries/" aria-current="page""#) || side.contains("aria-current=\"page\"")
        );
        assert!(side.contains("Constructeur de requêtes"));
        let typing = active::island("typing", "fr", typing).render();
        assert!(typing.contains(t(Lang::Fr).hero_taglines[0]));
    }
}
