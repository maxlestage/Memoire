//! FR / EN / ES pills with a sliding indicator.

use yew::prelude::*;

use crate::i18n::{use_lang, Lang};

#[component]
pub fn LangSwitch() -> Html {
    let ctx = use_lang();
    let t = ctx.lang.t();
    let style = format!("--idx:{}", ctx.lang.idx());
    html! {
        <div class="lang-switch" role="group" aria-label={t.language} {style}>
            <span class="lang-indicator" aria-hidden="true"></span>
            { for Lang::ALL.iter().map(|&l| {
                let set = ctx.set.clone();
                let active = l == ctx.lang;
                html! {
                    <button
                        type="button"
                        class={classes!("lang-btn", active.then_some("active"))}
                        aria-pressed={if active { "true" } else { "false" }}
                        lang={l.code()}
                        title={l.native_name()}
                        onclick={Callback::from(move |_| set.emit(l))}
                    >
                        { l.short() }
                    </button>
                }
            }) }
        </div>
    }
}
