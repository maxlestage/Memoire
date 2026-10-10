//! Glass card with 3D tilt + spotlight glow following the mouse.
//! The pointer tracking itself lives in [`super::effects`] (one global listener).

use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct TiltCardProps {
    #[prop_or_default]
    pub children: Html,
    #[prop_or_default]
    pub class: Classes,
}

#[component]
pub fn TiltCard(props: &TiltCardProps) -> Html {
    html! {
        <div class={classes!("tilt", "spotlight", "card", props.class.clone())}>
            <div class="tilt-inner">{ props.children.clone() }</div>
        </div>
    }
}
