# Publish to crates.io

Memoire ORM is published on [crates.io](https://crates.io/crates/memoire-orm), Rust's package registry, as two crates: `memoire-orm-derive` (the derive macros) and `memoire-orm` (the ORM, which depends on the first). This page explains how a new version is released, by a GitHub workflow that you can start from a phone, or by hand from a computer.

## How releases work

The workflow `.github/workflows/publish.yml`, named **Publish to crates.io**, runs when:

- a tag whose name starts with `v` is pushed (for example `v0.2.0`), or
- a GitHub **release** is published, or
- you start it manually from the **Actions** tab (by default as a dry run, which checks everything without uploading).

When it runs, it:

1. reads the version from the workspace and, for a tag, **checks that the tag matches it** (tag `v0.2.0` requires version `0.2.0`; otherwise it stops with an error);
2. runs the test suite (memory and SQLite backends);
3. publishes **`memoire-orm-derive` first, then `memoire-orm`**, because `memoire-orm` depends on the exact same version of the derive crate;
4. skips any crate whose version is already on crates.io, so running it again is always safe;
5. writes a summary with a link to the new version.

## One-time setup: the crates.io token

The workflow needs a crates.io API token stored as a repository secret called `CARGO_REGISTRY_TOKEN`.

### Create the token (from a phone)

1. Open `https://crates.io` in your mobile browser and **Log in with GitHub**.
2. Make sure your email address is verified (crates.io refuses to publish otherwise): open **Account Settings** and check the email section.
3. In **Account Settings**, open **API Tokens** and tap **New Token**.
4. Give it a name (for example "GitHub Actions Memoire") and, if you like, an expiration date.
5. Select the scopes **publish-new** (needed the first time a crate name is published) and **publish-update** (needed for new versions). You can also restrict the token to the crates `memoire-orm` and `memoire-orm-derive`.
6. Tap **Generate Token** and copy it. crates.io shows it only once.

### Store it in GitHub

1. In your mobile browser (secrets cannot be edited in the GitHub app), open the repository, then **Settings → Secrets and variables → Actions**.
2. Tap **New repository secret**.
3. Name: `CARGO_REGISTRY_TOKEN`. Value: the token. Tap **Add secret**.

> **Note** Only the owners of a crate on crates.io can publish new versions of it. If you work on a fork, the crate names already belong to the original authors.

## Releasing a new version

### 1. Bump the version

Every release needs a new version number (crates.io never accepts the same version twice). Follow [semantic versioning](https://semver.org): increase the last number for fixes (`0.1.0` → `0.1.1`), the middle one for new features or, before 1.0, breaking changes (`0.1.1` → `0.2.0`).

Three places change:

| File | What to change |
|---|---|
| `Cargo.toml` (repository root), `[workspace.package]` | `version = "0.2.0"`: both crates take their version from here |
| `Cargo.toml` (repository root), `[workspace.dependencies]` | `memoire-orm-derive = { version = "=0.2.0", path = "crates/memoire-orm-derive" }`: the exact pin must match the new version |
| `CHANGELOG.md` | Move the items under `[Unreleased]` into a new `[0.2.0] - YYYY-MM-DD` section |

The `=` in `"=0.2.0"` pins `memoire-orm` to exactly the matching derive crate, so the two crates are always used together.

From a phone, edit these files on the GitHub website: open the file, tap the pencil icon, make the change, and choose **Create a new branch for this commit and start a pull request**. Once the CI checks pass, the pull request is merged into `master` automatically.

### 2. Create the release

Once the version bump is on `master`:

1. In your mobile browser, open the repository's **Releases** page and tap **Draft a new release** (or go straight to `https://github.com/<owner>/Memoire/releases/new`).
2. Tap **Choose a tag**, type the new tag, for example `v0.2.0`, and select **Create new tag: v0.2.0 on publish**. Keep `master` as the target.
3. Enter a title (for example `v0.2.0`) and the release notes; the **Generate release notes** button fills them from the merged pull requests.
4. Tap **Publish release**.

The workflow starts within seconds. Follow it in the GitHub app under **Actions → Publish to crates.io**. When it is green, the new version is on crates.io, and docs.rs builds the API documentation shortly after.

> **Tip** Publishing a release also creates the tag, so the workflow may start twice. That is fine: the runs are queued one after the other, and the second one skips the crates that are already published.

### Checking without publishing

To check that everything would publish correctly, without uploading anything:

1. In the GitHub app, open **Actions → Publish to crates.io**.
2. Tap **Run workflow**, leave **dry_run** enabled, and run it.

This runs `cargo publish --workspace --dry-run`, which packages both crates and compiles them as crates.io would.

## Publishing by hand

From a computer with Rust installed, after bumping the version as above:

```bash
# Once: store your crates.io token locally
cargo login

# Check everything without uploading (Cargo 1.90 or newer verifies both crates together)
cargo publish --workspace --dry-run

# Publish, derive crate first
cargo publish -p memoire-orm-derive && cargo publish -p memoire-orm

# Tag the release
git tag v0.2.0
git push origin v0.2.0
```

With an older Cargo, check the derive crate with `cargo publish -p memoire-orm-derive --dry-run`; a dry run of `memoire-orm` alone only succeeds once the matching derive version is on crates.io.

## Release checklist

- [ ] `version` bumped in `[workspace.package]` of the root `Cargo.toml`
- [ ] `memoire-orm-derive = { version = "=X.Y.Z", ... }` pin updated to the same version
- [ ] `CHANGELOG.md` updated with the new version and date
- [ ] CI green on `master`
- [ ] Dry run passes (`cargo publish --workspace --dry-run`, or the workflow with **dry_run**)
- [ ] Release `vX.Y.Z` published (or tag pushed)
- [ ] New version visible on crates.io, documentation built on docs.rs

> **Warning** A published version can never be changed or deleted. If a release is broken, publish a fixed version and *yank* the broken one (`cargo yank memoire-orm --version X.Y.Z`, and the same for `memoire-orm-derive` if needed), which stops new projects from selecting it.

## Next steps

- [Deploy to Heroku from your phone](deploy-heroku-mobile): put the website online.
- [Installation](installation): how users add the published crate.
- [FAQ](faq): including how to contribute.
