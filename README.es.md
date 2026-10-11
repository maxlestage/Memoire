<div align="center">

# Memoire ORM

**El ORM nativo de Rust, ultrarrápido y con tipado seguro.**

[![crates.io](https://img.shields.io/crates/v/memoire-orm.svg)](https://crates.io/crates/memoire-orm)
[![docs.rs](https://img.shields.io/docsrs/memoire-orm)](https://docs.rs/memoire-orm)
[![Licencia: propietaria](https://img.shields.io/badge/license-proprietary-2a2a2e.svg)](#licencia)

[English](README.md) · [Français](README.fr.md) · **Español**

</div>

---

## ¿Por qué Memoire?

| | |
|---|---|
| ⚡ **SQL generado en compilación** | `#[derive(Model)]` genera todas las sentencias por clave primaria (`find`, `insert`, `update`, `delete`, `upsert`) para PostgreSQL, SQLite y MySQL mientras tu código compila. |
| 🛡️ **Consultas con tipado seguro** | `User::AGE.gt(18)` compila, `User::AGE.gt("18")` no. Combina condiciones con `&`, `\|` y `!`. |
| 🧬 **Mapeo sin sobrecoste** | Las filas se decodifican por posición en una sola pasada; los valores se mueven, nunca se copian. |
| 🗄️ **SQL y NoSQL, una sola API** | PostgreSQL, MySQL/MariaDB, SQLite, Redis, MongoDB y un motor en memoria 100 % Rust (¡que funciona incluso en WebAssembly!). |
| 🔋 **Todo incluido** | Transacciones, migraciones, joins, agregados, `GROUP BY`, paginación, upserts, errores unificados. |
| 🦀 **Seguro** | `#![forbid(unsafe_code)]`, la misma batería de pruebas pasa en los seis motores. |

## Vistazo rápido

```toml
[dependencies]
memoire-orm = { version = "0.1", features = ["sqlite"] }   # o "postgres", "mysql", "redis", "mongodb", "full"
tokio = { version = "1", features = ["full"] }
```

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    pub name: String,
    pub age: Option<i32>,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = SqliteDb::open("app.db")?;          // o MemoryDb, PgPool, MySqlPool, RedisDb, MongoDb
    db.create_table_if_not_exists::<User>().await?;

    let mut alice = User { id: 0, email: "alice@rust.dev".into(), name: "Alice".into(), age: Some(31) };
    alice.insert(&db).await?;                    // alice.id ya tiene valor

    let adultos = User::query()
        .filter(User::AGE.ge(18) & User::NAME.contains("li"))
        .order_by(User::NAME.asc())
        .limit(20)
        .all(&db)
        .await?;
    println!("{adultos:#?}");
    Ok(())
}
```

Motores compatibles (una feature cada uno):

| Motor | Feature | Conexión | Notas |
|---|---|---|---|
| En memoria | `memory` (por defecto) | `MemoryDb::new()` | 100 % Rust, funciona en WebAssembly, ideal para pruebas |
| SQLite | `sqlite` | `SqliteDb::open("app.db")?` | embebido, nada que instalar |
| PostgreSQL | `postgres` (+ `postgres-tls`) | `PgPool::connect(url).await?` | Heroku Postgres, Neon, Supabase… |
| MySQL / MariaDB | `mysql` (+ `mysql-tls`) | `MySqlPool::connect(url).await?` | MySQL 8+, MariaDB 10.6+ |
| Redis | `redis` (+ `redis-tls`) | `RedisDb::connect(url).await?` | NoSQL clave-valor, filas guardadas en JSON |
| MongoDB | `mongodb` | `MongoDb::connect(url).await?` | NoSQL documentos, transacciones reales en replica sets |

Ejecutar los ejemplos:

```bash
cargo run -p memoire-orm --example quickstart                    # motor en memoria
cargo run -p memoire-orm --example sqlite --features sqlite      # migraciones + transacciones
cargo run -p memoire-orm --example blog                          # joins, carga anticipada, GROUP BY
DATABASE_URL=postgres://… cargo run -p memoire-orm --example postgres --features postgres
```

## Rendimiento

El motor en memoria (que también evalúa las consultas de los backends Redis y MongoDB) filtra con
bucles especializados para cada tipo de columna, sobre copias contiguas de las columnas numéricas,
y sus índices nunca copian las claves. Intel Xeon 2,8 GHz, medianas de criterion, tabla de 10 000 filas:

| Motor en memoria | Antes | Ahora |
|---|---:|---:|
| Igualdad sobre una columna `#[memoire(index)]` | 140,9 µs | **1,1 µs** (×123) |
| `COUNT(*) WHERE` | 106,2 µs | **11,1 µs** (×9,6) |
| Filtro + `ORDER BY` + `LIMIT 20` | 231,2 µs | **36,3 µs** (×6,4) |
| `update_many` con un filtro | 177,0 µs | **35,8 µs** (×4,9) |
| `insert_many`, 1 000 filas | 422,9 µs | **246,0 µs** (×1,7) |

En SQLite, comparado con código `rusqlite` escrito a mano (mismo driver, sentencias preparadas en
caché: el límite al que puede llegar cualquier ORM sobre SQLite):

| Operación | Memoire | rusqlite puro |
|---|---:|---:|
| Búsqueda por clave primaria | 961 ns | 746 ns |
| Selección + decodificación de 1 000 filas | 315 µs | 308 µs |
| Inserción de una fila | 6,26 µs | 5,92 µs |
| Inserción de 1 000 filas en una transacción | **1,95 ms** | 6,64 ms |

Detalles: [`crates/memoire-orm/benches/RESULTS.md`](crates/memoire-orm/benches/RESULTS.md) ·
reprodúcelo con `cargo bench -p memoire-orm --features memory,sqlite`.

## Documentación

La guía completa está en [`docs/es`](docs/es) (también en [inglés](docs/en) y en [francés](docs/fr)) y en el sitio web.

| Primeros pasos | Conceptos clave | Bases de datos | Para ir más lejos |
|---|---|---|---|
| [Introducción](docs/es/introduction.md) | [Modelos](docs/es/models.md) | [En memoria](docs/es/memory.md) | [Por qué es rápido](docs/es/performance.md) |
| [Instalación](docs/es/installation.md) | [Constructor de consultas](docs/es/queries.md) | [SQLite](docs/es/sqlite.md) | [Benchmarks](docs/es/benchmarks.md) |
| [Inicio rápido](docs/es/quick-start.md) | [CRUD](docs/es/crud.md) | [PostgreSQL](docs/es/postgres.md) | [Desplegar desde el móvil](docs/es/deploy-heroku-mobile.md) |
| | [Uniones y relaciones](docs/es/relations.md) | [MySQL y MariaDB](docs/es/mysql.md) | [Publicar en crates.io](docs/es/publish-crates-io.md) |
| | [Transacciones](docs/es/transactions.md) · [Migraciones](docs/es/migrations.md) · [SQL sin procesar](docs/es/raw-sql.md) | [Redis](docs/es/redis.md) · [MongoDB (NoSQL)](docs/es/mongodb.md) | [Chuleta](docs/es/cheatsheet.md) · [Preguntas frecuentes](docs/es/faq.md) |

Referencia de la API: [docs.rs/memoire-orm](https://docs.rs/memoire-orm).

## Estructura del repositorio

```text
crates/memoire-orm/          el ORM (publicado en crates.io)
crates/memoire-orm-derive/   #[derive(Model)] y #[derive(FromRow)]
docs/{en,fr,es}/             documentación (mostrada por el sitio web)
site/                        sitio de presentación, escrito con active (servidor + islas WebAssembly, PWA)
Dockerfile, heroku.yml, app.json   despliegue en Heroku con un clic
.github/workflows/           CI, fusión automática, despliegue en Heroku y GitHub Pages, publicación en crates.io
```

## El sitio web

El sitio de presentación está escrito en Rust con [active](https://maxlestage.github.io/Active/):
un pequeño servidor Rust renderiza cada página (etiquetas SEO completas, en inglés, francés y
español), y solo las partes interactivas (el playground, las pestañas de código, la búsqueda en
la documentación) se activan en el navegador como WebAssembly. El playground ejecuta el motor en
memoria real en tu navegador. El sitio es una PWA: instálalo en la pantalla de inicio de tu
móvil; una vez abierto, sigue funcionando sin conexión, playground incluido. El mismo servidor
exporta todo el sitio como archivos estáticos para GitHub Pages.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129   # la versión de site/Cargo.lock
cd site
cargo build --lib --target wasm32-unknown-unknown --profile wasm-release
wasm-bindgen --target web --no-typescript --out-dir pkg target/wasm32-unknown-unknown/wasm-release/memoire_site.wasm
cargo run                                          # http://localhost:8080
BASE_PATH=/Memoire cargo run -- export dist-static  # archivos estáticos, como en GitHub Pages
```

## Desplegar desde el móvil 📱

Sin ordenador:

1. Abre `https://heroku.com/deploy?template=<la dirección de tu repositorio>` en tu móvil, elige un nombre de aplicación y pulsa **Deploy app**.
2. O añade los secretos `HEROKU_API_KEY` y `HEROKU_APP_NAME` en *Settings → Secrets and variables → Actions*:
   cada push a `master` se despliega automáticamente, y puedes volver a desplegar desde la app de GitHub
   (*Actions → Deploy to Heroku → Run workflow*).
3. O publica el sitio **gratis en GitHub Pages**: *Settings → Pages → Source: GitHub Actions* y luego
   *Actions → Deploy to GitHub Pages → Run workflow*. Queda en línea en `https://<tu-usuario>.github.io/Memoire/`.

Guía paso a paso: [docs/es/deploy-heroku-mobile.md](docs/es/deploy-heroku-mobile.md).

## Automatización

- **CI** (`ci.yml`): formato, clippy, la misma batería de pruebas en **los seis motores** (memoria, SQLite,
  PostgreSQL, MySQL/MariaDB, Redis, MongoDB), cada feature por separado, compilación WebAssembly, compilación del sitio.
- **Fusión automática** (`ci.yml`): las pull requests hacia `master` se fusionan automáticamente en cuanto la CI
  está en verde (y se lanzan los despliegues en Heroku y GitHub Pages).
- **Despliegue** (`deploy-heroku.yml`): cada push a `master` despliega el sitio en Heroku.
- **GitHub Pages** (`pages.yml`): cada push a `master` que cambia el sitio o la documentación lo publica en GitHub Pages.
- **Publicación** (`publish.yml`): subir un tag `v*` (o publicar una release de GitHub) publica los crates en
  crates.io — ver [Publicar en crates.io](docs/es/publish-crates-io.md).

## Contribuir

¡Las contribuciones son bienvenidas! Consulta [CONTRIBUTING.md](CONTRIBUTING.md).

## Licencia

Memoire ORM es software propietario: © 2026 Maxime Nathan Lestage, todos los
derechos reservados. No es software de código abierto. El código fuente se
publica en crates.io y GitHub para que pueda leerse, auditarse y compilarse,
y puedes usarlo libremente como dependencia de tus propios programas, también
comerciales. Modificarlo o redistribuir su código fuente requiere el permiso
escrito del autor. Condiciones completas: [LICENSE](LICENSE).

Las contribuciones (código, documentación, ideas) son bienvenidas y pasan a
formar parte del proyecto en los mismos términos: al enviar una, cedes sus
derechos al autor, como indica la licencia.

## Créditos

Diseñado y desarrollado por Maxime Nathan Lestage,
dentro de una misma familia de proyectos Rust: [Active](https://maxlestage.github.io/Active/), [Vitesse](https://maxlestage.github.io/Vitesse/), [Securite](https://maxlestage.github.io/Securite/), [Memoire](https://maxlestage.github.io/Memoire/), [Paquets](https://maxlestage.github.io/Paquets/) y [FerroSwift](https://maxlestage.github.io/FerroSwift/).
