//! The Memoire ORM logo: a memory chip with a stylised "M".

use yew::prelude::*;
use yew_router::prelude::*;

use crate::app::Route;

pub const LOGO_SVG: &str = r##"<svg class="logo-mark" viewBox="0 0 64 64" width="34" height="34" aria-hidden="true"><defs><linearGradient id="mlg-a" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#f74c00"/><stop offset=".55" stop-color="#ffb347"/><stop offset="1" stop-color="#7c5cff"/></linearGradient><linearGradient id="mlg-b" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#7c5cff"/><stop offset="1" stop-color="#22d3ee"/></linearGradient></defs><g class="logo-pins" stroke="url(#mlg-b)" stroke-width="3" stroke-linecap="round"><path d="M21 4v6M32 4v6M43 4v6M21 54v6M32 54v6M43 54v6M4 21h6M4 32h6M4 43h6M54 21h6M54 32h6M54 43h6"/></g><rect x="10" y="10" width="44" height="44" rx="12" fill="#0d0d18" stroke="url(#mlg-a)" stroke-width="3"/><path class="logo-m" d="M20.5 42V22.5L32 34l11.5-11.5V42" fill="none" stroke="url(#mlg-a)" stroke-width="4.6" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

#[derive(Properties, PartialEq)]
pub struct LogoProps {
    #[prop_or(true)]
    pub with_text: bool,
}

/// Logo linking to the home page.
#[component]
pub fn Logo(props: &LogoProps) -> Html {
    html! {
        <Link<Route> to={Route::Home} classes={classes!("logo")}>
            { Html::from_html_unchecked(AttrValue::Static(LOGO_SVG)) }
            if props.with_text {
                <span class="logo-text">{ "Memoire" }<span class="logo-accent">{ " ORM" }</span></span>
            }
        </Link<Route>>
    }
}
