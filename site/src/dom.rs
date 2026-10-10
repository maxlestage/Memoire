//! Small browser helpers shared by components (storage, media queries,
//! clipboard, requestAnimationFrame loops, IntersectionObserver…).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{
    Document, Element, HtmlElement, IntersectionObserver, IntersectionObserverEntry,
    IntersectionObserverInit, ScrollBehavior, ScrollToOptions, Window,
};

/// Path the site is served under, without a trailing slash: empty at the root
/// of a domain (Heroku), `/Memoire` for a GitHub Pages project site. Set with
/// the `MEMOIRE_BASE_PATH` environment variable when building.
pub fn base_path() -> &'static str {
    option_env!("MEMOIRE_BASE_PATH").unwrap_or("").trim_end_matches('/')
}

pub fn window() -> Window {
    web_sys::window().expect_throw("no global window")
}

pub fn document() -> Document {
    window().document().expect_throw("no document")
}

pub fn storage_get(key: &str) -> Option<String> {
    window()
        .local_storage()
        .ok()
        .flatten()
        .and_then(|s| s.get_item(key).ok().flatten())
}

/// Downloads a text file (same origin), e.g. a documentation page.
pub async fn fetch_text(url: &str) -> Result<String, String> {
    let fail = |e: JsValue| e.as_string().unwrap_or_else(|| format!("{e:?}"));
    let response: web_sys::Response = wasm_bindgen_futures::JsFuture::from(window().fetch_with_str(url))
        .await
        .map_err(fail)?
        .dyn_into()
        .map_err(fail)?;
    if !response.ok() {
        return Err(format!("HTTP {} for {url}", response.status()));
    }
    let text = wasm_bindgen_futures::JsFuture::from(response.text().map_err(fail)?).await.map_err(fail)?;
    Ok(text.as_string().unwrap_or_default())
}

pub fn storage_set(key: &str, value: &str) {
    if let Ok(Some(storage)) = window().local_storage() {
        let _ = storage.set_item(key, value);
    }
}

pub fn media_matches(query: &str) -> bool {
    window()
        .match_media(query)
        .ok()
        .flatten()
        .map(|m| m.matches())
        .unwrap_or(false)
}

pub fn prefers_reduced_motion() -> bool {
    media_matches("(prefers-reduced-motion: reduce)")
}

/// Desktop-like device: a mouse/trackpad that can hover.
pub fn has_fine_pointer() -> bool {
    media_matches("(hover: hover) and (pointer: fine)")
}

pub fn scroll_y() -> f64 {
    window().scroll_y().unwrap_or(0.0)
}

pub fn viewport_height() -> f64 {
    window()
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(800.0)
}

pub fn scroll_to_top(smooth: bool) {
    let opts = ScrollToOptions::new();
    opts.set_top(0.0);
    opts.set_behavior(if smooth && !prefers_reduced_motion() {
        ScrollBehavior::Smooth
    } else {
        ScrollBehavior::Instant
    });
    window().scroll_to_with_scroll_to_options(&opts);
}

/// Smoothly scroll an element (by id) into view; CSS `scroll-margin-top` handles the navbar.
pub fn scroll_to_id(id: &str) -> bool {
    match document().get_element_by_id(id) {
        Some(el) => {
            let opts = web_sys::ScrollIntoViewOptions::new();
            opts.set_behavior(if prefers_reduced_motion() {
                ScrollBehavior::Instant
            } else {
                ScrollBehavior::Smooth
            });
            opts.set_block(web_sys::ScrollLogicalPosition::Start);
            el.scroll_into_view_with_scroll_into_view_options(&opts);
            true
        }
        None => false,
    }
}

pub fn copy_to_clipboard(text: &str) {
    let promise = window().navigator().clipboard().write_text(text);
    // Swallow rejections (e.g. insecure context) instead of logging an unhandled error.
    let noop = Closure::<dyn FnMut(JsValue)>::new(|_| {});
    let _ = promise.catch(&noop);
    noop.forget();
}

pub fn now_ms() -> f64 {
    window().performance().map(|p| p.now()).unwrap_or(0.0)
}

pub fn set_css_var(el: &HtmlElement, name: &str, value: &str) {
    let _ = el.style().set_property(name, value);
}

pub fn set_title(title: &str) {
    document().set_title(title);
}

pub fn set_meta(name: &str, content: &str) {
    let selector = format!("meta[name=\"{name}\"]");
    if let Ok(Some(meta)) = document().query_selector(&selector) {
        let _ = meta.set_attribute("content", content);
    }
}

pub fn set_body_class(class: &str, on: bool) {
    if let Some(body) = document().body() {
        let list = body.class_list();
        let _ = if on { list.add_1(class) } else { list.remove_1(class) };
    }
}

// ---------------------------------------------------------------------------
// requestAnimationFrame loop
// ---------------------------------------------------------------------------

type FrameCallback = Closure<dyn FnMut(f64)>;

struct RafInner {
    cancelled: Cell<bool>,
    id: Cell<i32>,
    closure: RefCell<Option<FrameCallback>>,
}

/// Runs `tick(timestamp)` on every animation frame while it returns `true`.
/// Dropping the handle cancels the loop.
pub struct RafLoop {
    inner: Rc<RafInner>,
}

impl RafLoop {
    pub fn start(mut tick: impl FnMut(f64) -> bool + 'static) -> Self {
        let inner = Rc::new(RafInner {
            cancelled: Cell::new(false),
            id: Cell::new(0),
            closure: RefCell::new(None),
        });
        let weak: Weak<RafInner> = Rc::downgrade(&inner);
        let closure: FrameCallback = Closure::new(move |ts: f64| {
            let Some(inner) = weak.upgrade() else { return };
            if inner.cancelled.get() {
                return;
            }
            if tick(ts) {
                schedule(&inner);
            }
        });
        *inner.closure.borrow_mut() = Some(closure);
        schedule(&inner);
        RafLoop { inner }
    }
}

fn schedule(inner: &RafInner) {
    if let Some(cb) = inner.closure.borrow().as_ref() {
        if let Ok(id) = window().request_animation_frame(cb.as_ref().unchecked_ref()) {
            inner.id.set(id);
        }
    }
}

impl Drop for RafLoop {
    fn drop(&mut self) {
        self.inner.cancelled.set(true);
        let _ = window().cancel_animation_frame(self.inner.id.get());
    }
}

// ---------------------------------------------------------------------------
// IntersectionObserver helpers
// ---------------------------------------------------------------------------

type IoCallback = Closure<dyn FnMut(js_sys::Array, IntersectionObserver)>;

/// Keeps an observer alive; disconnects it on drop.
pub struct IoHandle {
    observer: IntersectionObserver,
    _closure: IoCallback,
}

impl Drop for IoHandle {
    fn drop(&mut self) {
        self.observer.disconnect();
    }
}

fn make_observer(
    threshold: f64,
    root_margin: &str,
    mut on_entry: impl FnMut(&IntersectionObserverEntry, &IntersectionObserver) + 'static,
) -> Option<(IntersectionObserver, IoCallback)> {
    let closure: IoCallback = Closure::new(move |entries: js_sys::Array, obs: IntersectionObserver| {
        for entry in entries.iter() {
            let entry: IntersectionObserverEntry = entry.unchecked_into();
            on_entry(&entry, &obs);
        }
    });
    let init = IntersectionObserverInit::new();
    init.set_threshold_f64(threshold);
    init.set_root_margin(root_margin);
    let observer =
        IntersectionObserver::new_with_options(closure.as_ref().unchecked_ref(), &init).ok()?;
    Some((observer, closure))
}

/// Calls `on_change(is_visible)` whenever the element enters / leaves the viewport.
pub fn observe_visibility(
    el: &Element,
    threshold: f64,
    mut on_change: impl FnMut(bool) + 'static,
) -> Option<IoHandle> {
    let (observer, closure) = make_observer(threshold, "0px", move |entry, _| {
        on_change(entry.is_intersecting());
    })?;
    observer.observe(el);
    Some(IoHandle {
        observer,
        _closure: closure,
    })
}

/// Calls `on_visible` once, the first time the element becomes visible.
/// If IntersectionObserver is unavailable it fires immediately.
pub fn observe_once(
    el: &Element,
    threshold: f64,
    on_visible: impl FnOnce() + 'static,
) -> Option<IoHandle> {
    let slot = Rc::new(RefCell::new(Some(on_visible)));
    let slot2 = slot.clone();
    match make_observer(threshold, "0px 0px -8% 0px", move |entry, obs| {
        if entry.is_intersecting() {
            obs.disconnect();
            if let Some(f) = slot2.borrow_mut().take() {
                f();
            }
        }
    }) {
        Some((observer, closure)) => {
            observer.observe(el);
            Some(IoHandle {
                observer,
                _closure: closure,
            })
        }
        None => {
            if let Some(f) = slot.borrow_mut().take() {
                f();
            }
            None
        }
    }
}

// Shared observer for scroll-reveal animations: one observer for the whole app.
thread_local! {
    static REVEAL: RefCell<Option<(IntersectionObserver, IoCallback)>> = const { RefCell::new(None) };
}

/// Adds `is-visible` to the element once it scrolls into view.
pub fn reveal_observe(el: &Element) {
    if prefers_reduced_motion() {
        let _ = el.class_list().add_1("is-visible");
        return;
    }
    let observed = REVEAL.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = make_observer(0.12, "0px 0px -6% 0px", |entry, obs| {
                if entry.is_intersecting() {
                    let target = entry.target();
                    let _ = target.class_list().add_1("is-visible");
                    obs.unobserve(&target);
                }
            });
        }
        match slot.as_ref() {
            Some((observer, _)) => {
                observer.observe(el);
                true
            }
            None => false,
        }
    });
    if !observed {
        let _ = el.class_list().add_1("is-visible");
    }
}

pub fn reveal_unobserve(el: &Element) {
    REVEAL.with(|slot| {
        if let Some((observer, _)) = slot.borrow().as_ref() {
            observer.unobserve(el);
        }
    });
}
