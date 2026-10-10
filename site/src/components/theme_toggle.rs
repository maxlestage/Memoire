//! Sun / moon theme toggle.

use yew::prelude::*;

use super::icons::Icon;
use crate::i18n::use_lang;
use crate::theme::{use_theme, Theme};

#[component]
pub fn ThemeToggle() -> Html {
    let theme = use_theme();
    let t = use_lang().lang.t();
    let onclick = {
        let toggle = theme.toggle.clone();
        Callback::from(move |_| toggle.emit(()))
    };
    html! {
        <button
            type="button"
            class={classes!("theme-toggle", (theme.theme == Theme::Light).then_some("is-light"))}
            aria-label={t.theme_toggle}
            title={t.theme_toggle}
            {onclick}
        >
            <span class="tt-icon tt-sun">{ Icon::Sun.html() }</span>
            <span class="tt-icon tt-moon">{ Icon::Moon.html() }</span>
        </button>
    }
}
