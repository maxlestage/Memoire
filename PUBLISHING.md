# Publishing on crates.io

Memoire is published on [crates.io](https://crates.io/crates/memoire-orm) as two crates,
`memoire-orm-derive` (the derive macros) and `memoire-orm`, under its
[proprietary license](LICENSE): the source is visible there so that it can be audited and
built, and anyone may depend on it.

## One release = one tag

1. Bump `version` in the root [`Cargo.toml`](Cargo.toml) (and the `=x.y.z` pin of
   `memoire-orm-derive` next to it), update [`CHANGELOG.md`](CHANGELOG.md) and commit.
2. Open **Releases → Draft a new release**, create the tag `vX.Y.Z` (the same version with a
   `v` in front) and tap **Publish release** — or push the tag.

The [`publish.yml`](.github/workflows/publish.yml) workflow checks that the tag matches the
version, runs the tests and publishes `memoire-orm-derive`, then `memoire-orm`, skipping a
crate whose version is already online. It works from the GitHub website on a phone; nothing
is installed locally. From the **Actions** tab it can also be started by hand, as a dry run by
default (the packages are built and checked, nothing is uploaded).

## One-time setup

- A crates.io account with a verified e-mail address.
- A token from [crates.io/settings/tokens](https://crates.io/settings/tokens) with the scopes
  **publish-new** and **publish-update**, saved as the repository secret
  `CARGO_REGISTRY_TOKEN` (GitHub → Settings → Secrets and variables → Actions).

## From a computer

```sh
cargo login                                     # paste the token, once
cargo publish -p memoire-orm-derive
cargo publish -p memoire-orm                    # once the derive crate is online
```

## The ecosystem, in order

The six projects by Maxime Nathan Lestage depend on each other through crates.io, so they
are published in this order, each one after the previous one is online:

1. **Active** — `active`
2. **Vitesse** — `vitesse`
3. **Securite** — `securite`
4. **Memoire** — `memoire-orm-derive`, then `memoire-orm` (this repository)
5. **Paquets** — `paquets-macros`, then `paquets` (depends on the four above)
6. **FerroSwift** — `ferroswift-macros`, `ferroswift-transpiler`, `ferroswift`, `ferroswift-cli`
