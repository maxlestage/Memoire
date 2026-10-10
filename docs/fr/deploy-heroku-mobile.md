# Déployer sur Heroku depuis son téléphone

Vous pouvez mettre en ligne le site de Memoire, et le maintenir à jour, uniquement avec un téléphone : ni ordinateur, ni terminal. Cette page présente trois façons de procéder, explique ce qui se passe en coulisses, et se termine par le déploiement de votre propre application Rust qui utilise Memoire et Heroku Postgres.

## Avant de commencer

Il vous faut :

- un **compte GitHub** et votre propre copie du dépôt : ouvrez `https://github.com/maxlestage/Memoire` et appuyez sur **Fork** (nécessaire uniquement pour les méthodes 2 et 3) ;
- un **compte Heroku** (inscrivez-vous sur `https://signup.heroku.com` depuis le navigateur de votre téléphone) ;
- un **moyen de paiement** enregistré sur le compte Heroku. Heroku n'a plus d'offre gratuite depuis novembre 2022 : le site tourne sur un dyno *Basic* (environ 7 USD par mois au moment où ces lignes sont écrites), et Heroku ne démarre pas d'applications sur un compte non vérifié. Consultez `https://www.heroku.com/pricing` pour connaître les tarifs actuels.

Toutes les étapes côté Heroku se font dans le tableau de bord Heroku (`https://dashboard.heroku.com`), depuis votre navigateur mobile. Les étapes côté GitHub fonctionnent dans l'application mobile GitHub, sauf la gestion des secrets, qui nécessite le site web de GitHub dans un navigateur mobile.

> **Astuce** Si un menu ou un onglet n'apparaît pas sur un petit écran, utilisez l'option « Version pour ordinateur » (Request desktop site) de votre navigateur. Mettre le téléphone à l'horizontale aide aussi pour les pages larges, comme les paramètres d'un dépôt.

## Quelle méthode choisir ?

| Méthode | Effort | Mises à jour après le premier déploiement | Où l'image est construite |
|---|---|---|---|
| 1. Bouton "Deploy to Heroku" | Minimal : quelques appuis | Aucune : il ne déploie qu'une fois | Sur Heroku |
| 2. Workflow GitHub Actions | Moyen : deux secrets à ajouter | Automatiques à chaque push sur `master`, ou en un appui dans l'application GitHub | Sur GitHub, puis envoyée à Heroku |
| 3. Intégration GitHub de Heroku | Moyen : connecter GitHub à Heroku | Automatiques à chaque push sur `master` | Sur Heroku |

Une combinaison courante : la méthode 1 pour le tout premier déploiement, puis la méthode 2 ou 3 pour garder le site à jour. N'activez pas les méthodes 2 et 3 en même temps, sinon chaque push sera déployé deux fois.

> **Astuce** Pas de compte Heroku, ou pas de budget ? Le site peut aussi être publié **gratuitement sur GitHub Pages**, en changeant un seul réglage : voir la section GitHub Pages plus bas.

## Méthode 1 : le bouton "Deploy to Heroku"

Le README du dépôt contient un bouton violet **Deploy to Heroku**. Il pointe vers :

```text
https://heroku.com/deploy?template=https://github.com/maxlestage/Memoire
```

1. Ouvrez le dépôt sur GitHub (application ou navigateur) et faites défiler jusqu'en haut du README.
2. Appuyez sur **Deploy to Heroku**. Votre navigateur ouvre Heroku ; connectez-vous si on vous le demande.
3. Choisissez un **App name** (nom de l'application : lettres minuscules, chiffres et tirets, unique sur tout Heroku) et une **region** (United States ou Europe).
4. Laissez le réglage `RUST_LOG` sur `info`.
5. Appuyez sur **Deploy app**.
6. Attendez la fin du build. Le premier compile tout le site depuis les sources, ce qui peut prendre 10 minutes ou plus. Vous pouvez le suivre en direct sur la page.
7. Quand Heroku affiche "Your app was successfully deployed", appuyez sur **View** pour ouvrir votre site.

Ce qui se passe en coulisses :

- `app.json` décrit l'application pour Heroku : son nom et sa description, la stack **container**, un dyno `web` de taille *Basic* et le réglage `RUST_LOG`.
- `heroku.yml` indique à Heroku de construire le processus `web` à partir du `Dockerfile` situé à la racine du dépôt.
- Le `Dockerfile` construit le site, écrit en Rust avec [active](https://github.com/maxlestage/Active), dans une première étape : le paquet du navigateur (les parties interactives et le moteur en mémoire du bac à sable, compilés en WebAssembly avec `wasm-bindgen`), puis le serveur web, qui rend chaque page et embarque la documentation. Il ne copie ensuite que le binaire du serveur et les fichiers statiques dans une image finale allégée.

> **Remarque** Le bouton déploie le dépôt tel qu'il est à ce moment-là. Pour récupérer les modifications ultérieures, utilisez la méthode 2 ou 3, ou appuyez à nouveau sur le bouton pour créer une nouvelle application.

## Méthode 2 : GitHub Actions (automatique, et en un appui depuis l'application GitHub)

Le dépôt contient un workflow, `.github/workflows/deploy-heroku.yml`, nommé **Deploy to Heroku**. Il s'exécute à chaque push sur `master` et peut aussi être lancé à la main. Il a besoin de deux secrets de dépôt.

### Étape 1 : obtenir une clé d'API Heroku

1. Dans votre navigateur mobile, ouvrez `https://dashboard.heroku.com/account`.
2. Faites défiler jusqu'à **API Key** et appuyez sur **Reveal**, puis copiez la clé.

Pour un token dédié à GitHub (recommandé, car vous pouvez le révoquer indépendamment) :

1. Ouvrez `https://dashboard.heroku.com/account/applications`.
2. Sous **Authorizations**, appuyez sur **Create authorization**.
3. Saisissez une description comme "GitHub Actions deploy", laissez l'expiration vide (ou choisissez-en une), puis appuyez sur **Create**.
4. Copiez le token affiché. Heroku ne le montre qu'une seule fois.

### Étape 2 : ajouter les secrets à votre dépôt

Les secrets ne peuvent pas être modifiés dans l'application GitHub : utilisez donc le site web dans votre navigateur mobile.

1. Ouvrez votre fork : `https://github.com/<your-name>/Memoire`.
2. Appuyez sur **Settings**, puis **Secrets and variables**, puis **Actions**.
3. Appuyez sur **New repository secret**. Name : `HEROKU_API_KEY`. Value : la clé ou le token de l'étape 1. Appuyez sur **Add secret**.
4. Appuyez à nouveau sur **New repository secret**. Name : `HEROKU_APP_NAME`. Value : le nom que vous voulez donner à votre application, par exemple `memoire-docs-yourname`. Appuyez sur **Add secret**.

`HEROKU_APP_NAME` peut aussi être créé sous forme de **variable** de dépôt (onglet **Variables** de la même page), puisqu'il n'est pas secret. Si l'application n'existe pas encore, le workflow la crée pour vous.

### Étape 3 : déployer

Tout push sur `master` déploie désormais automatiquement. Pour déployer tout de suite depuis l'**application mobile GitHub** :

1. Ouvrez votre dépôt dans l'application.
2. Appuyez sur **Actions** (dans le menu du dépôt ; il faudra peut-être d'abord appuyer sur **More**).
3. Appuyez sur le workflow **Deploy to Heroku**.
4. Appuyez sur **Run workflow**, gardez la branche `master` et confirmez avec **Run workflow**.
5. Appuyez sur la nouvelle exécution pour suivre sa progression. Une fois terminée, son résumé affiche "Deployed to" suivi de l'adresse de votre application.

### Ce que fait le workflow

1. **Il vérifie la configuration.** S'il manque un secret, il s'arrête poliment avec un avertissement ("Heroku deployment skipped") au lieu d'échouer.
2. **Il prépare l'application.** Il crée l'application Heroku si elle n'existe pas encore, et s'assure qu'elle utilise la stack **container**.
3. **Il construit l'image Docker** à partir du `Dockerfile` racine sur les machines de GitHub, en réutilisant un cache de build d'une exécution à l'autre pour accélérer les déploiements suivants.
4. **Il pousse l'image** vers le registre de conteneurs de Heroku (`registry.heroku.com`) et la **publie** (release) comme nouvelle version du processus `web`.
5. **Il vérifie l'état de santé** du site : il appelle `/healthz` sur votre application pendant cinq minutes au maximum et rapporte le résultat.

Un seul déploiement s'exécute à la fois ; un nouveau push attend que le déploiement en cours se termine.

## Méthode 3 : l'intégration GitHub de Heroku

Heroku peut surveiller votre dépôt et construire lui-même chaque nouveau commit.

1. Créez d'abord l'application avec la méthode 1 ou la méthode 2 (les deux configurent la stack **container** dont cette méthode a besoin).
2. Dans le tableau de bord Heroku, ouvrez votre application et appuyez sur l'onglet **Deploy**.
3. Sous **Deployment method**, appuyez sur **GitHub**, puis sur **Connect to GitHub**, et autorisez Heroku.
4. Recherchez votre dépôt (`Memoire`), puis appuyez sur **Connect**.
5. Sous **Automatic deploys**, choisissez la branche `master` et appuyez sur **Enable Automatic Deploys**. Vous pouvez aussi cocher "Wait for CI to pass before deploy" (attendre que la CI passe avant de déployer).
6. Pour déployer immédiatement, faites défiler jusqu'à **Manual deploy** et appuyez sur **Deploy Branch**.

Heroku construit ensuite l'image à partir de `heroku.yml` et du `Dockerfile` à chaque push sur `master`.

## GitHub Pages : l'option gratuite

Le serveur du site sait aussi écrire chaque page, dans chaque langue, en fichiers statiques : GitHub Pages peut donc aussi l'héberger, gratuitement, à l'adresse `https://<votre-compte>.github.io/Memoire/` (pour ce dépôt : `https://maxlestage.github.io/Memoire/`). Ni compte Heroku, ni secret : le workflow `.github/workflows/pages.yml` le construit et le publie.

À activer une seule fois :

1. Dans le navigateur de votre téléphone, ouvrez votre dépôt sur `https://github.com`, puis **Settings → Pages**.
2. Sous **Build and deployment → Source**, choisissez **GitHub Actions**.
3. Dans l'application GitHub, ouvrez **Actions → Deploy to GitHub Pages → Run workflow**. Deux ou trois minutes plus tard, le site est en ligne ; son adresse s'affiche sur la page de l'exécution.

Ensuite, chaque push sur `master` qui modifie le site, la documentation ou l'ORM publie la nouvelle version, de même que chaque pull request mergée automatiquement.

> **Remarque** Avec un compte GitHub gratuit, GitHub Pages ne fonctionne que pour les dépôts publics.

Ce que fait le workflow :

- il s'arrête avec le message "GitHub Pages deployment skipped" tant que l'étape 2 n'est pas faite ;
- il construit le paquet du navigateur, puis lance `memoire-site export`, qui écrit chaque page dans son propre fichier HTML sous `/Memoire/` (`fr/docs/models/`, `en/playground/`...), avec l'index de recherche, le manifeste d'application web et le service worker : les liens vers n'importe quelle page et les rechargements fonctionnent, les anciennes adresses (`docs/models`...) mènent à la langue du visiteur, et les adresses inconnues affichent la page « introuvable » du site ;
- il publie le site, puis vérifie que les pages d'accueil, une page de documentation, le bac à sable, le manifeste et le service worker répondent.

GitHub Pages et Heroku sont indépendants : vous pouvez utiliser les deux. La seule différence : GitHub Pages ne peut pas envoyer les en-têtes de sécurité supplémentaires (comme `Content-Security-Policy`) qu'ajoute le serveur Heroku ; le site lui-même fonctionne de la même façon.

> **Astuce** Pour utiliser votre propre nom de domaine, renseignez-le dans **Settings → Pages → Custom domain**, puis relancez le workflow : il construit alors le site pour la racine de ce domaine.

## Installer le site sur votre téléphone

Le site est une application web progressive (PWA), où qu'il soit hébergé (Heroku ou GitHub Pages) :

- **Android** (Chrome) : ouvrez le site, touchez le menu **⋮**, puis **Installer l'application** (ou **Ajouter à l'écran d'accueil**).
- **iPhone et iPad** (Safari) : ouvrez le site, touchez **Partager**, puis **Sur l'écran d'accueil**.

Il s'ouvre alors en plein écran depuis son icône, comme une application. Une fois une page ouverte, le site fonctionne même sans réseau : les pages d'accueil, la première page de la documentation, le bac à sable et toutes les pages déjà lues sont gardés sur l'appareil. Le bac à sable n'a besoin d'aucun réseau : l'ORM tourne dans la page, compilé en WebAssembly. Une nouvelle version du site remplace celle enregistrée à la prochaine visite avec du réseau.

## Comment un merge déclenche un déploiement

Les pull requests qui ciblent `master` sont **mergées automatiquement** dès que toutes les vérifications de la CI sont au vert (sauf si la pull request est un brouillon, provient d'un fork ou porte le label `no-auto-merge`). Après le merge, le workflow de CI lance explicitement les workflows **Deploy to Heroku** et **Deploy to GitHub Pages**, car les merges effectués par GitHub Actions ne déclenchent pas d'autres workflows d'eux-mêmes.

Résultat : approuvez ou ouvrez une pull request depuis votre téléphone, attendez les vérifications, et la nouvelle version se met en ligne toute seule (avec la méthode 2). Avec la méthode 3, Heroku voit le nouveau commit sur `master` et le construit.

## Dépannage

| Problème | Où regarder / que faire |
|---|---|
| Le build échoue (méthodes 1 et 3) | Tableau de bord Heroku → votre application → onglet **Activity** → **View build log** |
| Le build échoue (méthode 2) | Application GitHub → **Actions** → l'exécution en échec → l'étape en rouge |
| Le site ne s'ouvre pas, ou "Application error" | Tableau de bord Heroku → votre application → **More** (en haut à droite) → **View logs** |
| Le workflow indique "Heroku deployment skipped" | `HEROKU_API_KEY` ou `HEROKU_APP_NAME` est manquant (vérifiez l'orthographe) |
| Le workflow indique "GitHub Pages deployment skipped" | **Settings → Pages → Source** n'est pas réglé sur **GitHub Actions** |
| `401` ou "Invalid credentials" dans le workflow | La clé d'API est erronée, expirée ou révoquée : créez-en une nouvelle et mettez à jour le secret |
| "Name is already taken" | Les noms d'applications sont globaux sur Heroku : choisissez un autre `HEROKU_APP_NAME` |
| Heroku demande de vérifier le compte | Ajoutez un moyen de paiement dans **Account settings → Billing** |
| La méthode 3 ne parvient pas à détecter de buildpack | L'application n'est pas sur la stack container : créez-la avec la méthode 1 ou 2 |

Bon à savoir :

- **Vérification de l'état de santé :** ouvrez `https://<your-app-address>/healthz` dans votre navigateur. Le serveur répond `ok` lorsqu'il tourne. L'adresse de votre application est accessible via le bouton **Open app** du tableau de bord.
- **Port :** Heroku choisit le port et le transmet dans la variable d'environnement `PORT` ; le serveur du site la lit automatiquement (8080 n'est que la valeur par défaut pour les exécutions en local). Rien à configurer.
- **Coûts :** le site utilise un dyno *Basic*. Vous pouvez passer à un dyno *Eco*, moins cher (il se met en veille quand personne ne visite le site), dans l'onglet **Resources**, ou y réduire le dyno à zéro pour ne plus payer.

## Déployer votre propre application avec Memoire et Heroku Postgres

Les mêmes outils fonctionnent pour votre propre application web en Rust qui stocke ses données avec Memoire.

### 1. Ajouter une base de données

1. Dans le tableau de bord Heroku, ouvrez votre application et appuyez sur l'onglet **Resources**.
2. Sous **Add-ons**, recherchez **Heroku Postgres**, choisissez une offre et validez.
3. Heroku crée la base de données et ajoute une config var `DATABASE_URL` à votre application (visible dans **Settings → Reveal Config Vars**).

### 2. Se connecter depuis votre code

Heroku Postgres n'accepte que des connexions chiffrées : activez donc la feature `postgres-tls` :

```toml
[dependencies]
memoire-orm = { version = "0.1", features = ["postgres-tls"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Lisez ensuite `DATABASE_URL`, exécutez vos migrations au démarrage, et écoutez sur le port que Heroku vous attribue :

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
    // Heroku définit DATABASE_URL quand vous ajoutez Heroku Postgres.
    let url = std::env::var("DATABASE_URL")?;
    let url = if url.contains('?') { url } else { format!("{url}?sslmode=require") };
    let db = PgPool::connect_with(&url, 10).await?;

    Migrator::new()
        .add(Migration::new(1, "create_visits").create_table::<Visit>())
        .run(&db)
        .await?;

    // Heroku indique à votre serveur web quel port utiliser.
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
    println!("ready to listen on 0.0.0.0:{port}");
    // Démarrez ici votre framework web (axum, actix-web...), en partageant `db` avec vos handlers.
    Ok(())
}
```

Gardez un pool de petite taille : les plus petites offres Heroku Postgres n'acceptent qu'un nombre limité de connexions (vérifiez votre offre), partagé entre tous vos dynos. Consultez [PostgreSQL](postgres) pour les options TLS.

### Autres bases de données sur Heroku

Les autres backends serveur fonctionnent de la même façon : ajoutez le service, puis lisez son URL dans une config var.

| Base de données | Comment l'obtenir sur Heroku | Feature Memoire | Connexion |
|---|---|---|---|
| Redis | L'add-on **Heroku Key-Value Store**, qui définit `REDIS_URL` | `redis-tls` | `RedisDb::connect(&format!("{url}#insecure"))` (son certificat est auto-signé ; voir [Redis](redis)) |
| MySQL / MariaDB | Un add-on MySQL de la marketplace Elements, qui place une URL `mysql://` dans une config var | `mysql` (ou `mysql-tls`) | `MySqlPool::connect(&url)` (voir [MySQL & MariaDB](mysql)) |
| MongoDB | Un cluster MongoDB Atlas ; copiez son URI `mongodb+srv://` dans une config var comme `MONGODB_URI` (**Settings → Config Vars**) | `mongodb` | `MongoDb::connect(&uri)` (voir [MongoDB](mongodb)) |

### 3. Construire et déployer

Ajoutez à votre dépôt un `Dockerfile` et un `heroku.yml`, comme le fait le dépôt de Memoire. Un `Dockerfile` minimal pour un binaire Rust appelé `my-app` :

```dockerfile
FROM rust:1-slim-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/my-app /usr/local/bin/my-app
CMD ["my-app"]
```

Et le `heroku.yml` :

```yaml
build:
  docker:
    web: Dockerfile
```

La prise en charge de TLS par Memoire est écrite en Rust pur (`rustls`) : l'image finale n'a donc pas besoin d'OpenSSL. Copiez ensuite `.github/workflows/deploy-heroku.yml` dans votre dépôt, ajoutez les deux secrets comme dans la méthode 2, et chaque push sur `master` déploiera votre application.

## Étapes suivantes

- [PostgreSQL](postgres) : chaînes de connexion, TLS et dimensionnement du pool.
- [Migrations](migrations) : gardez le schéma de production à jour à chaque déploiement.
- [Publier sur crates.io](publish-crates-io) : publiez une nouvelle version de la crate, là aussi depuis un téléphone.
