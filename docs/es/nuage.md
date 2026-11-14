# Nuage DB

Nuage DB es la base de datos propia de Memoire, escrita en Rust. Guarda tus tablas en disco detrás de un registro de escritura anticipada (write-ahead log), crea al instante ramas copy-on-write de una base, se respalda sola en una segunda ubicación y lo sirve todo por HTTP(S), con una consola de administración. Es el almacenamiento recomendado para los proyectos nuevos de Memoire: los mismos modelos y consultas funcionan con Nuage DB embebido en tu aplicación durante el desarrollo y los tests, y con un servidor Nuage DB en producción, sin cambiar una línea.

Tiene dos partes:

- el **crate `nuage`**: el motor, el manejador embebido (`nuage::Embedded`), el servidor y el binario `nuage`;
- la **feature `nuage` de memoire-orm**: `NuageDb`, el cliente de un servidor Nuage DB, que también funciona en WebAssembly en el navegador.

## Cuándo usarlo

- **Proyectos nuevos**: una sola base para el desarrollo (embebida, una carpeta en el disco) y la producción (un servidor), con el mismo código.
- **Previsualizaciones y tests**: una rama es una copia completa de la base, hecha al instante, sea cual sea su tamaño.
- **Navegadores y funciones edge**: `NuageDb` habla HTTP(S), así que el código WebAssembly llega a la base mediante `fetch`.
- **Equipos pequeños sin administrador de bases de datos**: los checkpoints, las copias de seguridad, las restauraciones y la compactación se hacen solos.

Para muchos servidores de aplicación que escriben mucho en una misma base compartida, es mejor [PostgreSQL](postgres). Nuage DB también habla SQL, para sitios web y cualquier lenguaje (ver [SQL](#sql)).

## Instalación

```bash
# El motor, embebido, y el servidor
cargo add nuage --git https://github.com/maxlestage/Nuage
cargo add memoire-orm --git https://github.com/maxlestage/Memoire
cargo add tokio --features macros,rt-multi-thread

# Solo el cliente de un servidor (también para wasm32)
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features nuage
```

## Embebido: una carpeta en el disco

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
    let db = Embedded::open("data")?; // se crea si no existe
    db.create_table_if_not_exists::<Note>().await?;
    let mut note = Note { id: 0, text: "hola".into() };
    note.insert(&db).await?;
    println!("{:?}", Note::query().all(&db).await?);
    Ok(())
}
```

`Embedded` implementa los traits `Executor` y `Database` de Memoire: los modelos, el constructor de consultas, las relaciones, las transacciones (`db.begin()`, `db.transaction(...)`) y las migraciones funcionan como con todos los demás backends. Abre una carpeta una vez por proceso y clona el manejador (los clones comparten el motor).

## Servidor y cliente

Arranca el servidor (aquí en local, con los datos en `./nuage-data`):

```bash
git clone https://github.com/maxlestage/Nuage && cd Nuage
# La clave que lo cifra todo: guárdala, los datos son ilegibles sin ella.
export NUAGE_ENCRYPTION_KEY=$(openssl rand -hex 32)
NUAGE_TOKEN=dev-token NUAGE_ADMIN_PASSWORD=dev-password cargo run -p nuage --release
# nuage: data is stored in /home/me/app/nuage-data on this server's disk
# nuage: listening on 0.0.0.0:8080
```

Conéctate desde tu aplicación:

```rust
let db = NuageDb::connect("http://127.0.0.1:8080", "dev-token").await?;
Note::query().filter(Note::TEXT.contains("hola")).all(&db).await?;
```

`NuageDb` está en `memoire_orm::prelude` con la feature `nuage`. Mantiene abiertas sus conexiones HTTP entre peticiones, usa TLS (rustls) para las URL `https://` y necesita el runtime Tokio en nativo. En WebAssembly usa el `fetch` del navegador y no necesita runtime.

### Un solo ajuste para ambos

`nuage::Db` es uno u otro, elegido por la configuración:

```rust
// NUAGE_URL=data                                  -> embebido, en ./data
// NUAGE_URL=https://db.example.com NUAGE_TOKEN=…  -> un servidor
let db = nuage::Db::from_env().await?;
```

## Instalar en un servidor

En cualquier servidor Linux (Debian, Ubuntu, Fedora, Rocky, Alpine..., x86_64 o ARM64, desde 512 MiB de memoria), un comando:

```bash
curl -fsSL https://raw.githubusercontent.com/maxlestage/Nuage/master/install.sh | sudo bash
```

Instala el binario `nuage` (compilado estático, comprobado con su sha256), **crea swap** si el servidor no tiene (el tamaño de la memoria, de 1 a 4 GiB, con `vm.swappiness=10` para que la memoria sea para la base), un usuario `nuage`, los datos en `/var/lib/nuage`, los ajustes en `/etc/nuage/nuage.env` con una clave, un token y una contraseña de consola generados, y un servicio systemd que arranca con el sistema. Muestra una vez la dirección, el token y la contraseña: **copia `/etc/nuage/nuage.env` en un lugar seguro**, nadie puede leer los datos sin su clave.

- **HTTPS**: apunta un nombre de dominio al servidor y ejecuta `curl -fsSL .../install.sh | sudo NUAGE_DOMAIN=db.example.com bash` (Caddy, con un certificado gratuito).
- **Actualizar**: vuelve a ejecutar el comando; datos, clave y ajustes se conservan.
- **Opciones**: `NUAGE_PORT`, `NUAGE_SWAP_MB` (`0`: sin swap), `NUAGE_VERSION=v0.3.0`, `NUAGE_FROM_SOURCE=1`.
- **Desinstalar**: `... | sudo bash -s uninstall` (conserva los datos), o `purge`.
- **Docker**: `docker compose up -d` en el repositorio Nuage, con los ajustes en `.env`.

Hay binarios para Linux, macOS y Windows en la [página de versiones](https://github.com/maxlestage/Nuage/releases).

## Memoria

La caché de páginas de la base de cada aplicación es la única memoria que Nuage DB guarda a propósito; los datos se quedan en disco. `NUAGE_CACHE_MB=auto` (por defecto) le da un octavo de la memoria, de 8 a 512 MiB, incluido el límite del contenedor: un servidor de 512 MiB conserva libre la mayor parte de su memoria. La página **Servidor** de la consola y `/api/status` (`memory`) muestran la memoria que usa Nuage DB, la memoria libre y el swap.

## Cifrado

Nuage DB cifra todo lo que guarda: las páginas con XTS-AES-256, cada registro del log con AES-256-GCM, y las copias de seguridad, copias de ambos. `NUAGE_ENCRYPTION_KEY` es una clave (`nuage genkey`, `openssl rand -hex 32`) o una frase de contraseña (Argon2id); el servidor no arranca sin ella, y una clave errónea se rechaza antes de leer nada. **Guarda la clave**: sin ella, ni los datos ni las copias se pueden leer. En modo embebido, pasa `EngineOptions { encryption: Some(nuage::Encryption::parse(clave)?), .. }` a `Embedded::open_with`, o usa `nuage::Db::from_env()`, que lee `NUAGE_ENCRYPTION_KEY`.

## Varias aplicaciones

Un servidor aloja varias aplicaciones, cada una con su base, sus claves, sus copias y sus tokens. La primera es la base del servidor (`NUAGE_NAME`, `NUAGE_TOKEN`); la consola (**Nueva aplicación**) crea las demás y muestra cada token nuevo una vez (el servidor solo guarda su huella SHA-256). El token que envía una aplicación elige su base: `NuageDb::connect(url, token)` no cambia, y una aplicación nunca ve los datos de otra. Los tokens de solo lectura y los sitios autorizados a llamar al servidor desde el navegador (CORS) se ajustan por aplicación.

## SQL

Nuage DB habla SQL: `POST /v1/sql` desde cualquier lenguaje, la pestaña **Consulta** de la consola, y `Embedded::sql` en Rust. `SELECT` con uniones, `GROUP BY`, `HAVING`, `UNION`, subconsultas, `CASE` y las funciones habituales; `INSERT ... ON CONFLICT`, `UPDATE`, `DELETE`, `RETURNING`; `CREATE TABLE`, `CREATE INDEX`, `ALTER TABLE ... ADD COLUMN`, `DROP`. Los parámetros (`?` o `$1`) nunca se interpretan como SQL. Varias sentencias en una petición se ejecutan en una sola transacción.

```bash
curl https://db.example.com/v1/sql -H "Authorization: Bearer $NUAGE_TOKEN" \
  -d '{"sql": "SELECT id, text FROM notes WHERE text LIKE ? ORDER BY id DESC LIMIT 20", "params": ["%hello%"]}'
```

```rust
let page = db.sql("SELECT id, upper(text) AS t FROM notes ORDER BY id DESC LIMIT 10", &[])?;
```

`db.raw(...)` y los pasos de migración en SQL de Memoire siguen respondiendo `Error::Unsupported` en Nuage DB: desde Rust, usa los modelos y el constructor de consultas, o `Embedded::sql`.

## Transacciones

En un servidor, `db.begin()` abre una transacción interactiva: el servidor la conserva entre peticiones y la deshace si queda inactiva 30 segundos. Una transacción lee una instantánea tomada al empezar, más sus propios cambios. Es optimista: cuando otro commit cambió una tabla que usó, su commit falla con `Error::Conflict`, y `db.transaction(...)` vuelve a ejecutar la closure.

Para enviar varias sentencias en una sola petición, todo o nada, usa un lote:

```rust
let results = db.batch()
    .execute(Query::InsertRow { table: Note::SCHEMA, values: &note.to_values() })
    .fetch(Query::Select(Note::query().ast()))
    .run()
    .await?;
```

## Ramas

Una rama es una copia de la base para una previsualización, un test o un experimento. Crearla es instantáneo: comparte todas las páginas de su padre y solo guarda sus propios cambios. Los cambios posteriores del padre no la alcanzan.

```rust
let preview = db.create_branch("preview-42", None).await?; // desde la base principal
let on_preview = db.on_branch("preview-42");
Note { id: 0, text: "solo en la previsualización".into() }.insert(&on_preview).await?;

db.reset_branch("preview-42").await?;  // olvidar sus cambios
db.delete_branch("preview-42").await?;
```

En modo embebido, las mismas operaciones están en el motor: `db.engine().create_branch(...)` y `Embedded::on_branch`. Una rama puede tener sus propias ramas; bórralas antes que a su padre.

## La consola

Define `NUAGE_ADMIN_PASSWORD` y abre `/admin`:

- **Resumen**: tamaño, tablas, filas, ramas, tiempo en marcha, conexiones, caché, último checkpoint, copias de seguridad;
- **Tablas**: número de filas y tamaño, y las filas de cada tabla, página a página;
- **Consulta**: SQL, con una confirmación antes de las sentencias que cambian datos;
- **Ramas**: lista, creación, reinicio, borrado;
- **Almacenamiento**: uso del disco, registro, estado de las copias de seguridad, y botones para un checkpoint, una copia de seguridad, una verificación de integridad y una compactación.

Las páginas las genera el servidor y funcionan sin JavaScript, también en el móvil, en tema claro y oscuro, en español, inglés y francés. La cookie de sesión es `HttpOnly`, `SameSite=Strict` y, por HTTPS o en `localhost`, `Secure`; cada formulario lleva un token CSRF, y los inicios de sesión fallidos están limitados (5 por dirección y 30 en total cada 15 minutos). Por HTTP simple (un servidor recién instalado, al que se llega por su IP) la consola funciona, pero su página de inicio de sesión avisa de que la contraseña viaja sin cifrar: sírvela por HTTPS. Un enlace a `/admin?next=/una/pagina` vuelve a esa página del mismo sitio tras iniciar sesión; los demás destinos (otro sitio) llevan a `/admin`.

## Estado en vivo

| Endpoint | Acceso | Contenido |
|---|---|---|
| `GET /api/health` | público | `{"status":"ok"}` |
| `GET /api/status` | token o sesión de la consola | peticiones por segundo, latencia p50/p99, conexiones, almacenamiento, tasa de aciertos de la caché, registro, copia de seguridad, bases y ramas, servicios |
| `GET /api/status/stream` | igual | el mismo JSON cada segundo como Server-Sent Events (`event: status`), más `event: log` para checkpoints, copias de seguridad y ramas |

Con `NUAGE_PUBLIC_STATUS=true`, las peticiones sin credenciales reciben un subconjunto público: estado, tiempo en marcha y el nombre y el estado de cada servicio.

## Cómo funciona

- **Páginas en disco.** Tablas e índices viven en un solo archivo de páginas en árbol B que Nuage DB escribe por sí mismo (copia en escritura, suma de control en cada nodo, Rust puro), con una caché de páginas acotada (`NUAGE_CACHE_MB`, por defecto un octavo de la memoria, de 8 a 512 MiB). Los datos no están limitados por la memoria y el archivo crece con ellos.
- **Registro de escritura anticipada.** Cada commit se añade al registro y se sincroniza con el disco (`fsync`) antes de aplicarse. Un checkpoint, cada 30 segundos o cada 16 MiB de registro, hace duradero el almacén de páginas y empieza un nuevo segmento del registro. Tras un fallo, el siguiente arranque vuelve a aplicar lo que faltaba desde el último checkpoint. Los registros llevan un CRC-32 y las páginas una suma de comprobación: los daños se detectan, y un registro cortado por el fallo se descarta.
- **Sentencias.** El motor solo lee las filas que necesita una sentencia: las búsquedas por clave primaria y por índice leen las filas que coinciden, los demás filtros recorren la tabla por bloques, `LIMIT` se detiene pronto y `ORDER BY ... LIMIT n` guarda las `n` mejores filas mientras lee; `COUNT(*)` lee un contador. Luego el evaluador de Memoire ejecuta la sentencia sobre esas filas: Nuage DB se comporta exactamente como el motor en memoria.
- **Mantenimiento.** Un hilo en segundo plano hace los checkpoints, envía las copias de seguridad y compacta el archivo cuando una cuarta parte es espacio libre.

## Copias de seguridad y restauración

Las copias de seguridad van a una carpeta (`NUAGE_BACKUP_DIR`) o a un almacenamiento compatible con S3 (`NUAGE_BACKUP_S3_*`: AWS S3, Cloudflare R2, MinIO, Scaleway...). Una copia es una instantánea del almacén de páginas (cada hora por defecto) más los segmentos del registro escritos desde entonces, enviados en cada checkpoint. Cuando el servidor arranca con una carpeta de datos vacía y una ubicación de copias, restaura la última instantánea y vuelve a aplicar los segmentos siguientes antes de servir.

Los commits hechos desde el último segmento enviado aún no están en la copia: como mucho un intervalo de checkpoint, 30 segundos por defecto. Con `SIGTERM` (lo que envían los alojamientos antes de un reinicio), el servidor hace un checkpoint y envía el último segmento antes de detenerse.

## Desplegar en cualquier alojamiento

El servidor es un solo binario configurado con variables de entorno:

| Variable | Función |
|---|---|
| `PORT` | puerto (por defecto `8080`) |
| `NUAGE_ENCRYPTION_KEY` | la clave que lo cifra todo; **obligatoria** |
| `NUAGE_BIND` | dirección de escucha (por defecto `0.0.0.0`; `127.0.0.1` detrás de un proxy) |
| `NUAGE_DATA_DIR` | carpeta de datos (por defecto `nuage-data`) |
| `NUAGE_NAME` | nombre de la base (por defecto `nuage`) |
| `NUAGE_TOKEN` | token de la API de la base; sin él, la API rechaza todas las peticiones |
| `NUAGE_ADMIN_PASSWORD` | contraseña de la consola; sin ella, la consola está desactivada |
| `NUAGE_PUBLIC_STATUS` | `true` para el subconjunto público del estado |
| `NUAGE_CACHE_MB` | caché de páginas de cada aplicación en MiB, o `auto` (por defecto) |
| `NUAGE_BACKUP_DIR` | copias de seguridad en una carpeta |
| `NUAGE_BACKUP_S3_BUCKET`, `_REGION`, `_ENDPOINT`, `_PREFIX`, `_ACCESS_KEY_ID`, `_SECRET_ACCESS_KEY` | copias de seguridad en un almacenamiento compatible con S3 |
| `NUAGE_BACKUP_SNAPSHOT_MINUTES` | minutos entre instantáneas (por defecto 60) |

La raíz del repositorio Nuage contiene `install.sh` (arriba), un `Dockerfile` (los datos en un volumen `/data`) y `docker-compose.yml`, un `nuage.env.example` comentado y los archivos de Heroku de abajo. Pon el servidor detrás de HTTPS (la mayoría de alojamientos lo hacen por ti; en tu propio servidor, `NUAGE_DOMAIN` del instalador): el token viaja en una cabecera.

## Desplegar en Heroku

Heroku ejecuta el servidor en un dyno **Basic**. El disco de un dyno es **efímero**: se borra en cada reinicio (al menos una vez al día y en cada despliegue). La carpeta de datos es solo una copia de trabajo; **los datos viven en tu bucket de copias de seguridad**, y cada arranque los restaura desde ahí. Por eso las copias en un almacenamiento compatible con S3 son obligatorias en Heroku.

1. Crea la aplicación con el buildpack de Rust:

   ```bash
   heroku create my-nuage --buildpack emk/rust
   ```

2. Despliega el repositorio Nuage (https://github.com/maxlestage/Nuage) tal cual: su raíz ya contiene el `Procfile`, `RustConfig`, `heroku-build.sh` y `app.json` (para un botón «Deploy to Heroku»). El `Procfile` arranca `./target/release/nuage`.

3. Crea un bucket (AWS S3, Cloudflare R2...) y una clave de acceso limitada a él, y define las config vars:

   ```bash
   heroku config:set NUAGE_ENCRYPTION_KEY=$(openssl rand -hex 32) NUAGE_TOKEN=$(openssl rand -hex 32) \
     NUAGE_ADMIN_PASSWORD='una frase de paso larga' \
     NUAGE_NAME=nuage \
     NUAGE_BACKUP_S3_BUCKET=my-bucket NUAGE_BACKUP_S3_REGION=eu-west-3 \
     NUAGE_BACKUP_S3_ACCESS_KEY_ID=... NUAGE_BACKUP_S3_SECRET_ACCESS_KEY=...
   ```

   Para otro proveedor que no sea AWS, define también `NUAGE_BACKUP_S3_ENDPOINT`.

4. Despliega (`git push heroku main`, o la integración con GitHub), abre `https://my-nuage.herokuapp.com/admin` e inicia sesión con `NUAGE_ADMIN_PASSWORD`. Las aplicaciones se conectan con `NuageDb::connect("https://my-nuage.herokuapp.com", token)`.

A tener en cuenta en Heroku:

- **Dónde están los datos.** En el disco del dyno mientras funciona, y en el bucket de forma duradera. El registro de arranque indica ambos. Tras un reinicio, las primeras peticiones esperan la restauración, que dura lo que tarde la descarga de la última instantánea.
- **Ventana de pérdida.** Los commits desde el último segmento enviado (30 segundos por defecto) se pierden si el dyno se detiene sin el `SIGTERM` que Heroku envía normalmente antes.
- **Un solo dyno.** Ejecuta exactamente un dyno web: dos dynos serían dos bases distintas que restauran desde el mismo bucket y escriben en él.
- **Los dynos Eco** se duermen tras 30 minutos sin tráfico y despiertan con la siguiente petición: encaja con el «scale to zero» cuando la espera de la restauración al despertar es aceptable. Los dynos Basic nunca se duermen.
- **Memoria y disco.** La caché de páginas (`NUAGE_CACHE_MB`) debe caber en la memoria del dyno junto al servidor; el disco del dyno debe poder contener la carpeta de datos.
- **Las ramas** se guardan en el mismo archivo, así que se respaldan y restauran con la base principal.

## Límites

- **SQL desde Memoire.** `db.raw(...)` y los pasos de migración en SQL responden `Error::Unsupported` (como en el motor en memoria); el SQL pasa por `/v1/sql` o `Embedded::sql`. No están: uniones `RIGHT`/`FULL`, `WITH`, funciones de ventana, vistas, disparadores, claves primarias de varias columnas.
- **Un solo escritor a la vez.** Las escrituras se aplican una tras otra; las lecturas se ejecutan en paralelo. Un servidor por base.
- **Las uniones, `GROUP BY` y los agregados** distintos de `COUNT(*)` cargan en memoria las filas que combinan (tras los filtros propios de cada tabla; en SQL, uniones por hash). Los índices sirven para igualdades y listas `IN` sobre la clave primaria y las columnas `unique` e `index`; los rangos recorren la tabla.
- **Los índices** son los del modelo al crear la tabla, más los creados con `CREATE INDEX` (construidos a partir de las filas existentes). Las columnas nuevas de un modelo se guardan a partir de entonces (las filas existentes leen `NULL`); `ALTER TABLE ... ADD COLUMN` rellena las filas existentes con el valor por defecto.
- **Ramas.** Una escritura en una rama que tiene sus propias ramas copia primero el valor anterior en cada una de ellas. Borrar una tabla en una rama que tiene ramas conserva sus filas hasta que se borren esas ramas.
- **Las copias de seguridad** copian todo el almacén de páginas en cada instantánea (las escrituras esperan durante la copia local), y el envío a S3 mantiene la instantánea en memoria.
- **WebAssembly**: solo el cliente; el motor necesita un sistema de archivos.

## Siguientes pasos

- [Transacciones](transactions): el asistente de transacciones y los reintentos ante conflictos.
- [Migraciones](migrations): los cambios de esquema versionados, que también funcionan con Nuage DB.
- [Desplegar en Heroku desde el móvil](deploy-heroku-mobile): el panel de Heroku y los workflows de GitHub, paso a paso.
