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

| Método | Esfuerzo | Actualizaciones tras el primer despliegue | Dónde se construye la imagen |
|---|---|---|---|
| 1. Botón "Deploy to Heroku" | El mínimo: unos pocos toques | Ninguna: despliega una sola vez | En Heroku |
| 2. Workflow de GitHub Actions | Medio: hay que añadir dos secretos | Automáticas en cada push a `master`, o con un toque en la app de GitHub | En GitHub, y luego se envía a Heroku |
| 3. Integración de Heroku con GitHub | Medio: conectar GitHub con Heroku | Automáticas en cada push a `master` | En Heroku |

Una combinación habitual: el método 1 para el primerísimo despliegue y luego el método 2 o el 3 para mantener el sitio actualizado. No actives los métodos 2 y 3 a la vez, o cada push se desplegará dos veces.

> **Consejo** ¿No tienes cuenta de Heroku, o no tienes presupuesto? El sitio también se puede publicar **gratis en GitHub Pages**, cambiando un solo ajuste: consulta la sección GitHub Pages más abajo.

## Método 1: el botón "Deploy to Heroku"

El README del repositorio tiene un botón morado **Deploy to Heroku** ("Desplegar en Heroku"). Apunta a:

```text
https://heroku.com/deploy?template=https://github.com/maxlestage/Memoire
```

1. Abre el repositorio en GitHub (app o navegador) y desplázate hasta el principio del README.
2. Toca **Deploy to Heroku**. Tu navegador abre Heroku; inicia sesión si te lo pide.
3. Elige un **App name** (nombre de la aplicación: letras minúsculas, dígitos y guiones, único en todo Heroku) y una **region** (región: United States o Europe).
4. Deja el ajuste `RUST_LOG` en `info`.
5. Toca **Deploy app**.
6. Espera a que termine la compilación. La primera compila todo el sitio web desde el código fuente, lo que puede tardar 10 minutos o más. Puedes seguirla en directo en la página.
7. Cuando Heroku muestre "Your app was successfully deployed" (tu aplicación se ha desplegado correctamente), toca **View** para abrir tu sitio.

Lo que ocurre entre bastidores:

- `app.json` describe la aplicación para Heroku: su nombre y su descripción, el stack **container**, un dyno `web` de tamaño *Basic* y el ajuste `RUST_LOG`.
- `heroku.yml` le indica a Heroku que construya el proceso `web` a partir del `Dockerfile` de la raíz del repositorio.
- El `Dockerfile` compila el sitio web Yew (compilado a WebAssembly con Trunk) y el pequeño servidor web en Rust en una primera etapa, y luego copia solo el binario del servidor y los archivos estáticos en una imagen final ligera.

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

### Paso 3: desplegar

A partir de ahora, cualquier push a `master` despliega automáticamente. Para desplegar en el acto desde la **app móvil de GitHub**:

1. Abre tu repositorio en la app.
2. Toca **Actions** (en el menú del repositorio; puede que antes tengas que tocar **More**).
3. Toca el workflow **Deploy to Heroku**.
4. Toca **Run workflow**, deja la rama `master` y confirma con **Run workflow**.
5. Toca la nueva ejecución para seguir su progreso. Cuando termine, su resumen muestra "Deployed to" seguido de la dirección de tu aplicación.

### Qué hace el workflow

1. **Comprueba la configuración.** Si falta un secreto, se detiene educadamente con un aviso ("Heroku deployment skipped") en lugar de fallar.
2. **Prepara la aplicación.** Crea la aplicación de Heroku si aún no existe y se asegura de que use el stack **container**.
3. **Construye la imagen Docker** a partir del `Dockerfile` de la raíz en las máquinas de GitHub, reutilizando una caché de compilación entre ejecuciones para que los despliegues posteriores sean más rápidos.
4. **Sube la imagen** al registro de contenedores de Heroku (`registry.heroku.com`) y la **publica** como nueva versión del proceso `web`.
5. **Comprueba el estado** del sitio: llama a `/healthz` en tu aplicación durante un máximo de cinco minutos e informa del resultado.

Solo se ejecuta un despliegue a la vez; un nuevo push espera a que termine el despliegue en curso.

## Método 3: la integración de Heroku con GitHub

Heroku puede vigilar tu repositorio y construir él mismo cada nuevo commit.

1. Crea primero la aplicación con el método 1 o el 2 (ambos configuran el stack **container** que necesita este método).
2. En el panel de Heroku, abre tu aplicación y toca la pestaña **Deploy**.
3. En **Deployment method**, toca **GitHub**, luego **Connect to GitHub**, y autoriza a Heroku.
4. Busca tu repositorio (`Memoire`) y toca **Connect**.
5. En **Automatic deploys**, elige la rama `master` y toca **Enable Automatic Deploys**. Si quieres, marca "Wait for CI to pass before deploy" (esperar a que pase la CI antes de desplegar).
6. Para desplegar de inmediato, desplázate hasta **Manual deploy** y toca **Deploy Branch**.

A partir de ahí, Heroku construye la imagen a partir de `heroku.yml` y del `Dockerfile` en cada push a `master`.

## GitHub Pages: la opción gratuita

El sitio es una aplicación WebAssembly hecha de archivos estáticos, así que GitHub Pages también puede alojarlo, gratis, en `https://<tu-usuario>.github.io/Memoire/` (para este repositorio: `https://maxlestage.github.io/Memoire/`). Sin cuenta de Heroku y sin secretos: el workflow `.github/workflows/pages.yml` lo compila y lo publica.

Actívalo una sola vez:

1. En el navegador del móvil, abre tu repositorio en `https://github.com` y luego **Settings → Pages**.
2. En **Build and deployment → Source**, elige **GitHub Actions**.
3. En la app de GitHub, abre **Actions → Deploy to GitHub Pages → Run workflow**. Dos o tres minutos después el sitio está en línea; su dirección aparece en la página de la ejecución.

A partir de ahí, cada push a `master` que cambie el sitio, la documentación o el ORM publica la nueva versión, igual que cada pull request fusionada automáticamente.

> **Nota** Con un plan gratuito de GitHub, GitHub Pages solo funciona con repositorios públicos.

Qué hace el workflow:

- se omite con el aviso "GitHub Pages deployment skipped" mientras no hayas hecho el paso 2;
- compila el sitio con `site/build-static.sh`, que lo coloca bajo `/Memoire/` y escribe una copia de la página para cada dirección (`docs/models`, `playground`...), de modo que los enlaces a cualquier página y las recargas funcionan; las direcciones desconocidas muestran la página «no encontrada» del sitio;
- publica el sitio y luego comprueba que la página de inicio y una página de la documentación responden.

GitHub Pages y Heroku son independientes: puedes usar los dos. La única diferencia es que GitHub Pages no puede enviar las cabeceras de seguridad adicionales (como `Content-Security-Policy`) que añade el servidor de Heroku; el sitio funciona igual.

> **Consejo** Para usar tu propio dominio, configúralo en **Settings → Pages → Custom domain** y vuelve a lanzar el workflow: compilará el sitio para la raíz de ese dominio.

## Cómo un merge desencadena un despliegue

Las pull requests dirigidas a `master` se **fusionan automáticamente** en cuanto todas las comprobaciones de CI están en verde (salvo que la pull request sea un borrador, venga de un fork o tenga la etiqueta `no-auto-merge`). Tras el merge, el workflow de CI lanza explícitamente los workflows **Deploy to Heroku** y **Deploy to GitHub Pages**, porque los merges realizados por GitHub Actions no desencadenan otros workflows por sí solos.

El resultado: apruebas o abres una pull request desde el móvil, esperas a las comprobaciones y la nueva versión se publica sola (con el método 2). Con el método 3, Heroku ve el nuevo commit en `master` y lo construye.

## Solución de problemas

| Problema | Dónde mirar / qué hacer |
|---|---|
| La compilación falla (métodos 1 y 3) | Panel de Heroku → tu aplicación → pestaña **Activity** → **View build log** |
| La compilación falla (método 2) | App de GitHub → **Actions** → la ejecución fallida → el paso en rojo |
| El sitio no se abre, o aparece "Application error" | Panel de Heroku → tu aplicación → **More** (arriba a la derecha) → **View logs** |
| El workflow dice "Heroku deployment skipped" | Falta `HEROKU_API_KEY` o `HEROKU_APP_NAME` (revisa cómo están escritos) |
| El workflow dice "GitHub Pages deployment skipped" | **Settings → Pages → Source** no está configurado en **GitHub Actions** |
| `401` o "Invalid credentials" en el workflow | La clave de API es incorrecta, ha caducado o se ha revocado: crea una nueva y actualiza el secreto |
| "Name is already taken" | Los nombres de las aplicaciones son globales en Heroku: elige otro `HEROKU_APP_NAME` |
| Heroku pide verificar la cuenta | Añade un método de pago en **Account settings → Billing** |
| El método 3 no consigue detectar un buildpack | La aplicación no está en el stack container: créala con el método 1 o el 2 |

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
memoire-orm = { version = "0.1", features = ["postgres-tls"] }
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
    // Arranca aquí tu framework web (axum, actix-web...), compartiendo `db` con tus handlers.
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

Añade a tu repositorio un `Dockerfile` y un `heroku.yml`, como hace el repositorio de Memoire. Un `Dockerfile` mínimo para un binario de Rust llamado `my-app`:

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

El soporte de TLS de Memoire es Rust puro (`rustls`), así que la imagen final no necesita OpenSSL. Después copia `.github/workflows/deploy-heroku.yml` en tu repositorio, añade los dos secretos como en el método 2, y cada push a `master` desplegará tu aplicación.

## Próximos pasos

- [PostgreSQL](postgres): cadenas de conexión, TLS y tamaño del pool.
- [Migraciones](migrations): mantén actualizado el esquema de producción en cada despliegue.
- [Publicar en crates.io](publish-crates-io): publica una nueva versión del crate, también desde el móvil.
