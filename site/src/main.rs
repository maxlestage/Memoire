//! Memoire ORM presentation website (Yew + WebAssembly).

mod app;
mod components;
mod data;
mod docs;
mod dom;
mod highlight;
mod i18n;
mod markdown;
mod pages;
mod theme;

fn main() {
    match dom::document().get_element_by_id("app") {
        Some(root) => yew::Renderer::<app::App>::with_root(root).render(),
        None => yew::Renderer::<app::App>::new().render(),
    };
}
