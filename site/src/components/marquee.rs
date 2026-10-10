//! Infinite horizontal marquee (pure CSS animation, content duplicated once).

use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct MarqueeProps {
    pub items: &'static [&'static str],
    #[prop_or_default]
    pub reverse: bool,
}

#[component]
pub fn Marquee(props: &MarqueeProps) -> Html {
    let group = |hidden: bool| {
        html! {
            <ul class="marquee-group" aria-hidden={hidden.then_some("true")}>
                { for props.items.iter().map(|item| html! {
                    <li class="marquee-item"><span class="marquee-star" aria-hidden="true">{ "✦" }</span>{ *item }</li>
                }) }
            </ul>
        }
    };
    html! {
        <div class={classes!("marquee", props.reverse.then_some("reverse"))}>
            <div class="marquee-track">
                { group(false) }
                { group(true) }
            </div>
        </div>
    }
}
