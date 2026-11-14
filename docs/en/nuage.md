# Nuage DB

Nuage DB is Memoire's own database, written in Rust. It keeps your tables on disk behind a write-ahead log, makes instant copy-on-write branches of a database, backs itself up to a second location and serves everything over HTTP(S), with a management console. It is the recommended storage for new Memoire projects: the same models and queries run on it embedded in your application during development and tests, and on a Nuage DB server in production, without changing a line.

It comes in two parts:

- the **`nuage` crate**: the engine, the embedded handle (`nuage::Embedded`), the server and the `nuage` binary;
- the **`nuage` feature of memoire-orm**: `NuageDb`, the client of a Nuage DB server, which also works in WebAssembly in the browser.

## When to use it

- **New projects**: one database for development (embedded, a directory on disk) and production (a server), with the same code.
- **Previews and tests**: a branch is a full copy of the database made in an instant, whatever its size.
- **Browsers and edge functions**: `NuageDb` speaks HTTP(S), so WebAssembly code can reach the database through `fetch`.
- **Small teams without a database administrator**: checkpoints, backups, restores and compaction run on their own.

For many application servers writing heavily to one shared database, prefer [PostgreSQL](postgres). Nuage DB also speaks SQL, for websites and any language (see [SQL](#sql)).

## Install

```bash
# The engine, embedded, and the server
cargo add nuage --git https://github.com/maxlestage/Nuage
cargo add memoire-orm --git https://github.com/maxlestage/Memoire
cargo add tokio --features macros,rt-multi-thread

# Only the client of a server (also for wasm32)
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features nuage
```

## Embedded: a directory on disk

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
    let db = Embedded::open("data")?; // created if missing
    db.create_table_if_not_exists::<Note>().await?;
    let mut note = Note { id: 0, text: "hello".into() };
    note.insert(&db).await?;
    println!("{:?}", Note::query().all(&db).await?);
    Ok(())
}
```

`Embedded` implements Memoire's `Executor` and `Database` traits: models, the query builder, relations, transactions (`db.begin()`, `db.transaction(...)`) and migrations work as with every other backend. Open a directory once per process and clone the handle (clones share the engine).

## Server and client

Run the server (here locally, the data in `./nuage-data`):

```bash
git clone https://github.com/maxlestage/Nuage && cd Nuage
# The key everything is encrypted with: keep it, the data cannot be read without it.
export NUAGE_ENCRYPTION_KEY=$(openssl rand -hex 32)
NUAGE_TOKEN=dev-token NUAGE_ADMIN_PASSWORD=dev-password cargo run -p nuage --release
# nuage: data is stored in /home/me/app/nuage-data on this server's disk
# nuage: listening on 0.0.0.0:8080
```

Connect from your application:

```rust
let db = NuageDb::connect("http://127.0.0.1:8080", "dev-token").await?;
Note::query().filter(Note::TEXT.contains("hello")).all(&db).await?;
```

`NuageDb` is in `memoire_orm::prelude` with the `nuage` feature. It keeps its HTTP connections alive between requests, uses TLS (rustls) for `https://` URLs and needs the Tokio runtime natively. In WebAssembly it uses the browser's `fetch` and needs no runtime.

### One setting for both

`nuage::Db` is either kind, picked from configuration:

```rust
// NUAGE_URL=data                                  -> embedded, in ./data
// NUAGE_URL=https://db.example.com NUAGE_TOKEN=…  -> a server
let db = nuage::Db::from_env().await?;
```

## Install on a server

On any Linux server (Debian, Ubuntu, Fedora, Rocky, Alpine..., x86_64 or ARM64, from 512 MiB of memory), one command:

```bash
curl -fsSL https://raw.githubusercontent.com/maxlestage/Nuage/master/install.sh | sudo bash
```

It installs the `nuage` binary (a static build, checked with its sha256), **creates swap** when the server has none (the size of the memory, 1 to 4 GiB, with `vm.swappiness=10` so the memory goes to the database), a `nuage` user, the data in `/var/lib/nuage`, the settings in `/etc/nuage/nuage.env` with a generated key, token and console password, and a systemd service started at boot. It prints the address, the token and the password once: **copy `/etc/nuage/nuage.env` somewhere safe**, nobody can read the data without its key.

- **HTTPS**: point a domain name to the server, then run `curl -fsSL .../install.sh | sudo NUAGE_DOMAIN=db.example.com bash` (Caddy, with a free certificate).
- **Upgrade**: run the command again; data, key and settings stay.
- **Options**: `NUAGE_PORT`, `NUAGE_SWAP_MB` (`0`: no swap), `NUAGE_VERSION=v0.3.0`, `NUAGE_FROM_SOURCE=1`.
- **Remove**: `... | sudo bash -s uninstall` (keeps the data), or `purge`.
- **Docker**: `docker compose up -d` in the Nuage repository, with the settings in `.env`.

Binaries for Linux, macOS and Windows are on the [releases page](https://github.com/maxlestage/Nuage/releases).

## Memory

The page cache of each application's database is the only memory Nuage DB keeps on purpose; the data stays on disk. `NUAGE_CACHE_MB=auto` (the default) gives it an eighth of the memory, between 8 and 512 MiB, the container's limit included: a 512 MiB server keeps most of its memory free. The console's **Server** page and `/api/status` (`memory`) show the memory Nuage DB uses, the free memory and the swap.

## Encryption

Nuage DB encrypts everything it stores: the pages with XTS-AES-256, each write-ahead log record with AES-256-GCM, and the backups, copies of both. `NUAGE_ENCRYPTION_KEY` is a key (`nuage genkey`, `openssl rand -hex 32`) or a passphrase (Argon2id); the server does not start without it, and a wrong key is refused before anything is read. **Keep the key**: without it, neither the data nor the backups can be read. Embedded, pass `EngineOptions { encryption: Some(nuage::Encryption::parse(key)?), .. }` to `Embedded::open_with`, or use `nuage::Db::from_env()`, which reads `NUAGE_ENCRYPTION_KEY`.

## Several applications

One server holds several applications, each with its own database, keys, backups and tokens. The first one is the server's own database (`NUAGE_NAME`, `NUAGE_TOKEN`); the console (**New application**) makes the others and shows each new token once (the server keeps only its SHA-256 digest). The token an application sends picks its database: `NuageDb::connect(url, token)` does not change, and an application never sees another's data. Read-only tokens and the websites allowed to call the server from the browser (CORS) are set per application.

## SQL

Nuage DB speaks SQL: `POST /v1/sql` from any language, the console's **Query** tab, and `Embedded::sql` in Rust. `SELECT` with joins, `GROUP BY`, `HAVING`, `UNION`, subqueries, `CASE` and the usual functions; `INSERT ... ON CONFLICT`, `UPDATE`, `DELETE`, `RETURNING`; `CREATE TABLE`, `CREATE INDEX`, `ALTER TABLE ... ADD COLUMN`, `DROP`. Parameters (`?` or `$1`) are never parsed as SQL. Several statements in one request run in one transaction.

```bash
curl https://db.example.com/v1/sql -H "Authorization: Bearer $NUAGE_TOKEN" \
  -d '{"sql": "SELECT id, text FROM notes WHERE text LIKE ? ORDER BY id DESC LIMIT 20", "params": ["%hello%"]}'
```

```rust
let page = db.sql("SELECT id, upper(text) AS t FROM notes ORDER BY id DESC LIMIT 10", &[])?;
```

`db.raw(...)` and SQL migration steps of Memoire still answer `Error::Unsupported` on Nuage DB: from Rust, use models and the query builder, or `Embedded::sql`.

## Transactions

On a server, `db.begin()` opens an interactive transaction: the server keeps it between requests and rolls it back if it stays idle for 30 seconds. Transactions read a snapshot taken when they begin, plus their own changes. They are optimistic: when another commit changed a table they used, the commit fails with `Error::Conflict`, and `db.transaction(...)` runs the closure again.

To send several statements in one request, all or nothing, use a batch:

```rust
let results = db.batch()
    .execute(Query::InsertRow { table: Note::SCHEMA, values: &note.to_values() })
    .fetch(Query::Select(Note::query().ast()))
    .run()
    .await?;
```

## Branches

A branch is a copy of the database for a preview, a test or an experiment. Making one is instant: it shares every page of its parent and stores only its own changes. Later changes of the parent do not reach it.

```rust
let preview = db.create_branch("preview-42", None).await?; // from the main database
let on_preview = db.on_branch("preview-42");
Note { id: 0, text: "only in the preview".into() }.insert(&on_preview).await?;

db.reset_branch("preview-42").await?;  // forget its changes
db.delete_branch("preview-42").await?;
```

Embedded, the same operations are on the engine: `db.engine().create_branch(...)` and `Embedded::on_branch`. A branch can have branches of its own; delete them before their parent.

## The console

Set `NUAGE_ADMIN_PASSWORD` and open `/admin`:

- **Overview**: size, tables, rows, branches, uptime, connections, cache, last checkpoint, backups;
- **Tables**: row counts and sizes, and the rows of each table, page by page;
- **Query**: SQL, with a confirmation before statements that change data;
- **Branches**: list, create, reset, delete;
- **Storage**: disk use, write-ahead log, backup status, and buttons for a checkpoint, a backup, an integrity check and a compaction.

The pages are rendered by the server and work without JavaScript, on a phone too, in light and dark mode, in English, French and Spanish. The session cookie is `HttpOnly`, `SameSite=Strict` and, over HTTPS or on `localhost`, `Secure`; every form carries a CSRF token, and failed logins are limited (5 per address and 30 in all per 15 minutes). Over plain HTTP (a server just installed, reached by its IP) the console works, but its login page warns that the password travels in clear: serve it over HTTPS. A link to `/admin?next=/some/page` returns to that page of the same site after the login; other targets (another site) go to `/admin`.

## Live status

| Endpoint | Access | Content |
|---|---|---|
| `GET /api/health` | public | `{"status":"ok"}` |
| `GET /api/status` | token or console session | requests per second, p50/p99 latency, connections, storage, cache hit rate, write-ahead log, backup, databases and branches, services |
| `GET /api/status/stream` | same | the same JSON every second as Server-Sent Events (`event: status`), plus `event: log` for checkpoints, backups and branches |

With `NUAGE_PUBLIC_STATUS=true`, requests without credentials get a public subset: status, uptime and the name and status of each service.

## How it works

- **Pages on disk.** Tables and indexes live in one file of B-tree pages that Nuage DB writes itself (copy-on-write, a checksum on each node, pure Rust), with a bounded page cache (`NUAGE_CACHE_MB`, by default an eighth of the memory, 8 to 512 MiB). The data is not limited by memory, and the file grows with it.
- **Write-ahead log.** Each commit is appended to the log and synced to disk (`fsync`) before it is applied. A checkpoint, every 30 seconds or 16 MiB of log, makes the page store durable and starts a new log segment. After a crash, the next start replays what the last checkpoint missed. Log records carry a CRC-32 and pages a checksum: damage is detected, and a record torn by the crash is dropped.
- **Statements.** The engine reads only the rows a statement needs: primary-key and index lookups read the matching rows, other filters stream the table in chunks, `LIMIT` stops early and `ORDER BY ... LIMIT n` keeps the best `n` rows while streaming; `COUNT(*)` reads a counter. Memoire's own evaluator then runs the statement on those rows, so Nuage DB behaves exactly like the in-memory engine.
- **Upkeep.** A background thread runs the checkpoints, ships the backups and compacts the file when a quarter of it is free space.

## Backups and restore

Backups go to a directory (`NUAGE_BACKUP_DIR`) or to S3-compatible storage (`NUAGE_BACKUP_S3_*`: AWS S3, Cloudflare R2, MinIO, Scaleway...). A backup is a snapshot of the page store (every hour by default) plus the log segments written since, shipped at each checkpoint. When the server starts with an empty data directory and a backup location, it restores the latest snapshot and replays the segments after it before serving.

Commits made since the last shipped segment are not in the backup yet: at most one checkpoint interval, 30 seconds by default. On `SIGTERM` (what hosts send before a restart) the server checkpoints and ships the last segment before stopping.

## Deploy on any host

The server is one binary configured by environment variables:

| Variable | Meaning |
|---|---|
| `PORT` | port (default `8080`) |
| `NUAGE_ENCRYPTION_KEY` | the key everything is encrypted with; **required** |
| `NUAGE_BIND` | address to listen on (default `0.0.0.0`; `127.0.0.1` behind a proxy) |
| `NUAGE_DATA_DIR` | data directory (default `nuage-data`) |
| `NUAGE_NAME` | name of the database (default `nuage`) |
| `NUAGE_TOKEN` | token of the database API; without it, the API refuses every request |
| `NUAGE_ADMIN_PASSWORD` | password of the console; without it, the console is off |
| `NUAGE_PUBLIC_STATUS` | `true` for the public status subset |
| `NUAGE_CACHE_MB` | page cache of each application in MiB, or `auto` (the default) |
| `NUAGE_BACKUP_DIR` | backups to a directory |
| `NUAGE_BACKUP_S3_BUCKET`, `_REGION`, `_ENDPOINT`, `_PREFIX`, `_ACCESS_KEY_ID`, `_SECRET_ACCESS_KEY` | backups to S3-compatible storage |
| `NUAGE_BACKUP_SNAPSHOT_MINUTES` | minutes between snapshots (default 60) |

The root of the Nuage repository holds `install.sh` (above), a `Dockerfile` (the data in a `/data` volume) and `docker-compose.yml`, a commented `nuage.env.example`, and the Heroku files below. Put the server behind HTTPS (most hosts do it for you; on your own server, the installer's `NUAGE_DOMAIN`): the token travels in a header.

## Deploy on Heroku

Heroku runs the server on one **Basic** dyno. A dyno's disk is **ephemeral**: it is erased at every restart (at least once a day, and at each deploy). The data directory is only a working copy; **the data lives in your backup bucket**, and each start restores it from there. Backups to S3-compatible storage are therefore required on Heroku.

1. Create the app with the Rust buildpack:

   ```bash
   heroku create my-nuage --buildpack emk/rust
   ```

2. Deploy the Nuage repository (https://github.com/maxlestage/Nuage) as is: its root already holds the `Procfile`, `RustConfig`, `heroku-build.sh` and `app.json` (for a "Deploy to Heroku" button). The `Procfile` starts `./target/release/nuage`.

3. Create a bucket (AWS S3, Cloudflare R2...) and an access key limited to it, then set the config vars:

   ```bash
   heroku config:set NUAGE_ENCRYPTION_KEY=$(openssl rand -hex 32) NUAGE_TOKEN=$(openssl rand -hex 32) \
     NUAGE_ADMIN_PASSWORD='a long passphrase' \
     NUAGE_NAME=nuage \
     NUAGE_BACKUP_S3_BUCKET=my-bucket NUAGE_BACKUP_S3_REGION=eu-west-3 \
     NUAGE_BACKUP_S3_ACCESS_KEY_ID=... NUAGE_BACKUP_S3_SECRET_ACCESS_KEY=...
   ```

   For a provider other than AWS, set `NUAGE_BACKUP_S3_ENDPOINT` too.

4. Deploy (`git push heroku main`, or the GitHub integration), then open `https://my-nuage.herokuapp.com/admin` and log in with `NUAGE_ADMIN_PASSWORD`. Applications connect with `NuageDb::connect("https://my-nuage.herokuapp.com", token)`.

Things to know on Heroku:

- **Where the data is.** On the dyno's disk while it runs, and in the bucket for good. The startup log says both. After a restart, the first requests wait for the restore, which takes as long as downloading the latest snapshot.
- **Data loss window.** Commits since the last shipped segment (30 seconds by default) are lost if the dyno stops without the `SIGTERM` Heroku normally sends first.
- **One dyno.** Run exactly one web dyno: two dynos would be two separate databases restoring from, and writing to, the same bucket.
- **Eco dynos** sleep after 30 minutes without traffic and wake up at the next request: a fit for "scale to zero" when the wait for the restore at wake-up is acceptable. Basic dynos never sleep.
- **Memory and disk.** The page cache (`NUAGE_CACHE_MB`) must fit in the dyno's memory beside the server; the dyno's disk must hold the data directory.
- **Branches** are stored in the same file, so they are backed up and restored with the main database.

## Limits

- **SQL from Memoire.** `db.raw(...)` and SQL migration steps answer `Error::Unsupported` (as on the in-memory engine); SQL goes through `/v1/sql` or `Embedded::sql`. Not there: `RIGHT`/`FULL` joins, `WITH`, window functions, views, triggers, primary keys of several columns.
- **One writer at a time.** Writes are applied one after another; reads run in parallel. One server per database.
- **Joins, `GROUP BY` and aggregates** other than `COUNT(*)` load the rows they combine into memory (after the filters that apply to each table; SQL joins are hash joins). Index lookups are used for equalities and `IN` lists on the primary key, `unique` and `index` columns; ranges scan the table.
- **Indexes** are those of the model when the table was created, plus those made with `CREATE INDEX` (built from the existing rows). New columns added to a model are stored from then on (existing rows read `NULL`); `ALTER TABLE ... ADD COLUMN` fills existing rows with the default.
- **Branches.** A write in a branch that has branches of its own first copies the old value into each of them. Dropping a table in a branch that has branches keeps its rows until those branches are deleted.
- **Backups** copy the whole page store for each snapshot (writes wait during the local copy), and the S3 upload holds the snapshot in memory.
- **WebAssembly**: the client only; the engine needs a file system.

## Next steps

- [Transactions](transactions): the transaction helper, retries on conflict.
- [Migrations](migrations): versioned schema changes, which run on Nuage DB too.
- [Deploy to Heroku from your phone](deploy-heroku-mobile): the Heroku dashboard and GitHub workflows, step by step.
