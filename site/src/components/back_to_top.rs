//! Floating "back to top" button with a circular scroll-progress ring.

use gloo::events::EventListener;
use web_sys::HtmlElement;
use yew::prelude::*;

use super::icons::Icon;
use crate::dom;
use crate::i18n::use_lang;

const RING: &str = r#"<svg class="to-top-ring" viewBox="0 0 48 48" aria-hidden="true"><circle class="ring-bg" cx="24" cy="24" r="21"/><circle class="ring-fg" cx="24" cy="24" r="21" pathLength="100"/></svg>"#;

pub fn scroll_progress() -> f64 {
    let doc = dom::document();
    let max = doc
        .document_element()
        .map(|e| f64::from(e.scroll_height()) - dom::viewport_height())
        .unwrap_or(1.0);
    if max <= 0.0 {
        0.0
    } else {
        (dom::scroll_y() / max).clamp(0.0, 1.0)
    }
}

#[component]
pub fn BackToTop() -> Html {
    let t = use_lang().lang.t();
    let visible = use_state_eq(|| false);
    let node = use_node_ref();
    {
        let (visible, node) = (visible.clone(), node.clone());
        use_effect_with((), move |_| {
            let listener = EventListener::new(&dom::window(), "scroll", move |_| {
                visible.set(dom::scroll_y() > 640.0);
                if let Some(el) = node.cast::<HtmlElement>() {
                    dom::set_css_var(&el, "--p", &format!("{:.3}", scroll_progress()));
                }
            });
            move || drop(listener)
        });
    }
    html! {
        <button
            ref={node}
            type="button"
            class={classes!("to-top", "magnetic", visible.then_some("is-visible"))}
            aria-label={t.back_to_top}
            title={t.back_to_top}
            onclick={Callback::from(|_| dom::scroll_to_top(true))}
        >
            { Html::from_html_unchecked(AttrValue::Static(RING)) }
            { Icon::ArrowUp.html() }
        </button>
    }
}
