# Nuage DB

Nuage DB est la base de données de Memoire, écrite en Rust. Elle garde vos tables sur disque derrière un journal d'écriture anticipée (write-ahead log), fait en un instant des branches copy-on-write d'une base, se sauvegarde elle-même vers un second emplacement et sert le tout en HTTP(S), avec une console d'administration. C'est le stockage recommandé pour les nouveaux projets Memoire : les mêmes modèles et requêtes tournent sur Nuage DB embarqué dans votre application pendant le développement et les tests, et sur un serveur Nuage DB en production, sans changer une ligne.

Elle se compose de deux parties :

- la **crate `nuage`** : le moteur, la poignée embarquée (`nuage::Embedded`), le serveur et le binaire `nuage` ;
- la **feature `nuage` de memoire-orm** : `NuageDb`, le client d'un serveur Nuage DB, qui fonctionne aussi en WebAssembly dans le navigateur.

## Quand l'utiliser

- **Nouveaux projets** : une seule base pour le développement (embarquée, un dossier sur le disque) et la production (un serveur), avec le même code.
- **Prévisualisations et tests** : une branche est une copie complète de la base, faite en un instant, quelle que soit sa taille.
- **Navigateurs et fonctions edge** : `NuageDb` parle HTTP(S), donc du code WebAssembly atteint la base au moyen de `fetch`.
- **Petites équipes sans administrateur de bases** : points de reprise (checkpoints), sauvegardes, restaurations et compactage se font tout seuls.

Pour de nombreux serveurs d'application qui écrivent beaucoup dans une même base partagée, préférez [PostgreSQL](postgres). Nuage DB parle aussi SQL, pour les sites web et tous les langages (voir [SQL](#sql)).

## Installation

```bash
# Le moteur, embarqué, et le serveur
cargo add nuage --git https://github.com/maxlestage/Nuage
cargo add memoire-orm --git https://github.com/maxlestage/Memoire
cargo add tokio --features macros,rt-multi-thread

# Seulement le client d'un serveur (aussi pour wasm32)
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features nuage
```

## Embarqué : un dossier sur le disque

```rust
use memoire_orm::prelude::*;
use nuage::Embedded;

#[derive(Model, Debug, Clone)]
#[memoire(table = "notes")]
pub struct Note {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub text: String,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = Embedded::open("data")?; // créé s'il manque
    db.create_table_if_not_exists::<Note>().await?;
    let mut note = Note { id: 0, text: "bonjour".into() };
    note.insert(&db).await?;
    println!("{:?}", Note::query().all(&db).await?);
    Ok(())
}
```

`Embedded` implémente les traits `Executor` et `Database` de Memoire : modèles, constructeur de requêtes, relations, transactions (`db.begin()`, `db.transaction(...)`) et migrations fonctionnent comme avec tous les autres backends. Ouvrez un dossier une fois par processus et clonez la poignée (les clones partagent le moteur).

## Serveur et client

Lancez le serveur (ici en local, les données dans `./nuage-data`) :

```bash
git clone https://github.com/maxlestage/Nuage && cd Nuage
# La clé qui chiffre tout : gardez-la, les données sont illisibles sans elle.
export NUAGE_ENCRYPTION_KEY=$(openssl rand -hex 32)
NUAGE_TOKEN=dev-token NUAGE_ADMIN_PASSWORD=dev-password cargo run -p nuage --release
# nuage: data is stored in /home/me/app/nuage-data on this server's disk
# nuage: listening on 0.0.0.0:8080
```

Connectez-vous depuis votre application :

```rust
let db = NuageDb::connect("http://127.0.0.1:8080", "dev-token").await?;
Note::query().filter(Note::TEXT.contains("bonjour")).all(&db).await?;
```

`NuageDb` est dans `memoire_orm::prelude` avec la feature `nuage`. Il garde ses connexions HTTP ouvertes d'une requête à l'autre, utilise TLS (rustls) pour les URL `https://` et a besoin du runtime Tokio en natif. En WebAssembly, il utilise le `fetch` du navigateur et n'a besoin d'aucun runtime.

### Un seul réglage pour les deux

`nuage::Db` est l'un ou l'autre, choisi par la configuration :

```rust
// NUAGE_URL=data                                  -> embarqué, dans ./data
// NUAGE_URL=https://db.example.com NUAGE_TOKEN=…  -> un serveur
let db = nuage::Db::from_env().await?;
```

## Installer sur un serveur

Sur n'importe quel serveur Linux (Debian, Ubuntu, Fedora, Rocky, Alpine..., x86_64 ou ARM64, dès 512 Mio de mémoire), une commande :

```bash
curl -fsSL https://raw.githubusercontent.com/maxlestage/Nuage/master/install.sh | sudo bash
```

Elle installe le binaire `nuage` (compilé en statique, vérifié par son sha256), **crée un swap** si le serveur n'en a pas (la taille de la mémoire, de 1 à 4 Gio, avec `vm.swappiness=10` pour que la mémoire aille à la base), un utilisateur `nuage`, les données dans `/var/lib/nuage`, les réglages dans `/etc/nuage/nuage.env` avec une clé, un jeton et un mot de passe de console générés, et un service systemd lancé au démarrage. Elle affiche une fois l'adresse, le jeton et le mot de passe : **copiez `/etc/nuage/nuage.env` en lieu sûr**, personne ne peut lire les données sans sa clé.

- **HTTPS** : faites pointer un nom de domaine vers le serveur, puis lancez `curl -fsSL .../install.sh | sudo NUAGE_DOMAIN=db.example.com bash` (Caddy, avec un certificat gratuit).
- **Mise à jour** : relancez la commande ; données, clé et réglages restent.
- **Options** : `NUAGE_PORT`, `NUAGE_SWAP_MB` (`0` : pas de swap), `NUAGE_VERSION=v0.3.0`, `NUAGE_FROM_SOURCE=1`.
- **Désinstaller** : `... | sudo bash -s uninstall` (garde les données), ou `purge`.
- **Docker** : `docker compose up -d` dans le dépôt Nuage, avec les réglages dans `.env`.

Des binaires pour Linux, macOS et Windows sont sur la [page des versions](https://github.com/maxlestage/Nuage/releases).

## Mémoire

Le cache de pages de la base de chaque application est la seule mémoire que Nuage DB garde exprès ; les données restent sur disque. `NUAGE_CACHE_MB=auto` (par défaut) lui donne un huitième de la mémoire, de 8 à 512 Mio, limite du conteneur comprise : un serveur de 512 Mio garde l'essentiel de sa mémoire libre. La page **Serveur** de la console et `/api/status` (`memory`) montrent la mémoire utilisée par Nuage DB, la mémoire libre et le swap.

## Chiffrement

Nuage DB chiffre tout ce qu'il stocke : les pages en XTS-AES-256, chaque enregistrement du journal en AES-256-GCM, et les sauvegardes, copies des deux. `NUAGE_ENCRYPTION_KEY` est une clé (`nuage genkey`, `openssl rand -hex 32`) ou une phrase de passe (Argon2id) ; le serveur ne démarre pas sans elle, et une mauvaise clé est refusée avant toute lecture. **Gardez la clé** : sans elle, ni les données ni les sauvegardes ne sont lisibles. En mode embarqué, passez `EngineOptions { encryption: Some(nuage::Encryption::parse(cle)?), .. }` à `Embedded::open_with`, ou utilisez `nuage::Db::from_env()`, qui lit `NUAGE_ENCRYPTION_KEY`.

## Plusieurs applications

Un serveur héberge plusieurs applications, chacune avec sa base, ses clés, ses sauvegardes et ses jetons. La première est la base du serveur (`NUAGE_NAME`, `NUAGE_TOKEN`) ; la console (**Nouvelle application**) crée les autres et affiche chaque nouveau jeton une fois (le serveur n'en garde que l'empreinte SHA-256). Le jeton qu'envoie une application choisit sa base : `NuageDb::connect(url, jeton)` ne change pas, et une application ne voit jamais les données d'une autre. Les jetons en lecture seule et les sites autorisés à appeler le serveur depuis le navigateur (CORS) se règlent par application.

## SQL

Nuage DB parle SQL : `POST /v1/sql` depuis n'importe quel langage, l'onglet **Requête** de la console, et `Embedded::sql` en Rust. `SELECT` avec jointures, `GROUP BY`, `HAVING`, `UNION`, sous-requêtes, `CASE` et les fonctions usuelles ; `INSERT ... ON CONFLICT`, `UPDATE`, `DELETE`, `RETURNING` ; `CREATE TABLE`, `CREATE INDEX`, `ALTER TABLE ... ADD COLUMN`, `DROP`. Les paramètres (`?` ou `$1`) ne sont jamais lus comme du SQL. Plusieurs instructions dans une requête s'exécutent dans une seule transaction.

```bash
curl https://db.example.com/v1/sql -H "Authorization: Bearer $NUAGE_TOKEN" \
  -d '{"sql": "SELECT id, text FROM notes WHERE text LIKE ? ORDER BY id DESC LIMIT 20", "params": ["%hello%"]}'
```

```rust
let page = db.sql("SELECT id, upper(text) AS t FROM notes ORDER BY id DESC LIMIT 10", &[])?;
```

`db.raw(...)` et les étapes de migration en SQL de Memoire répondent toujours `Error::Unsupported` sur Nuage DB : depuis Rust, utilisez les modèles et le constructeur de requêtes, ou `Embedded::sql`.

## Transactions

Sur un serveur, `db.begin()` ouvre une transaction interactive : le serveur la garde d'une requête à l'autre et l'annule si elle reste inactive 30 secondes. Une transaction lit un instantané pris à son début, plus ses propres changements. Elle est optimiste : quand un autre commit a modifié une table qu'elle a utilisée, son commit échoue avec `Error::Conflict`, et `db.transaction(...)` relance la closure.

Pour envoyer plusieurs instructions en une seule requête, tout ou rien, utilisez un lot :

```rust
let results = db.batch()
    .execute(Query::InsertRow { table: Note::SCHEMA, values: &note.to_values() })
    .fetch(Query::Select(Note::query().ast()))
    .run()
    .await?;
```

## Branches

Une branche est une copie de la base pour une prévisualisation, un test ou une expérience. La créer est instantané : elle partage toutes les pages de son parent et ne stocke que ses propres changements. Les changements ultérieurs du parent ne l'atteignent pas.

```rust
let preview = db.create_branch("preview-42", None).await?; // depuis la base principale
let on_preview = db.on_branch("preview-42");
Note { id: 0, text: "seulement dans la préversion".into() }.insert(&on_preview).await?;

db.reset_branch("preview-42").await?;  // oublier ses changements
db.delete_branch("preview-42").await?;
```

En mode embarqué, les mêmes opérations sont sur le moteur : `db.engine().create_branch(...)` et `Embedded::on_branch`. Une branche peut avoir ses propres branches ; supprimez-les avant leur parent.

## La console

Définissez `NUAGE_ADMIN_PASSWORD` et ouvrez `/admin` :

- **Vue d'ensemble** : taille, tables, lignes, branches, temps de fonctionnement, connexions, cache, dernier checkpoint, sauvegardes ;
- **Tables** : nombre de lignes et taille, et les lignes de chaque table, page par page ;
- **Requête** : SQL, avec une confirmation avant les instructions qui modifient des données ;
- **Branches** : liste, création, remise à zéro, suppression ;
- **Stockage** : usage du disque, journal, état des sauvegardes, et des boutons pour un checkpoint, une sauvegarde, une vérification d'intégrité et un compactage.

Les pages sont rendues par le serveur et fonctionnent sans JavaScript, sur téléphone aussi, en thème clair et sombre, en français, anglais et espagnol. Le cookie de session est `HttpOnly`, `SameSite=Strict` et, en HTTPS ou sur `localhost`, `Secure` ; chaque formulaire porte un jeton CSRF, et les échecs de connexion sont limités (5 par adresse et 30 au total par 15 minutes). En HTTP simple (un serveur tout juste installé, joint par son IP), la console fonctionne, mais sa page de connexion prévient que le mot de passe circule en clair : servez-la en HTTPS. Un lien vers `/admin?next=/une/page` revient à cette page du même site après la connexion ; les autres cibles (un autre site) mènent à `/admin`.

## État en direct

| Point d'accès | Accès | Contenu |
|---|---|---|
| `GET /api/health` | public | `{"status":"ok"}` |
| `GET /api/status` | jeton ou session de la console | requêtes par seconde, latence p50/p99, connexions, stockage, taux de succès du cache, journal, sauvegarde, bases et branches, services |
| `GET /api/status/stream` | idem | le même JSON chaque seconde en Server-Sent Events (`event: status`), plus `event: log` pour les checkpoints, sauvegardes et branches |

Avec `NUAGE_PUBLIC_STATUS=true`, les requêtes sans identifiants reçoivent un sous-ensemble public : état, temps de fonctionnement, et le nom et l'état de chaque service.

## Comment ça marche

- **Pages sur disque.** Tables et index vivent dans un seul fichier, un stockage en B-arbres ([redb](https://docs.rs/redb), 100 % Rust) avec un cache de pages borné (`NUAGE_CACHE_MB`, par défaut un huitième de la mémoire, de 8 à 512 Mio). Les données ne sont pas limitées par la mémoire, et le fichier grandit avec elles.
- **Journal d'écriture anticipée.** Chaque commit est ajouté au journal et synchronisé sur le disque (`fsync`) avant d'être appliqué. Un checkpoint, toutes les 30 secondes ou tous les 16 Mio de journal, rend le stockage des pages durable et commence un nouveau segment de journal. Après un crash, le démarrage suivant rejoue ce que le dernier checkpoint n'avait pas. Les enregistrements du journal portent un CRC-32 et les pages une somme de contrôle : les dégâts sont détectés, et un enregistrement coupé par le crash est écarté.
- **Instructions.** Le moteur ne lit que les lignes dont une instruction a besoin : les recherches par clé primaire et par index lisent les lignes correspondantes, les autres filtres parcourent la table par morceaux, `LIMIT` s'arrête tôt et `ORDER BY ... LIMIT n` garde les `n` meilleures lignes au fil de la lecture ; `COUNT(*)` lit un compteur. L'évaluateur de Memoire exécute ensuite l'instruction sur ces lignes : Nuage DB se comporte exactement comme le moteur en mémoire.
- **Entretien.** Un thread d'arrière-plan fait les checkpoints, envoie les sauvegardes et compacte le fichier quand un quart en est de l'espace libre.

## Sauvegardes et restauration

Les sauvegardes vont dans un dossier (`NUAGE_BACKUP_DIR`) ou vers un stockage compatible S3 (`NUAGE_BACKUP_S3_*` : AWS S3, Cloudflare R2, MinIO, Scaleway...). Une sauvegarde est un instantané du stockage des pages (toutes les heures par défaut) plus les segments de journal écrits depuis, envoyés à chaque checkpoint. Quand le serveur démarre avec un dossier de données vide et un emplacement de sauvegarde, il restaure le dernier instantané et rejoue les segments suivants avant de servir.

Les commits faits depuis le dernier segment envoyé ne sont pas encore dans la sauvegarde : au plus un intervalle de checkpoint, 30 secondes par défaut. Sur `SIGTERM` (ce que les hébergeurs envoient avant un redémarrage), le serveur fait un checkpoint et envoie le dernier segment avant de s'arrêter.

## Déployer sur n'importe quel hébergeur

Le serveur est un seul binaire, configuré par des variables d'environnement :

| Variable | Rôle |
|---|---|
| `PORT` | port (par défaut `8080`) |
| `NUAGE_ENCRYPTION_KEY` | la clé qui chiffre tout ; **obligatoire** |
| `NUAGE_BIND` | adresse d'écoute (par défaut `0.0.0.0` ; `127.0.0.1` derrière un proxy) |
| `NUAGE_DATA_DIR` | dossier des données (par défaut `nuage-data`) |
| `NUAGE_NAME` | nom de la base (par défaut `nuage`) |
| `NUAGE_TOKEN` | jeton de l'API de la base ; sans lui, l'API refuse toute requête |
| `NUAGE_ADMIN_PASSWORD` | mot de passe de la console ; sans lui, la console est désactivée |
| `NUAGE_PUBLIC_STATUS` | `true` pour le sous-ensemble public de l'état |
| `NUAGE_CACHE_MB` | cache de pages de chaque application en Mio, ou `auto` (par défaut) |
| `NUAGE_BACKUP_DIR` | sauvegardes dans un dossier |
| `NUAGE_BACKUP_S3_BUCKET`, `_REGION`, `_ENDPOINT`, `_PREFIX`, `_ACCESS_KEY_ID`, `_SECRET_ACCESS_KEY` | sauvegardes vers un stockage compatible S3 |
| `NUAGE_BACKUP_SNAPSHOT_MINUTES` | minutes entre deux instantanés (par défaut 60) |

La racine du dépôt Nuage contient `install.sh` (ci-dessus), un `Dockerfile` (les données dans un volume `/data`) et `docker-compose.yml`, un `nuage.env.example` commenté et les fichiers Heroku ci-dessous. Placez le serveur derrière HTTPS (la plupart des hébergeurs le font pour vous ; sur votre serveur, `NUAGE_DOMAIN` de l'installateur) : le jeton voyage dans un en-tête.

## Déployer sur Heroku

Heroku fait tourner le serveur sur un dyno **Basic**. Le disque d'un dyno est **éphémère** : il est effacé à chaque redémarrage (au moins une fois par jour, et à chaque déploiement). Le dossier des données n'est qu'une copie de travail ; **les données vivent dans votre bucket de sauvegarde**, et chaque démarrage les restaure depuis là. Les sauvegardes vers un stockage compatible S3 sont donc obligatoires sur Heroku.

1. Créez l'application avec le buildpack Rust :

   ```bash
   heroku create my-nuage --buildpack emk/rust
   ```

2. Déployez le dépôt Nuage (https://github.com/maxlestage/Nuage) tel quel : sa racine contient déjà le `Procfile`, `RustConfig`, `heroku-build.sh` et `app.json` (pour un bouton « Deploy to Heroku »). Le `Procfile` lance `./target/release/nuage`.

3. Créez un bucket (AWS S3, Cloudflare R2...) et une clé d'accès limitée à lui, puis définissez les config vars :

   ```bash
   heroku config:set NUAGE_ENCRYPTION_KEY=$(openssl rand -hex 32) NUAGE_TOKEN=$(openssl rand -hex 32) \
     NUAGE_ADMIN_PASSWORD='une longue phrase de passe' \
     NUAGE_NAME=nuage \
     NUAGE_BACKUP_S3_BUCKET=my-bucket NUAGE_BACKUP_S3_REGION=eu-west-3 \
     NUAGE_BACKUP_S3_ACCESS_KEY_ID=... NUAGE_BACKUP_S3_SECRET_ACCESS_KEY=...
   ```

   Pour un autre fournisseur qu'AWS, définissez aussi `NUAGE_BACKUP_S3_ENDPOINT`.

4. Déployez (`git push heroku main`, ou l'intégration GitHub), puis ouvrez `https://my-nuage.herokuapp.com/admin` et connectez-vous avec `NUAGE_ADMIN_PASSWORD`. Les applications se connectent avec `NuageDb::connect("https://my-nuage.herokuapp.com", token)`.

À savoir sur Heroku :

- **Où sont les données.** Sur le disque du dyno pendant qu'il tourne, et dans le bucket pour de bon. Le journal de démarrage indique les deux. Après un redémarrage, les premières requêtes attendent la restauration, qui dure le temps de télécharger le dernier instantané.
- **Fenêtre de perte.** Les commits depuis le dernier segment envoyé (30 secondes par défaut) sont perdus si le dyno s'arrête sans le `SIGTERM` qu'Heroku envoie normalement d'abord.
- **Un seul dyno.** Faites tourner exactement un dyno web : deux dynos seraient deux bases distinctes, restaurées depuis le même bucket et écrivant dedans.
- **Les dynos Eco** s'endorment après 30 minutes sans trafic et se réveillent à la requête suivante : cela convient au « scale to zero » quand l'attente de la restauration au réveil est acceptable. Les dynos Basic ne s'endorment jamais.
- **Mémoire et disque.** Le cache de pages (`NUAGE_CACHE_MB`) doit tenir dans la mémoire du dyno à côté du serveur ; le disque du dyno doit contenir le dossier des données.
- **Les branches** sont stockées dans le même fichier : elles sont sauvegardées et restaurées avec la base principale.

## Limites

- **SQL depuis Memoire.** `db.raw(...)` et les étapes de migration en SQL répondent `Error::Unsupported` (comme sur le moteur en mémoire) ; le SQL passe par `/v1/sql` ou `Embedded::sql`. Absents : jointures `RIGHT`/`FULL`, `WITH`, fonctions de fenêtrage, vues, déclencheurs, clés primaires de plusieurs colonnes.
- **Un seul écrivain à la fois.** Les écritures sont appliquées l'une après l'autre ; les lectures tournent en parallèle. Un serveur par base.
- **Les jointures, `GROUP BY` et agrégats** autres que `COUNT(*)` chargent en mémoire les lignes qu'ils combinent (après les filtres propres à chaque table ; en SQL, jointures par hachage). Les index servent aux égalités et aux listes `IN` sur la clé primaire et les colonnes `unique` et `index` ; les intervalles parcourent la table.
- **Les index** sont ceux du modèle à la création de la table, plus ceux créés par `CREATE INDEX` (construits à partir des lignes existantes). Les nouvelles colonnes ajoutées à un modèle sont stockées à partir de là (les lignes existantes lisent `NULL`) ; `ALTER TABLE ... ADD COLUMN` remplit les lignes existantes avec la valeur par défaut.
- **Branches.** Une écriture dans une branche qui a ses propres branches copie d'abord l'ancienne valeur dans chacune d'elles. Supprimer une table dans une branche qui a des branches garde ses lignes jusqu'à la suppression de ces branches.
- **Les sauvegardes** copient tout le stockage des pages à chaque instantané (les écritures attendent pendant la copie locale), et l'envoi vers S3 garde l'instantané en mémoire.
- **WebAssembly** : le client seulement ; le moteur a besoin d'un système de fichiers.

## Pour aller plus loin

- [Transactions](transactions) : l'assistant de transaction, les relances sur conflit.
- [Migrations](migrations) : les changements de schéma versionnés, qui tournent aussi sur Nuage DB.
- [Déployer sur Heroku depuis son téléphone](deploy-heroku-mobile) : le tableau de bord Heroku et les workflows GitHub, pas à pas.
