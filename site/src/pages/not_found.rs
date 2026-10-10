//! 404 page.

use yew::prelude::*;
use yew_router::prelude::*;

use crate::app::Route;
use crate::components::icons::Icon;
use crate::i18n::use_lang;

#[component]
pub fn NotFound() -> Html {
    let t = use_lang().lang.t();
    html! {
        <section class="not-found container">
            <div class="nf-code glitch" data-text="404" aria-hidden="true">{ "404" }</div>
            <h1 class="hero-anim">{ t.nf_title }</h1>
            <p class="hero-anim" style="--d:100ms">{ t.nf_text }</p>
            <div class="hero-ctas hero-anim" style="--d:200ms">
                <Link<Route> to={Route::Home} classes={classes!("btn", "btn-primary", "magnetic")}>
                    { Icon::ArrowLeft.html() }<span>{ t.nf_home }</span>
                </Link<Route>>
                <Link<Route> to={Route::DocsIndex} classes={classes!("btn", "btn-ghost", "magnetic")}>
                    { Icon::Book.html() }<span>{ t.nf_docs }</span>
                </Link<Route>>
            </div>
        </section>
    }
}
