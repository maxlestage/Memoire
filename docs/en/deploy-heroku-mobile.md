# Deploy to Heroku from your phone

You can put the Memoire website online, and keep it up to date, using only a phone: no computer, no terminal. This page walks through three ways to do it, explains what happens behind the scenes, and ends with deploying your own Rust application that uses Memoire and Heroku Postgres.

## Before you start

You need:

- a **GitHub account** and your own copy of the repository: open `https://github.com/maxlestage/Memoire` and tap **Fork** (only needed for methods 2 and 3);
- a **Heroku account** (sign up at `https://signup.heroku.com` in your phone's browser);
- a **payment method** on the Heroku account. Heroku has had no free plan since November 2022: the website runs on a *Basic* dyno (about 7 USD per month at the time of writing) and Heroku will not start apps on an unverified account. Check `https://www.heroku.com/pricing` for current prices.

All the Heroku steps happen in the Heroku dashboard (`https://dashboard.heroku.com`) in your mobile browser. The GitHub steps work in the GitHub mobile app, except managing secrets, which needs the GitHub website in a mobile browser.

> **Tip** If a menu or tab is missing on a small screen, use your browser's "Request desktop site" option. Turning the phone sideways also helps with wide pages such as repository settings.

## Which method should I choose?

| Method | Effort | Updates after the first deploy | Where the image is built |
|---|---|---|---|
| 1. "Deploy to Heroku" button | Lowest: a few taps | None: it deploys once | On Heroku |
| 2. GitHub Actions workflow | Medium: two secrets to add | Automatic on every push to `master`, or one tap in the GitHub app | On GitHub, then sent to Heroku |
| 3. Heroku's GitHub integration | Medium: connect GitHub to Heroku | Automatic on every push to `master` | On Heroku |

A common combination: method 1 for the very first deploy, then method 2 or 3 to keep the site updated. Do not enable methods 2 and 3 at the same time, or every push will be deployed twice.

## Method 1: the "Deploy to Heroku" button

The repository's README has a purple **Deploy to Heroku** button. It points to:

```text
https://heroku.com/deploy?template=https://github.com/maxlestage/Memoire
```

1. Open the repository on GitHub (app or browser) and scroll to the top of the README.
2. Tap **Deploy to Heroku**. Your browser opens Heroku; log in if asked.
3. Choose an **App name** (lowercase letters, digits and dashes, unique across Heroku) and a **region** (United States or Europe).
4. Leave the `RUST_LOG` setting at `info`.
5. Tap **Deploy app**.
6. Wait for the build. The first one compiles the whole website from source, which can take 10 minutes or more. You can follow it live on the page.
7. When Heroku shows "Your app was successfully deployed", tap **View** to open your site.

What happens behind the scenes:

- `app.json` describes the app for Heroku: its name and description, the **container** stack, one `web` dyno of size *Basic*, and the `RUST_LOG` setting.
- `heroku.yml` tells Heroku to build the `web` process from the `Dockerfile` at the root of the repository.
- The `Dockerfile` builds the Yew website (compiled to WebAssembly with Trunk) and the small Rust web server in a first stage, then copies only the server binary and the static files into a slim final image.

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

### Step 3: deploy

Any push to `master` now deploys automatically. To deploy right away from the **GitHub mobile app**:

1. Open your repository in the app.
2. Tap **Actions** (in the repository's menu; you may need to tap **More** first).
3. Tap the **Deploy to Heroku** workflow.
4. Tap **Run workflow**, keep the branch `master`, and confirm with **Run workflow**.
5. Tap the new run to follow its progress. When it finishes, its summary shows "Deployed to" followed by your app's address.

### What the workflow does

1. **Checks the configuration.** If a secret is missing, it stops politely with a notice ("Heroku deployment skipped") instead of failing.
2. **Prepares the app.** It creates the Heroku app if it does not exist yet, and makes sure it uses the **container** stack.
3. **Builds the Docker image** from the root `Dockerfile` on GitHub's machines, reusing a build cache between runs so later deploys are faster.
4. **Pushes the image** to Heroku's container registry (`registry.heroku.com`) and **releases** it as the new version of the `web` process.
5. **Checks the health** of the site: it calls `/healthz` on your app for up to five minutes and reports the result.

Only one deployment runs at a time; a new push waits for the current deployment to finish.

## Method 3: Heroku's GitHub integration

Heroku can watch your repository and build each new commit itself.

1. Create the app first with method 1 or method 2 (both set up the **container** stack this method needs).
2. In the Heroku dashboard, open your app and tap the **Deploy** tab.
3. Under **Deployment method**, tap **GitHub**, then **Connect to GitHub**, and authorise Heroku.
4. Search for your repository (`Memoire`), then tap **Connect**.
5. Under **Automatic deploys**, choose the `master` branch and tap **Enable Automatic Deploys**. Optionally tick "Wait for CI to pass before deploy".
6. To deploy immediately, scroll to **Manual deploy** and tap **Deploy Branch**.

Heroku then builds the image from `heroku.yml` and the `Dockerfile` on every push to `master`.

## How merging triggers a deployment

Pull requests that target `master` are **merged automatically** as soon as every CI check is green (unless the pull request is a draft, comes from a fork, or has the `no-auto-merge` label). After the merge, the CI workflow explicitly starts the **Deploy to Heroku** workflow, because merges performed by GitHub Actions do not trigger other workflows on their own.

The result: approve or open a pull request from your phone, wait for the checks, and the new version goes online by itself (with method 2). With method 3, Heroku sees the new commit on `master` and builds it.

## Troubleshooting

| Problem | Where to look / what to do |
|---|---|
| Build fails (methods 1 and 3) | Heroku dashboard → your app → **Activity** tab → **View build log** |
| Build fails (method 2) | GitHub app → **Actions** → the failed run → the red step |
| Site does not open, or "Application error" | Heroku dashboard → your app → **More** (top right) → **View logs** |
| Workflow says "Heroku deployment skipped" | `HEROKU_API_KEY` or `HEROKU_APP_NAME` is missing (check the spelling) |
| `401` or "Invalid credentials" in the workflow | The API key is wrong, expired or revoked: create a new one and update the secret |
| "Name is already taken" | App names are global on Heroku: choose another `HEROKU_APP_NAME` |
| Heroku asks to verify the account | Add a payment method under **Account settings → Billing** |
| Method 3 fails to detect a buildpack | The app is not on the container stack: create it with method 1 or 2 |

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
memoire-orm = { version = "0.1", features = ["postgres-tls"] }
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
    // Start your web framework here (axum, actix-web...), sharing `db` with your handlers.
    Ok(())
}
```

Keep the pool small: the smallest Heroku Postgres plans accept only a limited number of connections (check your plan), shared by all your dynos. See [PostgreSQL](postgres) for the TLS options.

### 3. Build and deploy

Give your repository a `Dockerfile` and a `heroku.yml`, like the Memoire repository does. A minimal `Dockerfile` for a Rust binary called `my-app`:

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

Memoire's TLS support is pure Rust (`rustls`), so the final image needs no OpenSSL. Then copy `.github/workflows/deploy-heroku.yml` into your repository, add the two secrets as in method 2, and every push to `master` deploys your app.

## Next steps

- [PostgreSQL](postgres): connection strings, TLS and pool sizing.
- [Migrations](migrations): keep the production schema up to date at each deploy.
- [Publish to crates.io](publish-crates-io): release a new version of the crate, also from a phone.
