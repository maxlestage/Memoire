//! Root component: context providers (language, theme), router and page shell.

use yew::prelude::*;
use yew_router::prelude::*;

use crate::components::{
    back_to_top::BackToTop, background::Background, cursor::CursorFollower,
    effects::use_pointer_effects, footer::Footer, navbar::Navbar,
};
use crate::docs;
use crate::dom;
use crate::i18n::{Lang, LangContext};
use crate::pages::{
    docs_page::DocsPage, features::FeaturesPage, home::HomePage, not_found::NotFound,
    performance::PerformancePage, playground::PlaygroundPage,
};
use crate::theme::{Theme, ThemeContext};

#[derive(Clone, Routable, PartialEq, Debug)]
pub enum Route {
    #[at("/")]
    Home,
    #[at("/features")]
    Features,
    #[at("/performance")]
    Performance,
    #[at("/playground")]
    Playground,
    #[at("/docs")]
    DocsIndex,
    #[at("/docs/:slug")]
    Doc { slug: String },
    #[not_found]
    #[at("/404")]
    NotFound,
}

impl Route {
    /// Key used for the page transition: docs pages share one key so the
    /// sidebar stays mounted while the article changes.
    fn transition_key(&self) -> &'static str {
        match self {
            Route::Home => "home",
            Route::Features => "features",
            Route::Performance => "performance",
            Route::Playground => "playground",
            Route::DocsIndex | Route::Doc { .. } => "docs",
            Route::NotFound => "404",
        }
    }
}

fn switch(route: Route) -> Html {
    match route {
        Route::Home => html! { <HomePage /> },
        Route::Features => html! { <FeaturesPage /> },
        Route::Performance => html! { <PerformancePage /> },
        Route::Playground => html! { <PlaygroundPage /> },
        Route::DocsIndex => html! { <DocsPage slug={AttrValue::Static(docs::FIRST_SLUG)} /> },
        Route::Doc { slug } => html! { <DocsPage slug={AttrValue::from(slug)} /> },
        Route::NotFound => html! { <NotFound /> },
    }
}

#[component]
pub fn App() -> Html {
    let lang = use_state(Lang::detect);
    let theme = use_state(Theme::detect);
    let theme_initialised = use_mut_ref(|| false);

    use_effect_with(*lang, |lang| lang.apply());
    {
        let first = theme_initialised.clone();
        use_effect_with(*theme, move |theme| {
            // Don't persist the auto-detected theme: keep following the OS until the user picks.
            let persist = *first.borrow();
            *first.borrow_mut() = true;
            theme.apply(persist);
        });
    }

    let set_lang = {
        let setter = lang.setter();
        use_callback((), move |l: Lang, _| setter.set(l))
    };
    let toggle_theme = {
        let theme = theme.clone();
        use_callback(*theme, move |_: (), current| theme.set(current.toggled()))
    };

    let lang_ctx = LangContext {
        lang: *lang,
        set: set_lang,
    };
    let theme_ctx = ThemeContext {
        theme: *theme,
        toggle: toggle_theme,
    };

    // Served from a sub-path (GitHub Pages): every route lives under it.
    let basename = Some(dom::base_path()).filter(|p| !p.is_empty()).map(AttrValue::Static);

    // The live app now shows what the prerendered HTML showed: drop the HTML.
    use_effect_with((), |_| dom::remove_prerendered());

    html! {
        <ContextProvider<LangContext> context={lang_ctx}>
            <ContextProvider<ThemeContext> context={theme_ctx}>
                <BrowserRouter {basename}>
                    <Shell />
                </BrowserRouter>
            </ContextProvider<ThemeContext>>
        </ContextProvider<LangContext>>
    }
}

pub fn page_title(route: &Route, lang: Lang) -> String {
    let t = lang.t();
    let page = match route {
        Route::Home => return format!("Memoire ORM — {}", t.footer_tagline),
        Route::Features => t.nav_features,
        Route::Performance => t.nav_performance,
        Route::Playground => t.nav_playground,
        Route::DocsIndex => docs::title(docs::FIRST_SLUG, lang),
        Route::Doc { slug } => match docs::title(slug, lang) {
            "" => t.docs_not_found_title,
            title => title,
        },
        Route::NotFound => t.nf_title,
    };
    format!("{page} · Memoire ORM")
}

/// The app as the build-time prerenderer sees it: a fixed address and
/// language, no browser.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Properties, PartialEq)]
pub struct StaticAppProps {
    /// Route path, without the base path (`/docs/models`).
    pub path: AttrValue,
    pub lang: Lang,
}

#[cfg(not(target_arch = "wasm32"))]
#[component]
pub fn StaticApp(props: &StaticAppProps) -> Html {
    let history = yew_router::history::AnyHistory::from(yew_router::history::MemoryHistory::new());
    yew_router::history::History::push(&history, format!("{}{}", dom::base_path(), props.path));
    let basename = Some(dom::base_path()).filter(|p| !p.is_empty()).map(AttrValue::Static);
    let lang_ctx = LangContext { lang: props.lang, set: Callback::noop() };
    let theme_ctx = ThemeContext { theme: Theme::Dark, toggle: Callback::noop() };
    html! {
        <ContextProvider<LangContext> context={lang_ctx}>
            <ContextProvider<ThemeContext> context={theme_ctx}>
                <Router {history} {basename}>
                    <Shell />
                </Router>
            </ContextProvider<ThemeContext>>
        </ContextProvider<LangContext>>
    }
}

/// Layout shared by every page: background, navbar, animated page area, footer.
#[component]
fn Shell() -> Html {
    let route = use_route::<Route>().unwrap_or(Route::NotFound);

    // Leaving the prerendered first page: entrance animations are back. Done
    // while rendering, so the new page's components already see it.
    let first_route = use_mut_ref(|| route.clone());
    if *first_route.borrow() != route {
        dom::unsettle();
    }
    let location = use_location();
    let ctx = crate::i18n::use_lang();
    let t = ctx.lang.t();

    use_pointer_effects();

    // Scroll back to the top when the path changes (not for in-page #anchors).
    let path = location.as_ref().map(|l| l.path().to_string()).unwrap_or_default();
    use_effect_with(path, |_| {
        let hash = dom::window().location().hash().unwrap_or_default();
        if hash.len() <= 1 {
            dom::scroll_to_top(false);
        }
    });

    use_effect_with((route.clone(), ctx.lang), |(route, lang)| {
        dom::set_title(&page_title(route, *lang));
        dom::set_meta("description", lang.t().meta_description);
    });

    let key = route.transition_key();
    html! {
        <>
            <a class="skip-link" href="#main">{ t.skip_to_content }</a>
            <Background />
            <CursorFollower />
            <Navbar />
            <main id="main" class="main" tabindex="-1">
                <div class="page" key={key}>
                    { switch(route) }
                </div>
            </main>
            <Footer />
            <BackToTop />
        </>
    }
}
