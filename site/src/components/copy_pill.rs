//! `cargo add memoire-orm` pill: click to copy, with a "Copied!" tooltip.

use gloo::timers::callback::Timeout;
use yew::prelude::*;

use super::icons::Icon;
use crate::dom;
use crate::i18n::use_lang;

#[derive(Properties, PartialEq)]
pub struct CopyPillProps {
    pub text: AttrValue,
}

#[component]
pub fn CopyPill(props: &CopyPillProps) -> Html {
    let t = use_lang().lang.t();
    let copied = use_state(|| false);
    let timer = use_mut_ref(|| None::<Timeout>);
    let onclick = {
        let (copied, timer, text) = (copied.clone(), timer.clone(), props.text.clone());
        Callback::from(move |_| {
            dom::copy_to_clipboard(&text);
            copied.set(true);
            let copied = copied.clone();
            *timer.borrow_mut() = Some(Timeout::new(1800, move || copied.set(false)));
        })
    };
    html! {
        <button
            type="button"
            class={classes!("copy-pill", "magnetic", copied.then_some("is-copied"))}
            aria-label={t.copy_install}
            title={t.copy_install}
            {onclick}
        >
            <span class="copy-prompt" aria-hidden="true">{ "$" }</span>
            <code>{ props.text.clone() }</code>
            <span class="copy-icon">
                { if *copied { Icon::Check.html() } else { Icon::Copy.html() } }
            </span>
            <span class="copy-tip" role="status">{ if *copied { t.copied } else { "" } }</span>
        </button>
    }
}
