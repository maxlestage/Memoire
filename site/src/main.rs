//! Memoire ORM presentation website (Yew + WebAssembly).

// The native build is only the prerenderer: browser-only code goes unused there.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

mod app;
mod components;
mod data;
mod docs;
mod dom;
mod engine;
mod highlight;
mod i18n;
mod markdown;
mod pages;
#[cfg(not(target_arch = "wasm32"))]
mod prerender;
mod theme;

#[cfg(target_arch = "wasm32")]
fn main() {
    dom::init_settled();
    docs::adopt_embedded_sources();
    match dom::document().get_element_by_id("app") {
        Some(root) => yew::Renderer::<app::App>::with_root(root).render(),
        None => yew::Renderer::<app::App>::new().render(),
    };
}

/// Native build: the build-time prerenderer (see `prerender.rs`).
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    prerender::main();
}
