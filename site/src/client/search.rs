//! Browser side of the documentation search (the `docs-sidebar` island).

use std::cell::RefCell;
use std::rc::Rc;

use active::{Event, State};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{HtmlElement, KeyboardEvent};

use super::{on, window};
use crate::islands::HitView;
use crate::routes::{Lang, link};
use crate::search::{Entry, search};

type Results = State<Vec<HitView>>;

thread_local! {
    /// The search index of the page's language, downloaded on first use.
    static INDEX: RefCell<Option<Rc<Vec<Entry>>>> = const { RefCell::new(None) };
    /// The latest query typed while the index downloads; `Some` while downloading.
    static PENDING: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Shows the pages matching `q`. The first call downloads `/search/{lang}.txt` (the plain
/// text of every page of the language); then every key press is answered locally (and
/// offline, once the service worker has kept the file).
pub fn query(lang: Lang, q: String, results: Results, loaded: State<bool>) {
    if let Some(index) = INDEX.with(|i| i.borrow().clone()) {
        show(&index, &q, results, loaded);
        return;
    }
    let downloading = PENDING.with(|p| p.replace(Some(q)).is_some());
    if downloading {
        return;
    }
    let url = link(&format!("/search/{}.txt", lang.code()));
    spawn_local(async move {
        let text = fetch_text(&url).await;
        let index = Rc::new(text.as_deref().unwrap_or_default().lines().filter_map(Entry::parse).collect::<Vec<_>>());
        if text.is_some() {
            INDEX.with(|i| *i.borrow_mut() = Some(index.clone()));
        }
        let q = PENDING.with(|p| p.borrow_mut().take()).unwrap_or_default();
        show(&index, &q, results, loaded);
    });
}

fn show(index: &[Entry], q: &str, results: Results, loaded: State<bool>) {
    let hits = search(index, q)
        .into_iter()
        .map(|hit| (hit.entry.slug.clone(), hit.entry.title.clone(), hit.entry.category.clone(), hit.snippet))
        .collect();
    results.set(hits);
    loaded.set(true);
}

async fn fetch_text(url: &str) -> Option<String> {
    let response = JsFuture::from(window().fetch_with_str(url)).await.ok()?;
    let response = response.dyn_into::<web_sys::Response>().ok()?;
    if !response.ok() {
        return None;
    }
    JsFuture::from(response.text().ok()?).await.ok()?.as_string()
}

/// "/" focuses the search field from anywhere on the page.
pub fn shortcut(input: &web_sys::Element) {
    let Ok(input) = input.clone().dyn_into::<HtmlElement>() else { return };
    on(&window(), "keydown", move |e: KeyboardEvent| {
        if e.key() != "/" || e.ctrl_key() || e.meta_key() || e.alt_key() {
            return;
        }
        let typing = super::document()
            .active_element()
            .is_some_and(|el| matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT"));
        if !typing {
            e.prevent_default();
            let _ = input.focus();
        }
    });
}

/// After "clear": back to the field.
pub fn focus_input(e: &Event) {
    let input = e
        .current_target()
        .and_then(|button| button.closest(".docs-search").ok().flatten())
        .and_then(|search| search.query_selector("input").ok().flatten())
        .and_then(|input| input.dyn_into::<HtmlElement>().ok());
    if let Some(input) = input {
        let _ = input.focus();
    }
}
