//! Browser bundle: hydrates the islands and starts the page motion.

pub mod motion;
pub mod particles;
pub mod search;
pub mod showcase;
pub mod typing;

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{Element, HtmlElement, IntersectionObserver, IntersectionObserverEntry, IntersectionObserverInit};

#[wasm_bindgen(start)]
pub fn start() {
    let root = document().document_element().expect_throw("no <html>");
    // Links of a site published under a sub-path (GitHub Pages: `/Memoire`).
    if let Some(base) = root.get_attribute("data-base") {
        crate::routes::set_base(&base);
    }
    // A static host's stand-in for a server redirect: go to the visitor's language.
    if let Some(target) = root.get_attribute("data-redirect") {
        motion::redirect(&target);
        return;
    }
    motion::theme(&root);
    active::hydrate(&crate::islands::ISLANDS);
    motion::init(&root);
}

pub fn window() -> web_sys::Window {
    web_sys::window().expect_throw("no window")
}

pub fn document() -> web_sys::Document {
    window().document().expect_throw("no document")
}

pub fn media(query: &str) -> bool {
    window().match_media(query).ok().flatten().is_some_and(|m| m.matches())
}

pub fn reduced_motion() -> bool {
    media("(prefers-reduced-motion: reduce)")
}

/// A desktop-like device: a mouse or a trackpad that can hover.
pub fn fine_pointer() -> bool {
    media("(hover: hover) and (pointer: fine)")
}

pub fn viewport_height() -> f64 {
    window().inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(800.0)
}

/// Whether `el` is on screen or above it (already scrolled past): what the visitor has
/// seen keeps the server's rendering instead of animating in.
pub fn seen(el: &Element) -> bool {
    el.get_bounding_client_rect().top() < viewport_height()
}

pub fn set_body_class(class: &str, on: bool) {
    if let Some(body) = document().body() {
        let _ = body.class_list().toggle_with_force(class, on);
    }
}

pub fn set_var(el: &HtmlElement, name: &str, value: &str) {
    let _ = el.style().set_property(name, value);
}

/// Adds an event listener for the page's lifetime.
pub fn on<E: JsCast + 'static>(target: &web_sys::EventTarget, event: &str, f: impl FnMut(E) + 'static) {
    let mut f = f;
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| f(e.unchecked_into()));
    let _ = target.add_event_listener_with_callback(event, closure.as_ref().unchecked_ref());
    closure.forget();
}

/// Like [`on`], with `{ passive: true }` (scroll and pointer listeners).
pub fn on_passive<E: JsCast + 'static>(target: &web_sys::EventTarget, event: &str, f: impl FnMut(E) + 'static) {
    let mut f = f;
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| f(e.unchecked_into()));
    let options = web_sys::AddEventListenerOptions::new();
    options.set_passive(true);
    let _ = target.add_event_listener_with_callback_and_add_event_listener_options(
        event,
        closure.as_ref().unchecked_ref(),
        &options,
    );
    closure.forget();
}

type FrameCallback = Closure<dyn FnMut(f64)>;

/// Calls `f(timestamp)` on every animation frame while it returns `true`.
pub fn request_frame(f: impl FnMut(f64) -> bool + 'static) {
    let f = Rc::new(RefCell::new(f));
    let slot: Rc<RefCell<Option<FrameCallback>>> = Rc::new(RefCell::new(None));
    let again = slot.clone();
    *slot.borrow_mut() = Some(Closure::new(move |now: f64| {
        if (f.borrow_mut())(now) {
            if let Some(cb) = again.borrow().as_ref() {
                let _ = window().request_animation_frame(cb.as_ref().unchecked_ref());
            }
        } else {
            again.borrow_mut().take();
        }
    }));
    if let Some(cb) = slot.borrow().as_ref() {
        let _ = window().request_animation_frame(cb.as_ref().unchecked_ref());
    }
}

/// Every element matching `selector`.
pub fn all(selector: &str) -> Vec<HtmlElement> {
    let Ok(list) = document().query_selector_all(selector) else {
        return Vec::new();
    };
    (0..list.length()).filter_map(|i| list.item(i)?.dyn_into().ok()).collect()
}

/// Watches `elements` with one IntersectionObserver for the page's lifetime.
pub fn observe(
    elements: &[HtmlElement],
    threshold: f64,
    margin: &str,
    f: impl Fn(&IntersectionObserverEntry, &IntersectionObserver) + 'static,
) {
    if elements.is_empty() {
        return;
    }
    let callback = Closure::<dyn Fn(js_sys::Array, IntersectionObserver)>::new(
        move |entries: js_sys::Array, io: IntersectionObserver| {
            for entry in entries.iter() {
                if let Ok(entry) = entry.dyn_into::<IntersectionObserverEntry>() {
                    f(&entry, &io);
                }
            }
        },
    );
    let options = IntersectionObserverInit::new();
    options.set_threshold(&JsValue::from_f64(threshold));
    options.set_root_margin(margin);
    if let Ok(io) = IntersectionObserver::new_with_options(callback.as_ref().unchecked_ref(), &options) {
        for el in elements {
            io.observe(el);
        }
    }
    callback.forget();
}

/// Calls `f` once, the first time `el` is at least `threshold` visible.
pub fn when_visible(el: &Element, threshold: f64, f: impl FnOnce() + 'static) {
    let Ok(el) = el.clone().dyn_into::<HtmlElement>() else { return };
    let f = RefCell::new(Some(f));
    observe(&[el], threshold, "0px 0px -8% 0px", move |entry, io| {
        if entry.is_intersecting() {
            io.disconnect();
            if let Some(f) = f.borrow_mut().take() {
                f();
            }
        }
    });
}
