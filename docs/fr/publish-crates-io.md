# Publier sur crates.io

Memoire ORM est publié sur [crates.io](https://crates.io/crates/memoire-orm), le registre de paquets de Rust, sous la forme de deux crates : `memoire-orm-derive` (les macros derive) et `memoire-orm` (l'ORM, qui dépend de la première). Cette page explique comment une nouvelle version est publiée : par un workflow GitHub que vous pouvez lancer depuis un téléphone, ou à la main depuis un ordinateur.

## Comment fonctionnent les publications

Le workflow `.github/workflows/publish.yml`, nommé **Publish to crates.io**, s'exécute quand :

- un tag dont le nom commence par `v` est poussé (par exemple `v0.2.0`), ou
- une **release** GitHub est publiée, ou
- vous le lancez manuellement depuis l'onglet **Actions** (par défaut en mode dry run, qui vérifie tout sans rien envoyer).

Lorsqu'il s'exécute, il :

1. lit la version dans le workspace et, pour un tag, **vérifie que le tag correspond** (le tag `v0.2.0` exige la version `0.2.0` ; sinon il s'arrête avec une erreur) ;
2. lance la suite de tests (backends mémoire et SQLite) ;
3. publie **d'abord `memoire-orm-derive`, puis `memoire-orm`**, car `memoire-orm` dépend exactement de la même version de la crate derive ;
4. ignore toute crate dont la version est déjà sur crates.io : le relancer est donc toujours sans risque ;
5. écrit un résumé avec un lien vers la nouvelle version.

## Configuration initiale : le token crates.io

Le workflow a besoin d'un token d'API crates.io enregistré comme secret de dépôt sous le nom `CARGO_REGISTRY_TOKEN`.

### Créer le token (depuis un téléphone)

1. Ouvrez `https://crates.io` dans votre navigateur mobile et utilisez **Log in with GitHub** pour vous connecter.
2. Vérifiez que votre adresse e-mail est validée (sinon crates.io refuse la publication) : ouvrez **Account Settings** et contrôlez la section consacrée à l'e-mail.
3. Dans **Account Settings**, ouvrez **API Tokens** et appuyez sur **New Token**.
4. Donnez-lui un nom (par exemple "GitHub Actions Memoire") et, si vous le souhaitez, une date d'expiration.
5. Sélectionnez les scopes **publish-new** (nécessaire la première fois qu'un nom de crate est publié) et **publish-update** (nécessaire pour les nouvelles versions). Vous pouvez aussi restreindre le token aux crates `memoire-orm` et `memoire-orm-derive`.
6. Appuyez sur **Generate Token** et copiez-le. crates.io ne l'affiche qu'une seule fois.

### L'enregistrer dans GitHub

1. Dans votre navigateur mobile (les secrets ne peuvent pas être modifiés dans l'application GitHub), ouvrez le dépôt, puis **Settings → Secrets and variables → Actions**.
2. Appuyez sur **New repository secret**.
3. Name : `CARGO_REGISTRY_TOKEN`. Value : le token. Appuyez sur **Add secret**.

> **Remarque** Seuls les propriétaires d'une crate sur crates.io peuvent en publier de nouvelles versions. Si vous travaillez sur un fork, les noms des crates appartiennent déjà aux auteurs d'origine.

## Publier une nouvelle version

### 1. Incrémenter la version

Chaque publication nécessite un nouveau numéro de version (crates.io n'accepte jamais deux fois la même version). Suivez le [versionnage sémantique](https://semver.org) : augmentez le dernier nombre pour les corrections (`0.1.0` → `0.1.1`), celui du milieu pour les nouvelles fonctionnalités ou, avant la 1.0, pour les changements cassants (`0.1.1` → `0.2.0`).

Trois endroits changent :

| Fichier | Ce qu'il faut modifier |
|---|---|
| `Cargo.toml` (racine du dépôt), `[workspace.package]` | `version = "0.2.0"` : les deux crates prennent leur version ici |
| `Cargo.toml` (racine du dépôt), `[workspace.dependencies]` | `memoire-orm-derive = { version = "=0.2.0", path = "crates/memoire-orm-derive" }` : la version épinglée exacte doit correspondre à la nouvelle version |
| `CHANGELOG.md` | Déplacez les éléments situés sous `[Unreleased]` dans une nouvelle section `[0.2.0] - YYYY-MM-DD` |

Le `=` de `"=0.2.0"` épingle `memoire-orm` exactement sur la crate derive correspondante : les deux crates sont ainsi toujours utilisées ensemble.

Depuis un téléphone, modifiez ces fichiers sur le site web de GitHub : ouvrez le fichier, appuyez sur l'icône en forme de crayon, faites la modification, puis choisissez **Create a new branch for this commit and start a pull request**. Une fois les vérifications de la CI passées, la pull request est automatiquement mergée dans `master`.

### 2. Créer la release

Une fois l'incrément de version arrivé sur `master` :

1. Dans votre navigateur mobile, ouvrez la page **Releases** du dépôt et appuyez sur **Draft a new release** (ou allez directement sur `https://github.com/<owner>/Memoire/releases/new`).
2. Appuyez sur **Choose a tag**, tapez le nouveau tag, par exemple `v0.2.0`, et sélectionnez **Create new tag: v0.2.0 on publish**. Gardez `master` comme cible.
3. Saisissez un titre (par exemple `v0.2.0`) et les notes de version ; le bouton **Generate release notes** les remplit à partir des pull requests mergées.
4. Appuyez sur **Publish release**.

Le workflow démarre en quelques secondes. Suivez-le dans l'application GitHub, sous **Actions → Publish to crates.io**. Quand il est au vert, la nouvelle version est sur crates.io, et docs.rs génère la documentation de l'API peu après.

> **Astuce** Publier une release crée aussi le tag : le workflow peut donc démarrer deux fois. Ce n'est pas un problème : les exécutions sont mises en file d'attente l'une après l'autre, et la seconde ignore les crates déjà publiées.

### Vérifier sans publier

Pour vérifier que tout serait publié correctement, sans rien envoyer :

1. Dans l'application GitHub, ouvrez **Actions → Publish to crates.io**.
2. Appuyez sur **Run workflow**, laissez **dry_run** activé, et lancez-le.

Cela exécute `cargo publish --workspace --dry-run`, qui empaquette les deux crates et les compile comme le ferait crates.io.

## Publier à la main

Depuis un ordinateur sur lequel Rust est installé, après avoir incrémenté la version comme ci-dessus :

```bash
# Une seule fois : enregistrez votre token crates.io en local
cargo login

# Tout vérifier sans rien envoyer (Cargo 1.90 ou plus récent vérifie les deux crates ensemble)
cargo publish --workspace --dry-run

# Publier, la crate derive en premier
cargo publish -p memoire-orm-derive && cargo publish -p memoire-orm

# Tagger la release
git tag v0.2.0
git push origin v0.2.0
```

Avec une version plus ancienne de Cargo, vérifiez la crate derive avec `cargo publish -p memoire-orm-derive --dry-run` ; un dry run de `memoire-orm` seul ne réussit qu'une fois la version correspondante de la crate derive présente sur crates.io.

## Check-list de publication

- [ ] `version` incrémentée dans `[workspace.package]` du `Cargo.toml` racine
- [ ] Version épinglée `memoire-orm-derive = { version = "=X.Y.Z", ... }` mise à jour avec la même version
- [ ] `CHANGELOG.md` mis à jour avec la nouvelle version et la date
- [ ] CI au vert sur `master`
- [ ] Le dry run passe (`cargo publish --workspace --dry-run`, ou le workflow avec **dry_run**)
- [ ] Release `vX.Y.Z` publiée (ou tag poussé)
- [ ] Nouvelle version visible sur crates.io, documentation générée sur docs.rs

> **Attention** Une version publiée ne peut jamais être modifiée ni supprimée. Si une release est défectueuse, publiez une version corrigée et faites un *yank* de la version défectueuse (`cargo yank memoire-orm --version X.Y.Z`, et de même pour `memoire-orm-derive` si nécessaire), ce qui empêche les nouveaux projets de la sélectionner.

## Étapes suivantes

- [Déployer sur Heroku depuis son téléphone](deploy-heroku-mobile) : mettez le site en ligne.
- [Installation](installation) : comment les utilisateurs ajoutent la crate publiée.
- [FAQ](faq) : notamment comment contribuer.
