# Desplegar en Heroku desde el móvil

Puedes poner en línea el sitio web de Memoire, y mantenerlo actualizado, usando solo un teléfono: sin ordenador y sin terminal. Esta página recorre tres formas de hacerlo, explica lo que ocurre entre bastidores y termina con el despliegue de tu propia aplicación Rust que usa Memoire y Heroku Postgres.

## Antes de empezar

Necesitas:

- una **cuenta de GitHub** y tu propia copia del repositorio: abre `https://github.com/maxlestage/Memoire` y toca **Fork** (solo hace falta para los métodos 2 y 3);
- una **cuenta de Heroku** (regístrate en `https://signup.heroku.com` desde el navegador de tu teléfono);
- un **método de pago** en la cuenta de Heroku. Heroku no tiene plan gratuito desde noviembre de 2022: el sitio web se ejecuta en un dyno *Basic* (unos 7 USD al mes en el momento de escribir esto) y Heroku no arranca aplicaciones en una cuenta sin verificar. Consulta `https://www.heroku.com/pricing` para ver los precios actuales.

Todos los pasos de Heroku se hacen en el panel de Heroku (`https://dashboard.heroku.com`) desde el navegador del móvil. Los pasos de GitHub funcionan en la app móvil de GitHub, salvo la gestión de los secretos, que requiere el sitio web de GitHub en un navegador móvil.

> **Consejo** Si falta un menú o una pestaña en una pantalla pequeña, usa la opción "Request desktop site" de tu navegador (en un navegador en español suele llamarse "Sitio de escritorio" o "Versión para ordenador"). Girar el teléfono en horizontal también ayuda con las páginas anchas, como la configuración del repositorio.

## ¿Qué método debería elegir?

| Método | Esfuerzo | Actualizaciones tras el primer despliegue | Dónde se construye el sitio |
|---|---|---|---|
| 1. Botón "Deploy to Heroku" | El mínimo: unos pocos toques | Ninguna: despliega una sola vez | En Heroku, con el buildpack de Rust |
| 2. Workflow de GitHub Actions | Medio: hay que añadir dos secretos | Automáticas en cada push a `master`, o con un toque en la app de GitHub | En Heroku, con el buildpack de Rust (o en GitHub como imagen Docker, si lo eliges) |
| 3. Integración de Heroku con GitHub | Medio: conectar GitHub con Heroku | Automáticas en cada push a `master` | En Heroku, con el buildpack de Rust |

Una combinación habitual: el método 1 para el primerísimo despliegue y luego el método 2 o el 3 para mantener el sitio actualizado. No actives los métodos 2 y 3 a la vez, o cada push se desplegará dos veces.

## Método 1: el botón "Deploy to Heroku"

El README del repositorio tiene un botón morado **Deploy to Heroku** ("Desplegar en Heroku"). Apunta a:

```text
https://dashboard.heroku.com/new-app
```

1. Abre el repositorio en GitHub (app o navegador) y desplázate hasta el principio del README.
2. Toca **Deploy to Heroku**. Tu navegador abre Heroku; inicia sesión si te lo pide.
3. Elige un **App name** (nombre de la aplicación: letras minúsculas, dígitos y guiones, único en todo Heroku) y una **region** (región: United States o Europe).
4. Deja el ajuste `RUST_LOG` en `info`.
5. Toca **Deploy app**.
6. Espera a que termine la compilación. La primera compila todo el sitio web desde el código fuente, lo que puede tardar 10 minutos o más. Puedes seguirla en directo en la página.
7. Cuando Heroku muestre "Your app was successfully deployed" (tu aplicación se ha desplegado correctamente), toca **View** para abrir tu sitio.

Lo que ocurre entre bastidores:

- `app.json` describe la aplicación para Heroku: su nombre y su descripción, el stack **heroku-26** con el buildpack **emk/rust** ([heroku-buildpack-rust](https://elements.heroku.com/buildpacks/emk/heroku-buildpack-rust)), un dyno `web` de tamaño *Basic* y el ajuste `RUST_LOG`.
- `RustConfig` configura el buildpack: instala la versión de Rust fijada en ese archivo (la misma que en el `Dockerfile`), omite su propia compilación (`RUST_SKIP_BUILD=1`) y ejecuta `heroku-build.sh` en su lugar.
- `heroku-build.sh` compila el servidor web del sitio, escrito en Rust con [Active](https://active-c1883a966650.herokuapp.com/), en `site/` con `cargo build --release --locked`. El servidor renderiza cada página e incluye la documentación. Después compila el paquete del navegador en `site/pkg`: las partes interactivas y el motor en memoria del playground, compilados a WebAssembly con `wasm-bindgen`.
- `Procfile` le indica a Heroku cómo arrancar el proceso `web`: `web: SITE_DIR=site exec site/target/release/memoire-site`.

El `Dockerfile` y `heroku.yml` siguen ahí: construyen el mismo sitio como imagen Docker, para el stack **container** de Heroku (método 2 con `HEROKU_BUILD` en `docker`) o cualquier otro host Docker.

> **Nota** El botón despliega el repositorio tal como está en ese momento. Para obtener los cambios posteriores, usa el método 2 o el 3, o vuelve a tocar el botón para crear una aplicación nueva.

## Método 2: GitHub Actions (automático, y con un toque desde la app de GitHub)

El repositorio contiene un workflow, `.github/workflows/deploy-heroku.yml`, llamado **Deploy to Heroku**. Se ejecuta en cada push a `master` y también se puede lanzar a mano. Necesita dos secretos del repositorio.

### Paso 1: obtener una clave de API de Heroku

1. En el navegador del móvil, abre `https://dashboard.heroku.com/account`.
2. Desplázate hasta **API Key** y toca **Reveal**; después copia la clave.

Para tener un token dedicado a GitHub (recomendado, porque puedes revocarlo por separado):

1. Abre `https://dashboard.heroku.com/account/applications`.
2. En **Authorizations**, toca **Create authorization**.
3. Escribe una descripción como "GitHub Actions deploy", deja la caducidad vacía (o elige una) y toca **Create**.
4. Copia el token que aparece. Heroku solo lo muestra una vez.

### Paso 2: añadir los secretos a tu repositorio

Los secretos no se pueden editar en la app de GitHub, así que usa el sitio web en el navegador del móvil:

1. Abre tu fork: `https://github.com/<your-name>/Memoire`.
2. Toca **Settings**, luego **Secrets and variables** y luego **Actions**.
3. Toca **New repository secret**. Nombre (Name): `HEROKU_API_KEY`. Valor (Value): la clave o el token del paso 1. Toca **Add secret**.
4. Vuelve a tocar **New repository secret**. Nombre: `HEROKU_APP_NAME`. Valor: el nombre que quieras para tu aplicación, por ejemplo `memoire-docs-yourname`. Toca **Add secret**.

`HEROKU_APP_NAME` también se puede crear como **variable** del repositorio (la pestaña **Variables** de la misma página), ya que no es secreto. Si la aplicación aún no existe, el workflow la crea por ti.

Por defecto, Heroku construye él mismo el sitio con el buildpack de Rust. Para construir en su lugar una imagen Docker en las máquinas de GitHub, añade una **variable** del repositorio (en la misma pestaña **Variables**) llamada `HEROKU_BUILD` con el valor `docker`. El valor `buildpack`, o no crear la variable, mantiene el comportamiento por defecto.

### Paso 3: desplegar

A partir de ahora, cualquier push a `master` despliega automáticamente. Para desplegar en el acto desde la **app móvil de GitHub**:

1. Abre tu repositorio en la app.
2. Toca **Actions** (en el menú del repositorio; puede que antes tengas que tocar **More**).
3. Toca el workflow **Deploy to Heroku**.
4. Toca **Run workflow**, deja la rama `master` y confirma con **Run workflow**.
5. Toca la nueva ejecución para seguir su progreso. Cuando termine, su resumen muestra "Deployed to" seguido de la dirección de tu aplicación.

### Qué hace el workflow

1. **Comprueba la configuración.** Si falta un secreto, se detiene educadamente con un aviso ("Heroku deployment skipped") en lugar de fallar.
2. **Prepara la aplicación.** Crea la aplicación de Heroku si aún no existe, en el stack **heroku-26**, y configura el buildpack **emk/rust**. Una aplicación en el stack **container** pasa a heroku-26.
3. **Construye en Heroku.** Sube el código fuente del commit a Heroku, que lo compila con el buildpack (`RustConfig` y luego `heroku-build.sh`) y lo **publica** como nueva versión del proceso `web`. El registro de la compilación se muestra en la ejecución; el buildpack guarda las dependencias compiladas en su caché, así que los despliegues posteriores son más rápidos.
4. **Comprueba el estado** del sitio: llama a `/healthz` en tu aplicación durante un máximo de cinco minutos e informa del resultado.

Con `HEROKU_BUILD` en `docker`, el paso 2 pone la aplicación en el stack **container**, y el paso 3 construye la imagen Docker a partir del `Dockerfile` de la raíz en las máquinas de GitHub (reutilizando una caché de compilación entre ejecuciones), la sube al registro de contenedores de Heroku (`registry.heroku.com`) y la publica.

Solo se ejecuta un despliegue a la vez; un nuevo push espera a que termine el despliegue en curso.

## Método 3: la integración de Heroku con GitHub

Heroku puede vigilar tu repositorio y construir él mismo cada nuevo commit.

1. Crea primero la aplicación con el método 1 o el 2 (ambos configuran el stack **heroku-26** y el buildpack **emk/rust** que necesita este método). Para una aplicación creada de otra forma, abre **Settings → Buildpacks → Add buildpack**, escribe `emk/rust` y toca **Save changes**; la aplicación no debe estar en el stack **container**.
2. En el panel de Heroku, abre tu aplicación y toca la pestaña **Deploy**.
3. En **Deployment method**, toca **GitHub**, luego **Connect to GitHub**, y autoriza a Heroku.
4. Busca tu repositorio (`Memoire`) y toca **Connect**.
5. En **Automatic deploys**, elige la rama `master` y toca **Enable Automatic Deploys**. Si quieres, marca "Wait for CI to pass before deploy" (esperar a que pase la CI antes de desplegar).
6. Para desplegar de inmediato, desplázate hasta **Manual deploy** y toca **Deploy Branch**.

A partir de ahí, Heroku construye el sitio con el buildpack (`RustConfig`, `heroku-build.sh`, `Procfile`) en cada push a `master`.

## Desde un ordenador: la CLI de Heroku

Sin GitHub Actions, la [CLI de Heroku](https://devcenter.heroku.com/articles/heroku-cli) despliega el repositorio con un simple `git push`. En tu copia del repositorio:

```bash
heroku create my-memoire --stack heroku-26 --buildpack emk/rust
git push heroku master
```

Para una aplicación que ya existe (en el stack container, por ejemplo), configura primero su stack y su buildpack:

```bash
heroku git:remote -a my-memoire
heroku stack:set heroku-26
heroku buildpacks:set emk/rust
git push heroku master
```

## Instalar el sitio en tu móvil

El sitio es una aplicación web progresiva (PWA), servida por Heroku:

- **Android** (Chrome): abre el sitio, toca el menú **⋮** y luego **Instalar aplicación** (o **Añadir a pantalla de inicio**).
- **iPhone y iPad** (Safari): abre el sitio, toca **Compartir** y luego **Añadir a pantalla de inicio**.

Desde entonces se abre a pantalla completa desde su icono, como una aplicación. Una vez abierta una página, el sitio sigue funcionando sin red: las páginas de inicio, la primera página de la documentación, el playground y todas las páginas que ya leíste se guardan en el dispositivo. El playground no necesita red: el ORM se ejecuta en la página, compilado a WebAssembly. Una nueva versión del sitio reemplaza la guardada en la siguiente visita con red.

## Cómo un merge desencadena un despliegue

Las pull requests dirigidas a `master` se **fusionan automáticamente** en cuanto todas las comprobaciones de CI están en verde (salvo que la pull request sea un borrador, venga de un fork o tenga la etiqueta `no-auto-merge`). Tras el merge, el workflow de CI lanza explícitamente el workflow **Deploy to Heroku**, porque los merges realizados por GitHub Actions no desencadenan otros workflows por sí solos.

El resultado: apruebas o abres una pull request desde el móvil, esperas a las comprobaciones y la nueva versión se publica sola (con el método 2). Con el método 3, Heroku ve el nuevo commit en `master` y lo construye.

## Solución de problemas

| Problema | Dónde mirar / qué hacer |
|---|---|
| La compilación falla (métodos 1 y 3) | Panel de Heroku → tu aplicación → pestaña **Activity** → **View build log** |
| La compilación falla (método 2) | App de GitHub → **Actions** → la ejecución fallida → el paso en rojo |
| El sitio no se abre, o aparece "Application error" | Panel de Heroku → tu aplicación → **More** (arriba a la derecha) → **View logs** |
| El workflow dice "Heroku deployment skipped" | Falta `HEROKU_API_KEY` o `HEROKU_APP_NAME` (revisa cómo están escritos) |
| `401` o "Invalid credentials" en el workflow | La clave de API es incorrecta, ha caducado o se ha revocado: crea una nueva y actualiza el secreto |
| "Name is already taken" | Los nombres de las aplicaciones son globales en Heroku: elige otro `HEROKU_APP_NAME` |
| Heroku pide verificar la cuenta | Añade un método de pago en **Account settings → Billing** |
| "No default language could be detected" | La aplicación no tiene buildpack: **Settings → Buildpacks → Add buildpack** → `emk/rust` (la aplicación no debe estar en el stack container), o lanza una vez el método 2 |
| La compilación pide un Rust más reciente | Sube `VERSION` en `RustConfig`, y con ella la versión de Rust del `Dockerfile` |

Conviene saber que:

- **Comprobación de estado:** abre `https://<your-app-address>/healthz` en tu navegador. El servidor responde `ok` cuando está en marcha. La dirección de tu aplicación aparece en el botón **Open app** del panel.
- **Puerto:** Heroku elige el puerto y lo pasa en la variable de entorno `PORT`; el servidor del sitio web lo lee automáticamente (8080 solo es el valor por defecto para las ejecuciones en local). No hay nada que configurar.
- **Costes:** el sitio web usa un dyno *Basic*. Puedes cambiar a un dyno *Eco* más barato (que se duerme cuando nadie lo visita) en la pestaña **Resources**, o escalar ahí el dyno a cero para dejar de pagar por él.

## Desplegar tu propia aplicación con Memoire y Heroku Postgres

Las mismas herramientas sirven para tu propia aplicación web en Rust que guarda sus datos con Memoire.

### 1. Añadir una base de datos

1. En el panel de Heroku, abre tu aplicación y toca la pestaña **Resources**.
2. En **Add-ons**, busca **Heroku Postgres**, elige un plan y envía el formulario.
3. Heroku crea la base de datos y añade a tu aplicación una config var `DATABASE_URL` (visible en **Settings → Reveal Config Vars**).

### 2. Conectarse desde tu código

Heroku Postgres solo acepta conexiones cifradas, así que activa la feature `postgres-tls`:

```toml
[dependencies]
memoire-orm = { git = "https://github.com/maxlestage/Memoire", features = ["postgres-tls"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Después lee `DATABASE_URL`, ejecuta tus migraciones al arrancar y escucha en el puerto que te asigna Heroku:

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
    // Heroku define DATABASE_URL cuando añades Heroku Postgres.
    let url = std::env::var("DATABASE_URL")?;
    let url = if url.contains('?') { url } else { format!("{url}?sslmode=require") };
    let db = PgPool::connect_with(&url, 10).await?;

    Migrator::new()
        .add(Migration::new(1, "create_visits").create_table::<Visit>())
        .run(&db)
        .await?;

    // Heroku le indica a tu servidor web qué puerto debe usar.
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
    println!("ready to listen on 0.0.0.0:{port}");
    // Arranca aquí tu servidor web (Vitesse: `app.state(db)` comparte el pool con tus handlers).
    Ok(())
}
```

Mantén el pool pequeño: los planes más pequeños de Heroku Postgres solo aceptan un número limitado de conexiones (consulta tu plan), compartidas por todos tus dynos. Consulta [PostgreSQL](postgres) para ver las opciones de TLS.

### Otras bases de datos en Heroku

Los demás backends de servidor funcionan igual: añade el servicio y luego lee su URL desde una config var.

| Base de datos | Cómo obtenerla en Heroku | Feature de Memoire | Conexión |
|---|---|---|---|
| Redis | Add-on **Heroku Key-Value Store**, que define `REDIS_URL` | `redis-tls` | `RedisDb::connect(&format!("{url}#insecure"))` (su certificado es autofirmado; consulta [Redis](redis)) |
| MySQL / MariaDB | Un add-on de MySQL del marketplace Elements, que pone una URL `mysql://` en una config var | `mysql` (o `mysql-tls`) | `MySqlPool::connect(&url)` (consulta [MySQL y MariaDB](mysql)) |
| MongoDB | Un clúster de MongoDB Atlas; copia su URI `mongodb+srv://` en una config var como `MONGODB_URI` (**Settings → Config Vars**) | `mongodb` | `MongoDb::connect(&uri)` (consulta [MongoDB](mongodb)) |

### 3. Construir y desplegar

Lo más sencillo es el buildpack de Rust. Para un único crate cuyo binario, aquí `my-app`, es la aplicación, añade un `Procfile` en la raíz de tu repositorio:

```text
web: ./target/release/my-app
```

Y, si quieres, para fijar la versión de Rust, un `RustConfig`:

```text
VERSION=1.99
```

El buildpack ejecuta un simple `cargo build --release`, conserva los binarios de `target/release` y guarda las dependencias compiladas en su caché entre compilaciones. Si hace falta más (un paquete de un workspace, un ejemplo, un bundle WebAssembly), haz como el repositorio de Memoire: en `RustConfig`, `RUST_SKIP_BUILD=1` omite la compilación del propio buildpack, y un `trap` ejecuta un script de compilación una vez instalado Rust (consulta su `RustConfig` y su `heroku-build.sh`). El soporte de TLS de Memoire es Rust puro (`rustls`), así que nada necesita OpenSSL. Después copia `.github/workflows/deploy-heroku.yml` en tu repositorio, añade los dos secretos como en el método 2, y cada push a `master` desplegará tu aplicación (el método 3 también funciona, una vez añadido el buildpack `emk/rust` a la aplicación).

Para construir en su lugar una imagen Docker, añade a tu repositorio un `Dockerfile` y un `heroku.yml`, y pon la variable `HEROKU_BUILD` en `docker` (método 2). Un `Dockerfile` mínimo:

```dockerfile
FROM rust:1-slim-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/my-app /usr/local/bin/my-app
CMD ["my-app"]
```

Y `heroku.yml`:

```yaml
build:
  docker:
    web: Dockerfile
```

También aquí, la imagen final no necesita OpenSSL.

## Próximos pasos

- [PostgreSQL](postgres): cadenas de conexión, TLS y tamaño del pool.
- [Migraciones](migrations): mantén actualizado el esquema de producción en cada despliegue.
- [Publicar en crates.io](publish-crates-io): publica una nueva versión del crate, también desde el móvil.
