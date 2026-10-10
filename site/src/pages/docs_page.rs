//! `/docs` and `/docs/:slug`: sidebar (search + categories), rendered markdown,
//! "On this page" table of contents, prev / next navigation.

use std::collections::HashSet;

use gloo::events::{EventListener, EventListenerOptions};
use gloo::timers::callback::Timeout;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Element, HtmlElement, HtmlInputElement, KeyboardEvent};
use yew::prelude::*;
use yew_router::prelude::*;

use crate::app::Route;
use crate::components::icons::Icon;
use crate::docs::{self, CATEGORIES};
use crate::dom;
use crate::i18n::{use_lang, Lang};
use crate::markdown::{self, Labels, Rendered};

#[derive(Properties, PartialEq)]
pub struct DocsPageProps {
    pub slug: AttrValue,
}

fn render_page(slug: &str, lang: Lang) -> Option<Rendered> {
    let t = lang.t();
    docs::find(slug).map(|(_, page)| {
        markdown::render(
            page.content[lang.idx()],
            &Labels {
                copy: t.copy,
                note: t.callout_note,
                tip: t.callout_tip,
                warning: t.callout_warning,
            },
        )
    })
}

fn current_hash() -> Option<String> {
    let hash = dom::window().location().hash().ok()?;
    let id = hash.strip_prefix('#')?;
    (!id.is_empty()).then(|| js_sys::decode_uri_component(id).ok().map(String::from).unwrap_or_else(|| id.to_string()))
}

fn replace_hash(id: &str) {
    if let Ok(history) = dom::window().history() {
        let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&format!("#{id}")));
    }
}

#[component]
pub fn DocsPage(props: &DocsPageProps) -> Html {
    let ctx = use_lang();
    let lang = ctx.lang;
    let t = lang.t();
    let li = lang.idx();
    let slug = props.slug.to_string();
    let navigator = use_navigator();

    let query = use_state(String::new);
    let drawer = use_state_eq(|| false);
    let collapsed = use_state(HashSet::<usize>::new);
    let active_id = use_state_eq(String::new);
    let search_ref = use_node_ref();
    let copy_timer = use_mut_ref(|| None::<Timeout>);
    let hash_timer = use_mut_ref(|| None::<Timeout>);

    let rendered = use_memo((slug.clone(), lang), |(slug, lang)| render_page(slug, *lang));
    let results = use_memo(((*query).clone(), lang), |(q, lang)| docs::search(q, *lang));
    let found = docs::find(&slug);

    // Close the drawer when the page changes; jump to #hash if present.
    {
        let (drawer, hash_timer) = (drawer.clone(), hash_timer.clone());
        use_effect_with(slug.clone(), move |_| {
            drawer.set(false);
            if let Some(id) = current_hash() {
                *hash_timer.borrow_mut() = Some(Timeout::new(80, move || {
                    dom::scroll_to_id(&id);
                }));
            }
        });
    }

    // Lock the body scroll while the mobile drawer is open.
    use_effect_with(*drawer, |&open| {
        dom::set_body_class("drawer-open", open);
        move || dom::set_body_class("drawer-open", false)
    });

    // Scroll-spy for the "On this page" list.
    {
        let active_id = active_id.clone();
        let ids: Vec<String> = rendered
            .as_ref()
            .as_ref()
            .map(|r| r.toc.iter().map(|e| e.id.clone()).collect())
            .unwrap_or_default();
        use_effect_with(ids, move |ids| {
            let ids = ids.clone();
            let update = move || {
                let doc = dom::document();
                let mut current = String::new();
                for id in &ids {
                    match doc.get_element_by_id(id) {
                        Some(el) if el.get_bounding_client_rect().top() < 160.0 => current = id.clone(),
                        Some(_) => break,
                        None => {}
                    }
                }
                active_id.set(current);
            };
            update();
            let listener = EventListener::new(&dom::window(), "scroll", move |_| update());
            move || drop(listener)
        });
    }

    // "/" focuses the search box.
    {
        let search_ref = search_ref.clone();
        use_effect_with((), move |_| {
            // Non-passive so that preventDefault() keeps the "/" out of the input.
            let options = EventListenerOptions::enable_prevent_default();
            let listener = EventListener::new_with_options(&dom::document(), "keydown", options, move |e| {
                let Some(k) = e.dyn_ref::<KeyboardEvent>() else { return };
                if k.key() != "/" || k.ctrl_key() || k.meta_key() {
                    return;
                }
                let typing = dom::document()
                    .active_element()
                    .is_some_and(|el| matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT"));
                if !typing {
                    if let Some(input) = search_ref.cast::<HtmlInputElement>() {
                        k.prevent_default();
                        let _ = input.focus();
                    }
                }
            });
            move || drop(listener)
        });
    }

    // Event delegation inside the rendered markdown: copy buttons + links.
    let on_article_click = {
        let navigator = navigator.clone();
        let copy_timer = copy_timer.clone();
        let (copy_label, copied_label) = (t.copy, t.copied);
        Callback::from(move |e: MouseEvent| {
            let Some(target) = e.target_dyn_into::<Element>() else { return };
            if let Some(btn) = target.closest(".code-copy").ok().flatten() {
                if let Some(code) = btn
                    .closest(".code-block")
                    .ok()
                    .flatten()
                    .and_then(|b| b.query_selector("code").ok().flatten())
                {
                    dom::copy_to_clipboard(&code.text_content().unwrap_or_default());
                }
                btn.set_text_content(Some(copied_label));
                let _ = btn.class_list().add_1("is-copied");
                *copy_timer.borrow_mut() = Some(Timeout::new(1500, move || {
                    btn.set_text_content(Some(copy_label));
                    let _ = btn.class_list().remove_1("is-copied");
                }));
                return;
            }
            let Some(a) = target.closest("a").ok().flatten() else { return };
            if e.ctrl_key() || e.meta_key() || e.shift_key() || e.alt_key() || e.button() != 0 {
                return;
            }
            if a.get_attribute("target").is_some() {
                return;
            }
            let href = a.get_attribute("href").unwrap_or_default();
            if let Some(id) = href.strip_prefix('#') {
                e.prevent_default();
                dom::scroll_to_id(id);
                replace_hash(id);
            } else if href.starts_with('/') {
                let (path, hash) = match href.split_once('#') {
                    Some((p, h)) => (p.to_string(), Some(h.to_string())),
                    None => (href.clone(), None),
                };
                // Links carry the site's base path; routes do not.
                let path = path.strip_prefix(dom::base_path()).unwrap_or(&path);
                if let (Some(nav), Some(route)) = (navigator.as_ref(), Route::recognize(path)) {
                    e.prevent_default();
                    nav.push(&route);
                    if let Some(id) = hash {
                        Timeout::new(120, move || {
                            dom::scroll_to_id(&id);
                            replace_hash(&id);
                        })
                        .forget();
                    }
                }
            }
        })
    };

    let on_search = {
        let query = query.clone();
        Callback::from(move |e: InputEvent| query.set(e.target_unchecked_into::<HtmlInputElement>().value()))
    };
    let clear_search = {
        let (query, search_ref) = (query.clone(), search_ref.clone());
        Callback::from(move |_| {
            query.set(String::new());
            if let Some(input) = search_ref.cast::<HtmlElement>() {
                let _ = input.focus();
            }
        })
    };
    let toggle_drawer = {
        let drawer = drawer.clone();
        Callback::from(move |_| drawer.set(!*drawer))
    };
    let close_drawer = {
        let drawer = drawer.clone();
        Callback::from(move |_| drawer.set(false))
    };

    // ---- Sidebar -------------------------------------------------------
    let sidebar_body = if !query.trim().is_empty() {
        if results.is_empty() {
            html! { <p class="search-empty">{ format!("{} “{}”", t.docs_no_results, query.trim()) }</p> }
        } else {
            html! {
                <div class="search-results">
                    <p class="search-count">{ format!("{} {}", results.len(), t.docs_results) }</p>
                    { for results.iter().map(|hit| html! {
                        <Link<Route> to={Route::Doc { slug: hit.slug.to_string() }} classes={classes!("search-hit", (hit.slug == slug).then_some("active"))}>
                            <span class="hit-cat">{ hit.category }</span>
                            <span class="hit-title">{ hit.title }</span>
                            if let Some((before, m, after)) = &hit.snippet {
                                <span class="hit-snippet">{ before.clone() }<mark>{ m.clone() }</mark>{ after.clone() }</span>
                            }
                        </Link<Route>>
                    }) }
                </div>
            }
        }
    } else {
        html! {
            <nav class="docs-nav" aria-label={t.docs_title}>
                { for CATEGORIES.iter().enumerate().map(|(ci, cat)| {
                    let is_collapsed = collapsed.contains(&ci);
                    let toggle = {
                        let collapsed = collapsed.clone();
                        Callback::from(move |_| {
                            let mut next = (*collapsed).clone();
                            if !next.remove(&ci) {
                                next.insert(ci);
                            }
                            collapsed.set(next);
                        })
                    };
                    html! {
                        <div class={classes!("docs-cat", is_collapsed.then_some("collapsed"))}>
                            <button type="button" class="docs-cat-title" onclick={toggle} aria-expanded={if is_collapsed { "false" } else { "true" }}>
                                <span>{ cat.title[li] }</span>
                                <span class="chev">{ Icon::ChevronRight.html() }</span>
                            </button>
                            <ul class="docs-cat-pages">
                                { for cat.pages.iter().map(|page| html! {
                                    <li>
                                        <Link<Route> to={Route::Doc { slug: page.slug.to_string() }} classes={classes!("docs-link", (page.slug == slug).then_some("active"))}>
                                            { page.title[li] }
                                        </Link<Route>>
                                    </li>
                                }) }
                            </ul>
                        </div>
                    }
                }) }
            </nav>
        }
    };

    // ---- Article -------------------------------------------------------
    let article = match (found, rendered.as_ref().as_ref()) {
        (Some((cat, page)), Some(r)) => {
            let (prev, next) = docs::neighbours(&slug);
            let minutes = docs::reading_minutes(r.words);
            html! {
                <article class="docs-article" key={format!("{slug}-{}", lang.code())}>
                    <nav class="breadcrumb" aria-label="Breadcrumb">
                        <Link<Route> to={Route::DocsIndex}>{ t.nav_docs }</Link<Route>>
                        <span class="sep">{ Icon::ChevronRight.html() }</span>
                        <span>{ cat.title[li] }</span>
                        <span class="sep">{ Icon::ChevronRight.html() }</span>
                        <span class="current" aria-current="page">{ page.title[li] }</span>
                    </nav>
                    <div class="doc-meta">
                        <span>{ Icon::Clock.html() }{ format!("{minutes} {}", t.docs_min_read) }</span>
                        <a href={docs::edit_url(lang, &slug)} target="_blank" rel="noopener noreferrer">
                            { Icon::Edit.html() }{ t.docs_edit }
                        </a>
                    </div>
                    <div class="markdown-body" onclick={on_article_click}>
                        { Html::from_html_unchecked(AttrValue::from(r.html.clone())) }
                    </div>
                    <nav class="doc-pager">
                        if let Some(p) = prev {
                            <Link<Route> to={Route::Doc { slug: p.slug.to_string() }} classes={classes!("pager", "prev", "spotlight")}>
                                <span class="pager-label">{ Icon::ArrowLeft.html() }{ t.docs_prev }</span>
                                <span class="pager-title">{ p.title[li] }</span>
                            </Link<Route>>
                        } else { <span></span> }
                        if let Some(n) = next {
                            <Link<Route> to={Route::Doc { slug: n.slug.to_string() }} classes={classes!("pager", "next", "spotlight")}>
                                <span class="pager-label">{ t.docs_next }{ Icon::ArrowRight.html() }</span>
                                <span class="pager-title">{ n.title[li] }</span>
                            </Link<Route>>
                        }
                    </nav>
                </article>
            }
        }
        _ => html! {
            <article class="docs-article" key="not-found">
                <div class="markdown-body">
                    <h1>{ t.docs_not_found_title }</h1>
                    <p>{ t.docs_not_found_text }</p>
                </div>
                <Link<Route> to={Route::DocsIndex} classes={classes!("btn", "btn-primary")}>
                    { Icon::Book.html() }<span>{ docs::title(docs::FIRST_SLUG, lang) }</span>
                </Link<Route>>
            </article>
        },
    };

    // ---- Table of contents ---------------------------------------------
    let toc = rendered.as_ref().as_ref().map(|r| r.toc.clone()).unwrap_or_default();
    let toc_html = if toc.is_empty() {
        html! {}
    } else {
        html! {
            <div class="toc">
                <p class="toc-title">{ t.docs_on_this_page }</p>
                <ul>
                    { for toc.iter().map(|entry| {
                        let id = entry.id.clone();
                        let active_id = active_id.clone();
                        let is_active = *active_id == entry.id;
                        let onclick = Callback::from(move |e: MouseEvent| {
                            e.prevent_default();
                            dom::scroll_to_id(&id);
                            replace_hash(&id);
                            active_id.set(id.clone());
                        });
                        html! {
                            <li class={classes!(format!("toc-l{}", entry.level), is_active.then_some("active"))}>
                                <a href={format!("#{}", entry.id)} {onclick}>{ entry.text.clone() }</a>
                            </li>
                        }
                    }) }
                </ul>
            </div>
        }
    };

    html! {
        <div class="docs container-wide">
            <button type="button" class="docs-menu-btn" onclick={toggle_drawer} aria-expanded={if *drawer { "true" } else { "false" }}>
                { Icon::Menu.html() }<span>{ t.docs_menu }</span>
                if let Some((_, page)) = found {
                    <span class="docs-menu-current">{ page.title[li] }</span>
                }
            </button>
            <div class={classes!("docs-overlay", drawer.then_some("is-open"))} onclick={close_drawer.clone()} aria-hidden="true"></div>
            <aside class={classes!("docs-sidebar", drawer.then_some("is-open"))}>
                <div class="docs-sidebar-head">
                    <span class="docs-sidebar-title">{ Icon::Book.html() }{ t.docs_title }</span>
                    <button type="button" class="icon-btn docs-close" onclick={close_drawer} aria-label={t.nav_close_menu}>{ Icon::Close.html() }</button>
                </div>
                <div class="docs-search">
                    { Icon::Search.html() }
                    <input
                        ref={search_ref}
                        type="search"
                        placeholder={t.docs_search}
                        aria-label={t.docs_search}
                        value={(*query).clone()}
                        oninput={on_search}
                        autocomplete="off"
                        spellcheck="false"
                    />
                    if query.is_empty() {
                        <kbd aria-hidden="true">{ "/" }</kbd>
                    } else {
                        <button type="button" class="search-clear" onclick={clear_search} aria-label="Clear">{ Icon::Close.html() }</button>
                    }
                </div>
                { sidebar_body }
            </aside>
            <div class="docs-main">{ article }</div>
            <aside class="docs-toc">{ toc_html }</aside>
        </div>
    }
}
