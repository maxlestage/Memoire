# Deploy to Heroku from your phone

You can put the Memoire website online, and keep it up to date, using only a phone: no computer, no terminal. This page walks through three ways to do it, explains what happens behind the scenes, and ends with deploying your own Rust application that uses Memoire and Heroku Postgres.

## Before you start

You need:

- a **GitHub account** and your own copy of the repository: open `https://maxlestage.github.io/Memoire/` and tap **Fork** (only needed for methods 2 and 3);
- a **Heroku account** (sign up at `https://signup.heroku.com` in your phone's browser);
- a **payment method** on the Heroku account. Heroku has had no free plan since November 2022: the website runs on a *Basic* dyno (about 7 USD per month at the time of writing) and Heroku will not start apps on an unverified account. Check `https://www.heroku.com/pricing` for current prices.

All the Heroku steps happen in the Heroku dashboard (`https://dashboard.heroku.com`) in your mobile browser. The GitHub steps work in the GitHub mobile app, except managing secrets, which needs the GitHub website in a mobile browser.

> **Tip** If a menu or tab is missing on a small screen, use your browser's "Request desktop site" option. Turning the phone sideways also helps with wide pages such as repository settings.

## Which method should I choose?

| Method | Effort | Updates after the first deploy | Where the site is built |
|---|---|---|---|
| 1. "Deploy to Heroku" button | Lowest: a few taps | None: it deploys once | On Heroku, with the Rust buildpack |
| 2. GitHub Actions workflow | Medium: two secrets to add | Automatic on every push to `master`, or one tap in the GitHub app | On Heroku, with the Rust buildpack (or on GitHub as a Docker image, if you choose) |
| 3. Heroku's GitHub integration | Medium: connect GitHub to Heroku | Automatic on every push to `master` | On Heroku, with the Rust buildpack |

A common combination: method 1 for the very first deploy, then method 2 or 3 to keep the site updated. Do not enable methods 2 and 3 at the same time, or every push will be deployed twice.

> **Tip** No Heroku account, or no budget? The website can also be published **for free on GitHub Pages**, by changing a single setting: see the GitHub Pages section below.

## Method 1: the "Deploy to Heroku" button

The repository's README has a purple **Deploy to Heroku** button. It points to:

```text
https://dashboard.heroku.com/new-app
```

1. Open the repository on GitHub (app or browser) and scroll to the top of the README.
2. Tap **Deploy to Heroku**. Your browser opens Heroku; log in if asked.
3. Choose an **App name** (lowercase letters, digits and dashes, unique across Heroku) and a **region** (United States or Europe).
4. Leave the `RUST_LOG` setting at `info`.
5. Tap **Deploy app**.
6. Wait for the build. The first one compiles the whole website from source, which can take 10 minutes or more. You can follow it live on the page.
7. When Heroku shows "Your app was successfully deployed", tap **View** to open your site.

What happens behind the scenes:

- `app.json` describes the app for Heroku: its name and description, the **heroku-26** stack with the **emk/rust** buildpack ([heroku-buildpack-rust](https://elements.heroku.com/buildpacks/emk/heroku-buildpack-rust)), one `web` dyno of size *Basic*, and the `RUST_LOG` setting.
- `RustConfig` configures the buildpack: it installs the Rust version pinned in that file (the same as in the `Dockerfile`), skips its own build (`RUST_SKIP_BUILD=1`) and runs `heroku-build.sh` instead.
- `heroku-build.sh` compiles the web server of the website, written in Rust with [Active](https://maxlestage.github.io/Active/), in `site/` with `cargo build --release --locked`. The server renders every page and embeds the documentation. It then builds the browser bundle into `site/pkg`: the interactive parts and the playground's in-memory engine, compiled to WebAssembly with `wasm-bindgen`.
- `Procfile` tells Heroku how to start the `web` process: `web: SITE_DIR=site exec site/target/release/memoire-site`.

The `Dockerfile` and `heroku.yml` are still there: they build the same website as a Docker image, for Heroku's **container** stack (method 2 with `HEROKU_BUILD` set to `docker`) or any other Docker host.

> **Note** The button deploys the repository as it is at that moment. To get later changes, use method 2 or 3, or tap the button again to create a fresh app.

## Method 2: GitHub Actions (automatic, and one tap from the GitHub app)

The repository contains a workflow, `.github/workflows/deploy-heroku.yml`, named **Deploy to Heroku**. It runs on every push to `master` and can also be started by hand. It needs two repository secrets.

### Step 1: get a Heroku API key

1. In your mobile browser, open `https://dashboard.heroku.com/account`.
2. Scroll to **API Key** and tap **Reveal**, then copy the key.

For a token dedicated to GitHub (recommended, because you can revoke it on its own):

1. Open `https://dashboard.heroku.com/account/applications`.
2. Under **Authorizations**, tap **Create authorization**.
3. Enter a description such as "GitHub Actions deploy", leave the expiry empty (or pick one), and tap **Create**.
4. Copy the token that is displayed. Heroku shows it only once.

### Step 2: add the secrets to your repository

Secrets cannot be edited in the GitHub app, so use the website in your mobile browser:

1. Open your fork: `https://github.com/<your-name>/Memoire`.
2. Tap **Settings**, then **Secrets and variables**, then **Actions**.
3. Tap **New repository secret**. Name: `HEROKU_API_KEY`. Value: the key or token from step 1. Tap **Add secret**.
4. Tap **New repository secret** again. Name: `HEROKU_APP_NAME`. Value: the name you want for your app, for example `memoire-docs-yourname`. Tap **Add secret**.

`HEROKU_APP_NAME` may also be created as a repository **variable** (the **Variables** tab of the same page), since it is not secret. If the app does not exist yet, the workflow creates it for you.

By default, Heroku builds the website itself with the Rust buildpack. To build a Docker image on GitHub's machines instead, add a repository **variable** (on the same **Variables** tab) named `HEROKU_BUILD` with the value `docker`. The value `buildpack`, or no variable at all, keeps the default.

### Step 3: deploy

Any push to `master` now deploys automatically. To deploy right away from the **GitHub mobile app**:

1. Open your repository in the app.
2. Tap **Actions** (in the repository's menu; you may need to tap **More** first).
3. Tap the **Deploy to Heroku** workflow.
4. Tap **Run workflow**, keep the branch `master`, and confirm with **Run workflow**.
5. Tap the new run to follow its progress. When it finishes, its summary shows "Deployed to" followed by your app's address.

### What the workflow does

1. **Checks the configuration.** If a secret is missing, it stops politely with a notice ("Heroku deployment skipped") instead of failing.
2. **Prepares the app.** It creates the Heroku app if it does not exist yet, on the **heroku-26** stack, and sets the **emk/rust** buildpack. An app on the **container** stack is moved to heroku-26.
3. **Builds on Heroku.** It uploads the source of the commit to Heroku, which builds it with the buildpack (`RustConfig`, then `heroku-build.sh`) and **releases** it as the new version of the `web` process. The build log is shown in the run; the buildpack keeps the compiled dependencies in its cache, so later deploys are faster.
4. **Checks the health** of the site: it calls `/healthz` on your app for up to five minutes and reports the result.

With `HEROKU_BUILD` set to `docker`, step 2 puts the app on the **container** stack instead, and step 3 builds the Docker image from the root `Dockerfile` on GitHub's machines (reusing a build cache between runs), pushes it to Heroku's container registry (`registry.heroku.com`) and releases it.

Only one deployment runs at a time; a new push waits for the current deployment to finish.

## Method 3: Heroku's GitHub integration

Heroku can watch your repository and build each new commit itself.

1. Create the app first with method 1 or method 2 (both set up the **heroku-26** stack and the **emk/rust** buildpack this method needs). For an app created otherwise, open **Settings → Buildpacks → Add buildpack**, enter `emk/rust` and tap **Save changes**; this needs an app that is not on the **container** stack.
2. In the Heroku dashboard, open your app and tap the **Deploy** tab.
3. Under **Deployment method**, tap **GitHub**, then **Connect to GitHub**, and authorise Heroku.
4. Search for your repository (`Memoire`), then tap **Connect**.
5. Under **Automatic deploys**, choose the `master` branch and tap **Enable Automatic Deploys**. Optionally tick "Wait for CI to pass before deploy".
6. To deploy immediately, scroll to **Manual deploy** and tap **Deploy Branch**.

Heroku then builds the website with the buildpack (`RustConfig`, `heroku-build.sh`, `Procfile`) on every push to `master`.

## From a computer: the Heroku CLI

Without GitHub Actions, the [Heroku CLI](https://devcenter.heroku.com/articles/heroku-cli) deploys the repository with a plain `git push`. In your copy of the repository:

```bash
heroku create my-memoire --stack heroku-26 --buildpack emk/rust
git push heroku master
```

For an app that already exists (on the container stack, for example), set its stack and its buildpack first:

```bash
heroku git:remote -a my-memoire
heroku stack:set heroku-26
heroku buildpacks:set emk/rust
git push heroku master
```

## GitHub Pages: the free option

The website's server can also write every page, in every language, to static files, so GitHub Pages can host it too, for free, at `https://<your-user>.github.io/Memoire/` (for this repository: `https://maxlestage.github.io/Memoire/`). No Heroku account and no secret: the `.github/workflows/pages.yml` workflow builds and publishes it.

Turn it on once:

1. In your phone's browser, open your repository on `https://github.com`, then **Settings → Pages**.
2. Under **Build and deployment → Source**, choose **GitHub Actions**.
3. In the GitHub app, open **Actions → Deploy to GitHub Pages → Run workflow**. Two or three minutes later the site is online; its address is shown on the run's page.

After that, every push to `master` that changes the website, the docs or the ORM publishes the new version, and so does every pull request merged automatically.

> **Note** On a free GitHub plan, GitHub Pages only works for public repositories.

What the workflow does:

- it skips with the notice "GitHub Pages deployment skipped" as long as step 2 is not done;
- it builds the browser bundle, then runs `memoire-site export`, which writes every page into its own HTML file under `/Memoire/` (`en/docs/models/`, `fr/playground/`...), with the search index, the web app manifest and the service worker: links to any page and refreshes work, the old addresses (`docs/models`...) lead to the visitor's language, and unknown addresses show the site's "page not found";
- it publishes the site, then checks that the home pages, a documentation page, the playground, the manifest and the service worker answer.

GitHub Pages and Heroku are independent: you can use both. The only difference is that GitHub Pages cannot send the extra security headers (such as `Content-Security-Policy`) that the Heroku server adds; the site itself works the same.

> **Tip** To use your own domain, set it under **Settings → Pages → Custom domain**, then run the workflow again: it builds the site for the root of that domain.

## Install the site on your phone

The website is a Progressive Web App, wherever it is hosted (Heroku or GitHub Pages):

- **Android** (Chrome): open the site, tap the **⋮** menu, then **Install app** (or **Add to Home screen**).
- **iPhone and iPad** (Safari): open the site, tap **Share**, then **Add to Home Screen**.

It then opens full screen from its icon, like an app. Once a page has been opened, the site keeps working without network: the home pages, the first documentation page, the playground and every page you have already read are kept on the device. The playground needs no network at all: the ORM runs in the page, compiled to WebAssembly. A new version of the site replaces the saved one at the next visit with network.

## How merging triggers a deployment

Pull requests that target `master` are **merged automatically** as soon as every CI check is green (unless the pull request is a draft, comes from a fork, or has the `no-auto-merge` label). After the merge, the CI workflow explicitly starts the **Deploy to Heroku** and **Deploy to GitHub Pages** workflows, because merges performed by GitHub Actions do not trigger other workflows on their own.

The result: approve or open a pull request from your phone, wait for the checks, and the new version goes online by itself (with method 2). With method 3, Heroku sees the new commit on `master` and builds it.

## Troubleshooting

| Problem | Where to look / what to do |
|---|---|
| Build fails (methods 1 and 3) | Heroku dashboard → your app → **Activity** tab → **View build log** |
| Build fails (method 2) | GitHub app → **Actions** → the failed run → the red step |
| Site does not open, or "Application error" | Heroku dashboard → your app → **More** (top right) → **View logs** |
| Workflow says "Heroku deployment skipped" | `HEROKU_API_KEY` or `HEROKU_APP_NAME` is missing (check the spelling) |
| Workflow says "GitHub Pages deployment skipped" | **Settings → Pages → Source** is not set to **GitHub Actions** |
| `401` or "Invalid credentials" in the workflow | The API key is wrong, expired or revoked: create a new one and update the secret |
| "Name is already taken" | App names are global on Heroku: choose another `HEROKU_APP_NAME` |
| Heroku asks to verify the account | Add a payment method under **Account settings → Billing** |
| "No default language could be detected" | The app has no buildpack: **Settings → Buildpacks → Add buildpack** → `emk/rust` (the app must not be on the container stack), or run method 2 once |
| The build asks for a newer Rust | Raise `VERSION` in `RustConfig`, and the Rust version of the `Dockerfile` with it |

Good to know:

- **Health check:** open `https://<your-app-address>/healthz` in your browser. The server answers `ok` when it is running. Your app's address is shown by the **Open app** button in the dashboard.
- **Port:** Heroku chooses the port and passes it in the `PORT` environment variable; the website's server reads it automatically (8080 is only the default for local runs). Nothing to configure.
- **Costs:** the website uses one *Basic* dyno. You can switch to a cheaper *Eco* dyno (which sleeps when nobody visits) in the **Resources** tab, or scale the dyno to zero there to stop paying for it.

## Deploying your own app with Memoire and Heroku Postgres

The same tools work for your own Rust web application that stores data with Memoire.

### 1. Add a database

1. In the Heroku dashboard, open your app and tap the **Resources** tab.
2. Under **Add-ons**, search for **Heroku Postgres**, pick a plan and submit.
3. Heroku creates the database and adds a `DATABASE_URL` config var to your app (visible in **Settings → Reveal Config Vars**).

### 2. Connect from your code

Heroku Postgres only accepts encrypted connections, so enable the `postgres-tls` feature:

```toml
[dependencies]
memoire-orm = { git = "https://github.com/maxlestage/Memoire", features = ["postgres-tls"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Then read `DATABASE_URL`, run your migrations at startup, and listen on the port Heroku gives you:

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "visits")]
pub struct Visit {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub path: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Heroku sets DATABASE_URL when you add Heroku Postgres.
    let url = std::env::var("DATABASE_URL")?;
    let url = if url.contains('?') { url } else { format!("{url}?sslmode=require") };
    let db = PgPool::connect_with(&url, 10).await?;

    Migrator::new()
        .add(Migration::new(1, "create_visits").create_table::<Visit>())
        .run(&db)
        .await?;

    // Heroku tells your web server which port to use.
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
    println!("ready to listen on 0.0.0.0:{port}");
    // Start your web server here (Vitesse: `app.state(db)` shares the pool with your handlers).
    Ok(())
}
```

Keep the pool small: the smallest Heroku Postgres plans accept only a limited number of connections (check your plan), shared by all your dynos. See [PostgreSQL](postgres) for the TLS options.

### Other databases on Heroku

The other server backends work the same way: add the service, then read its URL from a config var.

| Database | How to get it on Heroku | Memoire feature | Connect with |
|---|---|---|---|
| Redis | **Heroku Key-Value Store** add-on, which sets `REDIS_URL` | `redis-tls` | `RedisDb::connect(&format!("{url}#insecure"))` (its certificate is self-signed; see [Redis](redis)) |
| MySQL / MariaDB | A MySQL add-on from the Elements marketplace, which sets a `mysql://` URL in a config var | `mysql` (or `mysql-tls`) | `MySqlPool::connect(&url)` (see [MySQL & MariaDB](mysql)) |
| MongoDB | A MongoDB Atlas cluster; copy its `mongodb+srv://` URI into a config var such as `MONGODB_URI` (**Settings → Config Vars**) | `mongodb` | `MongoDb::connect(&uri)` (see [MongoDB](mongodb)) |

### 3. Build and deploy

The simplest is the Rust buildpack. For a single crate whose binary, here `my-app`, is the app, add a `Procfile` at the root of your repository:

```text
web: ./target/release/my-app
```

And, optionally, to pin the Rust version, a `RustConfig`:

```text
VERSION=1.99
```

The buildpack runs a plain `cargo build --release`, keeps the binaries of `target/release` and keeps the compiled dependencies in its cache between builds. When more is needed (a package of a workspace, an example, a WebAssembly bundle), do as the Memoire repository: in `RustConfig`, `RUST_SKIP_BUILD=1` skips the buildpack's own build, and a `trap` runs a build script once Rust is installed (see its `RustConfig` and `heroku-build.sh`). Memoire's TLS support is pure Rust (`rustls`), so nothing needs OpenSSL. Then copy `.github/workflows/deploy-heroku.yml` into your repository, add the two secrets as in method 2, and every push to `master` deploys your app (method 3 works too, once the app has the `emk/rust` buildpack).

To build a Docker image instead, give your repository a `Dockerfile` and a `heroku.yml`, and set the `HEROKU_BUILD` variable to `docker` (method 2). A minimal `Dockerfile`:

```dockerfile
FROM rust:1-slim-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/my-app /usr/local/bin/my-app
CMD ["my-app"]
```

And `heroku.yml`:

```yaml
build:
  docker:
    web: Dockerfile
```

Here too, the final image needs no OpenSSL.

## Next steps

- [PostgreSQL](postgres): connection strings, TLS and pool sizing.
- [Migrations](migrations): keep the production schema up to date at each deploy.
- [Publish to crates.io](publish-crates-io): release a new version of the crate, also from a phone.
