//! `memoire-site`: the web server (build the browser bundle first, see CONTRIBUTING.md).
//!
//! `memoire-site export <dir>`: the whole site as static files in `<dir>`, for GitHub Pages
//! or any static host. `BASE_PATH` (e.g. `/Memoire`) prefixes every link, `SITE_URL`
//! (e.g. `https://maxlestage.github.io/Memoire`) makes the absolute URLs.

#[cfg(not(target_arch = "wasm32"))]
#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("export") => {
            let dir = args.next().unwrap_or_else(|| "dist".into());
            let base = std::env::var("BASE_PATH").unwrap_or_default();
            let site_url = std::env::var("SITE_URL").ok();
            match memoire_site::server::export::export(std::path::Path::new(&dir), &base, site_url).await {
                Ok(files) => println!("{files} files written to {dir}"),
                Err(error) => {
                    eprintln!("export failed: {error}");
                    std::process::exit(1);
                }
            }
        }
        Some(other) => {
            eprintln!("unknown command {other:?}: run without arguments to serve, or `export <dir>`");
            std::process::exit(2);
        }
        None => memoire_site::server::run().await,
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {}
