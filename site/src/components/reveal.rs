//! Scroll-reveal wrapper: content fades / scales in when it enters the viewport.

use web_sys::Element;
use yew::prelude::*;

use crate::dom;

#[derive(Properties, PartialEq)]
pub struct RevealProps {
    #[prop_or_default]
    pub children: Html,
    /// Animation class: `fade-up` (default), `fade-in`, `scale-in`, `slide-left`, `slide-right`.
    #[prop_or(AttrValue::Static("fade-up"))]
    pub anim: AttrValue,
    /// Delay in milliseconds (used for staggered lists).
    #[prop_or_default]
    pub delay: u32,
    #[prop_or_default]
    pub class: Classes,
}

#[component]
pub fn Reveal(props: &RevealProps) -> Html {
    let node = use_node_ref();
    // Prerendered first view: already shown (decided once, when mounted).
    let shown_at_mount = *use_state(dom::settled);
    {
        let node = node.clone();
        use_effect_with((), move |_| {
            let el = node.cast::<Element>();
            if let Some(el) = &el {
                dom::reveal_observe(el);
            }
            move || {
                if let Some(el) = el {
                    dom::reveal_unobserve(&el);
                }
            }
        });
    }
    let style = (props.delay > 0).then(|| format!("--d:{}ms", props.delay));
    html! {
        <div ref={node} class={classes!("reveal", props.anim.to_string(), props.class.clone(), shown_at_mount.then_some("is-visible"))} {style}>
            { props.children.clone() }
        </div>
    }
}
