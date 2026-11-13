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

For many application servers writing heavily to one shared database, or for SQL written by hand, prefer [PostgreSQL](postgres): Nuage DB runs Memoire statements, not SQL (see [Limits](#limits)).

## Install

```bash
# The engine, embedded, and the server
cargo add nuage --git https://github.com/maxlestage/Memoire
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
- **Query**: a console for a small subset of SQL (`SELECT`, `UPDATE`, `DELETE` on one table, with `WHERE`, `ORDER BY`, `LIMIT`, `OFFSET`). Statements that change data show how many rows they would touch and wait for a confirmation;
- **Branches**: list, create, reset, delete;
- **Storage**: disk use, write-ahead log, backup status, and buttons for a checkpoint, a backup, an integrity check and a compaction.

The pages are rendered by the server and work without JavaScript, on a phone too, in light and dark mode. The session cookie is `HttpOnly`, `Secure` and `SameSite=Strict`, every form carries a CSRF token, and failed logins are limited (5 per address and 30 in all per 15 minutes). Browsers keep a `Secure` cookie only over HTTPS (or on `localhost`): serve the console over HTTPS. A link to `/admin?next=/some/page` returns to that page of the same site after the login; other targets (another site) go to `/admin`.

## Live status

| Endpoint | Access | Content |
|---|---|---|
| `GET /api/health` | public | `{"status":"ok"}` |
| `GET /api/status` | token or console session | requests per second, p50/p99 latency, connections, storage, cache hit rate, write-ahead log, backup, databases and branches, services |
| `GET /api/status/stream` | same | the same JSON every second as Server-Sent Events (`event: status`), plus `event: log` for checkpoints, backups and branches |

With `NUAGE_PUBLIC_STATUS=true`, requests without credentials get a public subset: status, uptime and the name and status of each service.

## How it works

- **Pages on disk.** Tables and indexes live in one file, a B-tree store ([redb](https://docs.rs/redb), pure Rust) with a bounded page cache (`NUAGE_CACHE_MB`, 64 MiB by default). The data is not limited by memory, and the file grows with it.
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
| `NUAGE_DATA_DIR` | data directory (default `nuage-data`) |
| `NUAGE_NAME` | name of the database (default `nuage`) |
| `NUAGE_TOKEN` | token of the database API; without it, the API refuses every request |
| `NUAGE_ADMIN_PASSWORD` | password of the console; without it, the console is off |
| `NUAGE_PUBLIC_STATUS` | `true` for the public status subset |
| `NUAGE_CACHE_MB` | page cache in MiB |
| `NUAGE_BACKUP_DIR` | backups to a directory |
| `NUAGE_BACKUP_S3_BUCKET`, `_REGION`, `_ENDPOINT`, `_PREFIX`, `_ACCESS_KEY_ID`, `_SECRET_ACCESS_KEY` | backups to S3-compatible storage |
| `NUAGE_BACKUP_SNAPSHOT_MINUTES` | minutes between snapshots (default 60) |

`deploy/` in the Nuage repository holds a `Dockerfile` (the data in a `/data` volume), a commented `nuage.env.example`, and the Heroku files below. Put the server behind HTTPS (most hosts do it for you): the token travels in a header.

## Deploy on Heroku

Heroku runs the server on one **Basic** dyno. A dyno's disk is **ephemeral**: it is erased at every restart (at least once a day, and at each deploy). The data directory is only a working copy; **the data lives in your backup bucket**, and each start restores it from there. Backups to S3-compatible storage are therefore required on Heroku.

1. Create the app with the Rust buildpack:

   ```bash
   heroku create my-nuage --buildpack emk/rust
   ```

2. Deploy the Nuage repository (https://github.com/maxlestage/Nuage) as is: its root already holds the `Procfile`, `RustConfig`, `heroku-build.sh` and `app.json` (for a "Deploy to Heroku" button). The `Procfile` starts `./target/release/nuage`.

3. Create a bucket (AWS S3, Cloudflare R2...) and an access key limited to it, then set the config vars:

   ```bash
   heroku config:set NUAGE_TOKEN=$(openssl rand -hex 32) \
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

- **No SQL.** Nuage DB runs Memoire statements; `db.raw(...)` and SQL migration steps answer `Error::Unsupported` (as on the in-memory engine). The console's query language is a small subset of SQL.
- **One writer at a time.** Writes are applied one after another; reads run in parallel. One server per database.
- **Joins, `GROUP BY` and aggregates** other than `COUNT(*)` load the rows they combine into memory (after the filters that apply to each table). Index lookups are used for equalities and `IN` lists on the primary key, `unique` and `index` columns; ranges scan the table.
- **Indexes** are those of the model when the table was created. New columns added to a model are stored from then on (existing rows read `NULL`); new indexes are not built for existing tables.
- **Branches.** A write in a branch that has branches of its own first copies the old value into each of them. Dropping a table in a branch that has branches keeps its rows until those branches are deleted.
- **Backups** copy the whole page store for each snapshot (writes wait during the local copy), and the S3 upload holds the snapshot in memory.
- **WebAssembly**: the client only; the engine needs a file system.

## Next steps

- [Transactions](transactions): the transaction helper, retries on conflict.
- [Migrations](migrations): versioned schema changes, which run on Nuage DB too.
- [Deploy to Heroku from your phone](deploy-heroku-mobile): the Heroku dashboard and GitHub workflows, step by step.
