//! Fixed navbar: transparent at the top, glass once scrolled, with a scroll
//! progress bar and an animated mobile menu.

use gloo::events::EventListener;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, KeyboardEvent};
use yew::prelude::*;
use yew_router::prelude::*;

use super::back_to_top::scroll_progress;
use super::icons::Icon;
use super::lang_switch::LangSwitch;
use super::logo::Logo;
use super::theme_toggle::ThemeToggle;
use crate::app::Route;
use crate::docs::REPO_URL;
use crate::dom;
use crate::i18n::use_lang;

#[component]
pub fn Navbar() -> Html {
    let t = use_lang().lang.t();
    let route = use_route::<Route>();
    let scrolled = use_state_eq(|| false);
    let open = use_state_eq(|| false);
    let progress = use_node_ref();

    // Scroll state + progress bar (set directly on the element, no re-render).
    {
        let (scrolled, progress) = (scrolled.clone(), progress.clone());
        use_effect_with((), move |_| {
            let update = move || {
                scrolled.set(dom::scroll_y() > 12.0);
                if let Some(el) = progress.cast::<HtmlElement>() {
                    dom::set_css_var(&el, "--p", &format!("{:.4}", scroll_progress()));
                }
            };
            update();
            let listener = EventListener::new(&dom::window(), "scroll", move |_| update());
            move || drop(listener)
        });
    }

    // Close the mobile menu on navigation.
    {
        let open = open.clone();
        use_effect_with(route.clone(), move |_| open.set(false));
    }

    // Lock page scroll while the menu is open; Escape closes it.
    {
        let open_handle = open.clone();
        use_effect_with(*open, move |&is_open| {
            dom::set_body_class("menu-open", is_open);
            let listener = is_open.then(|| {
                EventListener::new(&dom::document(), "keydown", move |e| {
                    if e.dyn_ref::<KeyboardEvent>().is_some_and(|k| k.key() == "Escape") {
                        open_handle.set(false);
                    }
                })
            });
            move || {
                drop(listener);
                dom::set_body_class("menu-open", false);
            }
        });
    }

    let is_active = |r: &Route| match (route.as_ref(), r) {
        (Some(Route::Doc { .. }) | Some(Route::DocsIndex), Route::DocsIndex) => true,
        (Some(cur), r) => cur == r,
        _ => false,
    };
    let links = [
        (Route::Features, t.nav_features),
        (Route::Performance, t.nav_performance),
        (Route::Playground, t.nav_playground),
        (Route::DocsIndex, t.nav_docs),
    ];
    let toggle = {
        let open = open.clone();
        Callback::from(move |_| open.set(!*open))
    };

    html! {
        <header class={classes!("navbar", scrolled.then_some("scrolled"), open.then_some("menu-is-open"))}>
            <div class="nav-inner container">
                <Logo />
                <nav class="nav-links" aria-label="Main">
                    { for links.iter().map(|(r, label)| html! {
                        <Link<Route> to={r.clone()} classes={classes!("nav-link", is_active(r).then_some("active"))}>
                            { *label }
                        </Link<Route>>
                    }) }
                </nav>
                <div class="nav-actions">
                    <LangSwitch />
                    <ThemeToggle />
                    <a class="icon-btn nav-github" href={REPO_URL} target="_blank" rel="noopener noreferrer" aria-label="GitHub" title="GitHub">
                        { Icon::Github.html() }
                    </a>
                    <Link<Route> to={Route::Doc { slug: "quick-start".into() }} classes={classes!("btn", "btn-primary", "btn-sm", "magnetic", "nav-cta")}>
                        <span>{ t.nav_get_started }</span>
                    </Link<Route>>
                    <button
                        type="button"
                        class={classes!("burger", open.then_some("is-open"))}
                        aria-label={if *open { t.nav_close_menu } else { t.nav_open_menu }}
                        aria-expanded={if *open { "true" } else { "false" }}
                        aria-controls="mobile-menu"
                        onclick={toggle}
                    >
                        <span></span><span></span><span></span>
                    </button>
                </div>
            </div>
            <div ref={progress} class="scroll-progress" aria-hidden="true"></div>

            <div id="mobile-menu" class={classes!("mobile-menu", open.then_some("is-open"))} inert={!*open}>
                <nav class="mobile-links" aria-label="Mobile">
                    <Link<Route> to={Route::Home} classes={classes!("mobile-link", is_active(&Route::Home).then_some("active"))}>
                        <span class="ml-index">{ "01" }</span>{ t.nav_home }
                    </Link<Route>>
                    { for links.iter().enumerate().map(|(i, (r, label))| html! {
                        <Link<Route> to={r.clone()} classes={classes!("mobile-link", is_active(r).then_some("active"))}>
                            <span class="ml-index">{ format!("{:02}", i + 2) }</span>{ *label }
                        </Link<Route>>
                    }) }
                </nav>
                <div class="mobile-extras">
                    <LangSwitch />
                    <ThemeToggle />
                    <a class="icon-btn" href={REPO_URL} target="_blank" rel="noopener noreferrer" aria-label="GitHub">{ Icon::Github.html() }</a>
                </div>
                <Link<Route> to={Route::Doc { slug: "quick-start".into() }} classes={classes!("btn", "btn-primary", "mobile-cta")}>
                    <span>{ t.nav_get_started }</span>{ Icon::ArrowRight.html() }
                </Link<Route>>
            </div>
        </header>
    }
}
