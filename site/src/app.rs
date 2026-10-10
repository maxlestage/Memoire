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

    html! {
        <ContextProvider<LangContext> context={lang_ctx}>
            <ContextProvider<ThemeContext> context={theme_ctx}>
                <BrowserRouter>
                    <Shell />
                </BrowserRouter>
            </ContextProvider<ThemeContext>>
        </ContextProvider<LangContext>>
    }
}

fn page_title(route: &Route, lang: Lang) -> String {
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

/// Layout shared by every page: background, navbar, animated page area, footer.
#[component]
fn Shell() -> Html {
    let route = use_route::<Route>().unwrap_or(Route::NotFound);
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
