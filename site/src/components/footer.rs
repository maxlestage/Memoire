//! Multi-column footer with an animated gradient top border.

use yew::prelude::*;
use yew_router::prelude::*;

use super::icons::Icon;
use super::lang_switch::LangSwitch;
use super::logo::Logo;
use crate::app::Route;
use crate::docs::{self, REPO_URL};
use crate::dom;
use crate::i18n::use_lang;

#[component]
pub fn Footer() -> Html {
    let ctx = use_lang();
    let t = ctx.lang.t();
    let doc = |slug: &'static str, label: &'static str| {
        html! {
            <li><Link<Route> to={Route::Doc { slug: slug.into() }}>{ label }</Link<Route>></li>
        }
    };
    let ext = |href: String, label: &'static str| {
        html! {
            <li>
                <a href={href} target="_blank" rel="noopener noreferrer">
                    { label }<span class="ext">{ Icon::External.html() }</span>
                </a>
            </li>
        }
    };
    html! {
        <footer class="footer">
            <div class="footer-border" aria-hidden="true"></div>
            <div class="container footer-grid">
                <div class="footer-brand">
                    <Logo />
                    <p class="footer-tagline">{ t.footer_tagline }</p>
                    <p class="footer-mission">{ t.footer_mission }</p>
                    <div class="footer-social">
                        <a class="icon-btn" href={REPO_URL} target="_blank" rel="noopener noreferrer" aria-label="GitHub">{ Icon::Github.html() }</a>
                        <a class="icon-btn" href="https://crates.io/crates/memoire-orm" target="_blank" rel="noopener noreferrer" aria-label="crates.io">{ Icon::Crate.html() }</a>
                        <a class="icon-btn" href="https://docs.rs/memoire-orm" target="_blank" rel="noopener noreferrer" aria-label="docs.rs">{ Icon::Book.html() }</a>
                    </div>
                </div>
                <div class="footer-col">
                    <h3>{ t.footer_product }</h3>
                    <ul>
                        <li><Link<Route> to={Route::Features}>{ t.nav_features }</Link<Route>></li>
                        <li><Link<Route> to={Route::Performance}>{ t.nav_performance }</Link<Route>></li>
                        <li><Link<Route> to={Route::Playground}>{ t.nav_playground }</Link<Route>></li>
                        <li><Link<Route> to={Route::DocsIndex}>{ t.nav_docs }</Link<Route>></li>
                    </ul>
                </div>
                <div class="footer-col">
                    <h3>{ t.footer_docs }</h3>
                    <ul>
                        { doc("quick-start", docs::title("quick-start", ctx.lang)) }
                        { doc("models", docs::title("models", ctx.lang)) }
                        { doc("queries", docs::title("queries", ctx.lang)) }
                        { doc("deploy-heroku-mobile", t.footer_deploy) }
                    </ul>
                </div>
                <div class="footer-col">
                    <h3>{ t.footer_community }</h3>
                    <ul>
                        { ext(REPO_URL.to_string(), "GitHub") }
                        { ext("https://crates.io/crates/memoire-orm".to_string(), "crates.io") }
                        { ext("https://docs.rs/memoire-orm".to_string(), "docs.rs") }
                        { ext(format!("{REPO_URL}/issues"), t.footer_issues) }
                    </ul>
                </div>
                <div class="footer-col footer-lang">
                    <h3 class="with-icon">{ Icon::Globe.html() }{ t.language }</h3>
                    <LangSwitch />
                </div>
            </div>
            <div class="container footer-bottom">
                <span>{ t.footer_rights }</span>
                <span class="footer-built">{ t.footer_built }</span>
                <button type="button" class="footer-top" onclick={Callback::from(|_| dom::scroll_to_top(true))}>
                    { t.back_to_top }{ Icon::ArrowUp.html() }
                </button>
            </div>
        </footer>
    }
}
