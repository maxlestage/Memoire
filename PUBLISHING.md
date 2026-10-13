# Publishing on crates.io

Memoire is published on [crates.io](https://crates.io/crates/memoire-orm) as two crates,
`memoire-orm-derive` (the derive macros) and `memoire-orm`, under its
[proprietary license](LICENSE): the source is visible there so that it can be audited and
built, and anyone may depend on it.

## One release = one push

1. Bump `version` in the root [`Cargo.toml`](Cargo.toml) (and the `=x.y.z` pin of
   `memoire-orm-derive` next to it), write the version's section in
   [`CHANGELOG.md`](CHANGELOG.md) and commit.
2. Push to `master`. That is all: no form to fill in.

Once CI is green on `master`, the [`publish.yml`](.github/workflows/publish.yml) workflow sees
that the version has no tag yet, creates the tag `vX.Y.Z` (the same version with a `v` in
front) and the GitHub release, with the version's section of `CHANGELOG.md` as notes (or
GitHub's generated notes if there is none), runs the tests and publishes
`memoire-orm-derive`, then `memoire-orm`, skipping a crate whose version is already online.
It works from the GitHub website on a phone; nothing is installed locally. A release created
by hand (**Releases → Draft a new release**) or a pushed tag publishes the same way, after
checking that the tag matches the version. From the **Actions** tab it can also be started by
hand, as a dry run by default (the packages are built and checked, nothing is uploaded).

## One-time setup

- A crates.io account with a verified e-mail address.
- A token from [crates.io/settings/tokens](https://crates.io/settings/tokens) with the scopes
  **publish-new** and **publish-update**, saved as the repository secret
  `CARGO_REGISTRY_TOKEN` (GitHub → Settings → Secrets and variables → Actions). Without it,
  the tag and the GitHub release are still created, but the publication on crates.io is
  postponed: add the secret, then run the workflow by hand from **Actions** with **dry_run**
  unticked.

## From a computer

```sh
cargo login                                     # paste the token, once
cargo publish -p memoire-orm-derive
cargo publish -p memoire-orm                    # once the derive crate is online
```

## The ecosystem, in order

The seven projects by Maxime Nathan Lestage depend on each other through crates.io, so they
are published in this order, each one after the previous one is online:

1. **Active** — `active`
2. **Vitesse** — `vitesse`
3. **Securite** — `securite`
4. **Memoire** — `memoire-orm-derive`, then `memoire-orm` (this repository)
5. **Paquets** — `paquets-macros`, then `paquets` (depends on the four above)
6. **FerroSwift** — `ferroswift-macros`, `ferroswift-transpiler`, `ferroswift`, `ferroswift-cli`
7. **Solidify** — `solidify` (and `solidify-rs` on npm; depends on none of the above)
