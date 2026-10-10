//! Build-time prerendering (native build): `cargo run -- prerender <dist>`.
//!
//! Renders every page of the site, in each language, with Yew's server
//! renderer and writes it into a copy of `<dist>/index.html`:
//! `features.html` and `features/index.html`, `docs/<slug>.html` ..., and
//! `404.html` for any other address. Visitors see the page as soon as the HTML
//! arrives; the WebAssembly app then takes over (see [`crate::dom::settled`]).
//! Run after `trunk build`, with the same `MEMOIRE_BASE_PATH`.

use std::fs;
use std::path::{Path, PathBuf};

use yew::LocalServerRenderer;
use yew_router::Routable;

use crate::app::{page_title, Route, StaticApp, StaticAppProps};
use crate::docs;
use crate::i18n::Lang;

const START: &str = "<!--prerender";
const END: &str = "<!--/prerender-->";

pub fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dist = match args.as_slice() {
        [cmd, dist] if cmd == "prerender" => PathBuf::from(dist),
        _ => {
            eprintln!("usage: memoire-site prerender <dist dir>   (run after `trunk build`)");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(&dist) {
        eprintln!("prerender: {e}");
        std::process::exit(1);
    }
}

fn run(dist: &Path) -> Result<(), String> {
    let docs_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs");
    for (_, page) in docs::all_pages() {
        for lang in Lang::ALL {
            let path = docs_dir.join(lang.code()).join(format!("{}.md", page.slug));
            let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            docs::insert_source(page.slug, lang, &text);
        }
    }

    let template_path = dist.join("index.html");
    let template = fs::read_to_string(&template_path).map_err(|e| format!("{}: {e}", template_path.display()))?;
    if !template.contains(START) || !template.contains(END) {
        return Err(format!(
            "{} has no {START} … {END} markers (already prerendered? run `trunk build` first)",
            template_path.display()
        ));
    }

    // (route path, files to write). index.html, the template, is written last
    // so a failed run can simply be started again.
    let mut pages: Vec<(String, Vec<String>)> = Vec::new();
    for route in ["features", "performance", "playground", "docs"] {
        pages.push((format!("/{route}"), vec![format!("{route}.html"), format!("{route}/index.html")]));
    }
    for (_, page) in docs::all_pages() {
        let slug = page.slug;
        pages.push((format!("/docs/{slug}"), vec![format!("docs/{slug}.html"), format!("docs/{slug}/index.html")]));
    }
    pages.push(("/404".into(), vec!["404.html".into()]));
    pages.push(("/".into(), vec!["index.html".into()]));

    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(|e| e.to_string())?;
    let local = tokio::task::LocalSet::new();
    for (path, files) in &pages {
        let mut views = String::new();
        for lang in Lang::ALL {
            let props = StaticAppProps { path: path.clone().into(), lang };
            let html = local.block_on(
                &runtime,
                LocalServerRenderer::<StaticApp>::with_props(props).hydratable(false).render(),
            );
            views.push_str(&format!("<div class=\"prerendered-view\" data-lang=\"{}\">{html}</div>", lang.code()));
        }
        let route = Route::recognize(path).unwrap_or(Route::NotFound);
        // The docs page's markdown, so the app shows it at once instead of
        // downloading it again (see `docs::adopt_embedded_sources`).
        let slug = match &route {
            Route::Doc { slug } => Some(slug.as_str()),
            Route::DocsIndex => Some(docs::FIRST_SLUG),
            _ => None,
        };
        if let Some((_, page)) = slug.and_then(docs::find) {
            for lang in Lang::ALL {
                let source = docs::source(page.slug, lang).unwrap_or_default().replace("</script", "<\\/script");
                views.push_str(&format!(
                    "<script type=\"text/markdown\" class=\"doc-source\" data-slug=\"{}\" data-lang=\"{}\">{source}</script>",
                    page.slug,
                    lang.code()
                ));
            }
        }
        let page = fill(&template, &views, &page_title(&route, Lang::En));
        for file in files {
            let target = dist.join(file);
            if let Some(dir) = target.parent() {
                fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            fs::write(&target, &page).map_err(|e| format!("{}: {e}", target.display()))?;
        }
    }
    println!("prerendered {} pages × {} languages into {}", pages.len(), Lang::ALL.len(), dist.display());
    Ok(())
}

/// The template with the prerendered views in place of the preloader, the
/// page's title, and `<html class="settled">`.
fn fill(template: &str, views: &str, title: &str) -> String {
    let start = template.find(START).expect("checked");
    let end = template.find(END).expect("checked") + END.len();
    let mut page = format!("{}{views}{}", &template[..start], &template[end..]);
    if let (Some(open), Some(close)) = (page.find("<title>"), page.find("</title>")) {
        let escaped = title.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        page.replace_range(open + "<title>".len()..close, &escaped);
    }
    page.replacen("<html ", "<html class=\"settled\" ", 1)
}
