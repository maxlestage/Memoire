# Publicar en crates.io

Memoire ORM se publica en [crates.io](https://crates.io/crates/memoire-orm), el registro de paquetes de Rust, como dos crates: `memoire-orm-derive` (las macros derive) y `memoire-orm` (el ORM, que depende del primero). Esta página explica cómo se publica una nueva versión, ya sea con un workflow de GitHub que puedes lanzar desde el móvil, o a mano desde un ordenador.

## Cómo funcionan las publicaciones

El workflow `.github/workflows/publish.yml`, llamado **Publish to crates.io**, se ejecuta cuando:

- se hace push de un tag cuyo nombre empieza por `v` (por ejemplo, `v0.2.0`), o
- se publica una **release** de GitHub, o
- lo lanzas a mano desde la pestaña **Actions** (por defecto como ejecución de prueba, o dry run, que lo comprueba todo sin subir nada).

Cuando se ejecuta:

1. lee la versión del workspace y, en el caso de un tag, **comprueba que el tag coincide con ella** (el tag `v0.2.0` exige la versión `0.2.0`; si no, se detiene con un error);
2. ejecuta la batería de pruebas (backends en memoria y SQLite);
3. publica **primero `memoire-orm-derive` y después `memoire-orm`**, porque `memoire-orm` depende exactamente de la misma versión del crate derive;
4. omite cualquier crate cuya versión ya esté en crates.io, así que volver a ejecutarlo siempre es seguro;
5. escribe un resumen con un enlace a la nueva versión.

## Configuración inicial (una sola vez): el token de crates.io

El workflow necesita un token de API de crates.io guardado como secreto del repositorio con el nombre `CARGO_REGISTRY_TOKEN`.

### Crear el token (desde el móvil)

1. Abre `https://crates.io` en el navegador del móvil y pulsa **Log in with GitHub** (iniciar sesión con GitHub).
2. Asegúrate de que tu dirección de email esté verificada (si no, crates.io se niega a publicar): abre **Account Settings** y revisa la sección del email.
3. En **Account Settings**, abre **API Tokens** y toca **New Token**.
4. Ponle un nombre (por ejemplo, "GitHub Actions Memoire") y, si quieres, una fecha de caducidad.
5. Selecciona los permisos (scopes) **publish-new** (necesario la primera vez que se publica un nombre de crate) y **publish-update** (necesario para las nuevas versiones). También puedes restringir el token a los crates `memoire-orm` y `memoire-orm-derive`.
6. Toca **Generate Token** y cópialo. crates.io solo lo muestra una vez.

### Guardarlo en GitHub

1. En el navegador del móvil (los secretos no se pueden editar en la app de GitHub), abre el repositorio y luego **Settings → Secrets and variables → Actions**.
2. Toca **New repository secret**.
3. Nombre (Name): `CARGO_REGISTRY_TOKEN`. Valor (Value): el token. Toca **Add secret**.

> **Nota** Solo los propietarios de un crate en crates.io pueden publicar nuevas versiones de él. Si trabajas en un fork, los nombres de los crates ya pertenecen a los autores originales.

## Publicar una nueva versión

### 1. Subir el número de versión

Cada publicación necesita un número de versión nuevo (crates.io nunca acepta dos veces la misma versión). Sigue el [versionado semántico](https://semver.org): incrementa el último número para las correcciones (`0.1.0` → `0.1.1`) y el del medio para las nuevas funcionalidades o, antes de la 1.0, para los cambios incompatibles (`0.1.1` → `0.2.0`).

Hay que cambiar tres sitios:

| Archivo | Qué cambiar |
|---|---|
| `Cargo.toml` (raíz del repositorio), `[workspace.package]` | `version = "0.2.0"`: ambos crates toman su versión de aquí |
| `Cargo.toml` (raíz del repositorio), `[workspace.dependencies]` | `memoire-orm-derive = { version = "=0.2.0", path = "crates/memoire-orm-derive" }`: la versión fijada con exactitud debe coincidir con la nueva versión |
| `CHANGELOG.md` | Mueve los elementos de `[Unreleased]` a una nueva sección `[0.2.0] - YYYY-MM-DD` |

El `=` de `"=0.2.0"` fija `memoire-orm` exactamente al crate derive correspondiente, de modo que los dos crates siempre se usan juntos.

Desde el móvil, edita estos archivos en el sitio web de GitHub: abre el archivo, toca el icono del lápiz, haz el cambio y elige **Create a new branch for this commit and start a pull request** (crear una rama nueva para este commit y abrir una pull request). En cuanto pasen las comprobaciones de CI, la pull request se fusiona automáticamente en `master`.

### 2. Crear la release

Una vez que el cambio de versión esté en `master`:

1. En el navegador del móvil, abre la página **Releases** del repositorio y toca **Draft a new release** (o ve directamente a `https://github.com/<owner>/Memoire/releases/new`).
2. Toca **Choose a tag**, escribe el nuevo tag, por ejemplo `v0.2.0`, y selecciona **Create new tag: v0.2.0 on publish**. Deja `master` como destino (target).
3. Escribe un título (por ejemplo, `v0.2.0`) y las notas de la versión; el botón **Generate release notes** las rellena a partir de las pull requests fusionadas.
4. Toca **Publish release**.

El workflow arranca en cuestión de segundos. Síguelo en la app de GitHub, en **Actions → Publish to crates.io**. Cuando esté en verde, la nueva versión estará en crates.io, y docs.rs generará la documentación de la API poco después.

> **Consejo** Publicar una release también crea el tag, así que puede que el workflow arranque dos veces. No pasa nada: las ejecuciones se ponen en cola una detrás de otra, y la segunda omite los crates que ya están publicados.

### Comprobar sin publicar

Para comprobar que todo se publicaría correctamente, sin subir nada:

1. En la app de GitHub, abre **Actions → Publish to crates.io**.
2. Toca **Run workflow**, deja **dry_run** activado y ejecútalo.

Esto ejecuta `cargo publish --workspace --dry-run`, que empaqueta ambos crates y los compila como lo haría crates.io.

## Publicar a mano

Desde un ordenador con Rust instalado, después de subir la versión como se ha explicado:

```bash
# Una sola vez: guarda tu token de crates.io en local
cargo login

# Comprueba todo sin subir nada (Cargo 1.90 o posterior verifica los dos crates juntos)
cargo publish --workspace --dry-run

# Publica, primero el crate derive
cargo publish -p memoire-orm-derive && cargo publish -p memoire-orm

# Etiqueta la versión
git tag v0.2.0
git push origin v0.2.0
```

Con un Cargo más antiguo, comprueba el crate derive con `cargo publish -p memoire-orm-derive --dry-run`; un dry run de `memoire-orm` por sí solo solo funciona cuando la versión correspondiente del crate derive ya está en crates.io.

## Lista de comprobación de la publicación

- [ ] `version` actualizada en `[workspace.package]` del `Cargo.toml` raíz
- [ ] Versión fijada `memoire-orm-derive = { version = "=X.Y.Z", ... }` actualizada a la misma versión
- [ ] `CHANGELOG.md` actualizado con la nueva versión y la fecha
- [ ] CI en verde en `master`
- [ ] El dry run pasa (`cargo publish --workspace --dry-run`, o el workflow con **dry_run**)
- [ ] Release `vX.Y.Z` publicada (o tag subido con push)
- [ ] Nueva versión visible en crates.io y documentación generada en docs.rs

> **Atención** Una versión publicada nunca se puede cambiar ni eliminar. Si una versión está rota, publica una versión corregida y haz *yank* de la rota (`cargo yank memoire-orm --version X.Y.Z`, y lo mismo con `memoire-orm-derive` si hace falta), lo que impide que los proyectos nuevos la seleccionen.

## Próximos pasos

- [Desplegar en Heroku desde el móvil](deploy-heroku-mobile): pon el sitio web en línea.
- [Instalación](installation): cómo añaden los usuarios el crate publicado.
- [Preguntas frecuentes](faq): incluido cómo contribuir.
