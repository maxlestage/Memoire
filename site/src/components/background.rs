//! Fixed animated backdrop: gradient mesh, floating blurred blobs and a subtle grid,
//! with parallax on scroll (`--sy`) and mouse (`--mx`, `--my`).

use gloo::events::EventListener;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, PointerEvent};
use yew::prelude::*;

use crate::dom;

#[component]
pub fn Background() -> Html {
    let node = use_node_ref();
    {
        let node = node.clone();
        use_effect_with((), move |_| {
            let mut listeners: Vec<EventListener> = Vec::new();
            if let Some(el) = node.cast::<HtmlElement>() {
                if !dom::prefers_reduced_motion() {
                    let scroll_el = el.clone();
                    listeners.push(EventListener::new(&dom::window(), "scroll", move |_| {
                        dom::set_css_var(&scroll_el, "--sy", &format!("{:.0}", dom::scroll_y()));
                    }));
                    if dom::has_fine_pointer() {
                        listeners.push(EventListener::new(&dom::window(), "pointermove", move |e| {
                            let Some(e) = e.dyn_ref::<PointerEvent>() else { return };
                            let w = dom::window();
                            let iw = w.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1.0);
                            let ih = dom::viewport_height();
                            let x = f64::from(e.client_x()) / iw * 2.0 - 1.0;
                            let y = f64::from(e.client_y()) / ih * 2.0 - 1.0;
                            dom::set_css_var(&el, "--mx", &format!("{x:.3}"));
                            dom::set_css_var(&el, "--my", &format!("{y:.3}"));
                        }));
                    }
                }
            }
            move || drop(listeners)
        });
    }
    html! {
        <div ref={node} class="bg" aria-hidden="true">
            <div class="bg-mesh"></div>
            <div class="blob-wrap b1"><div class="blob"></div></div>
            <div class="blob-wrap b2"><div class="blob"></div></div>
            <div class="blob-wrap b3"><div class="blob"></div></div>
            <div class="blob-wrap b4"><div class="blob"></div></div>
            <div class="bg-grid"></div>
            <div class="bg-vignette"></div>
        </div>
    }
}
