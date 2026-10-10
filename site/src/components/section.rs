//! Section header: eyebrow, gradient title and subtitle, revealed on scroll.

use yew::prelude::*;

use super::reveal::Reveal;

#[derive(Properties, PartialEq)]
pub struct SectionHeadProps {
    pub eyebrow: AttrValue,
    pub title: AttrValue,
    #[prop_or_default]
    pub subtitle: Option<AttrValue>,
    #[prop_or(true)]
    pub center: bool,
}

#[component]
pub fn SectionHead(props: &SectionHeadProps) -> Html {
    html! {
        <Reveal class={classes!("section-head", props.center.then_some("center"))}>
            <span class="eyebrow">{ props.eyebrow.clone() }</span>
            <h2 class="section-title">{ props.title.clone() }</h2>
            if let Some(sub) = props.subtitle.clone() {
                <p class="section-subtitle">{ sub }</p>
            }
        </Reveal>
    }
}
