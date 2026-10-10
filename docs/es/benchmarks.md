# Benchmarks

Esta página explica cómo se miden las prestaciones de Memoire, cómo ejecutar los benchmarks en tu propia máquina y cómo interpretar los resultados. El objetivo no es solo demostrar que Memoire es rápido, sino medir con honestidad cuánto cuesta la capa del ORM en comparación con el código escrito a mano.

## Metodología

Los benchmarks están en `crates/memoire-orm/benches/orm.rs` y usan [Criterion](https://github.com/bheisler/criterion.rs), la biblioteca de benchmarking basada en estadística que es el estándar en Rust. Para cada operación, Criterion hace un calentamiento, ejecuta el código muchas veces e informa del tiempo típico por ejecución con un intervalo de confianza.

Se compilan con el perfil `bench` del workspace (`lto = "thin"`, `codegen-units = 1`), de forma parecida a como se compila una aplicación optimizada.

### Qué se mide

| Grupo | Benchmark | Qué hace |
|---|---|---|
| `sql_render` | `select_filter_order_limit` | Construye un `SELECT` con un filtro, una ordenación y un límite, y lo genera como SQL de PostgreSQL (sin ninguna base de datos de por medio) |
| `memory` | `find_by_pk_10k` | `User::find` por clave primaria en una tabla de 10 000 filas |
| `memory` | `filter_order_limit_10k` | Filtro (`between` y `gt`), ordenación y `limit(20)` sobre 10 000 filas |
| `memory` | `count_where_10k` | `count` con un filtro sobre 10 000 filas |
| `memory` | `insert_one` | Insertar una fila |
| `memory` | `insert_many_1000` | `insert_many` de 1000 filas en una tabla vacía |
| `sqlite` | `find_by_pk/...` | Buscar una fila por clave primaria entre 10 000 filas |
| `sqlite` | `select_1000_rows/...` | Seleccionar 1000 filas y decodificarlas en structs |
| `sqlite` | `insert_one/...` | Insertar una fila y leer de vuelta su id generado |
| `sqlite` | `insert_1000_tx/...` | Insertar 1000 filas dentro de una sola transacción |

### La referencia: `rusqlite` escrito a mano

Cada benchmark de SQLite se ejecuta dos veces:

- `.../memoire`: la operación escrita con Memoire;
- `.../raw_rusqlite`: la misma operación escrita a mano con `rusqlite`, el driver que usa el propio Memoire, con sentencias preparadas en caché y mapeo manual de las filas.

La versión escrita a mano es el *suelo*: ningún ORM sobre SQLite puede ser más rápido que el driver sobre el que está construido. La diferencia entre las dos líneas es exactamente lo que cuesta el ORM. Para las inserciones masivas, la referencia es un bucle con una sentencia preparada, que es como la mayoría de la gente lo escribe a mano, mientras que Memoire usa `insert_many`.

Durante los benchmarks, SQLite se ejecuta en memoria, para que la velocidad del disco no distorsione la comparación.

## Ejecutar los benchmarks

Desde la raíz del repositorio:

```bash
cargo bench -p memoire-orm --features memory,sqlite
```

Una ejecución completa tarda unos minutos. Para ejecutar solo algunos benchmarks, pasa un filtro después de `--`; se compara con los nombres de los benchmarks:

```bash
# Solo el grupo de SQLite
cargo bench -p memoire-orm --features memory,sqlite -- sqlite

# Solo las búsquedas por clave primaria, en todos los backends
cargo bench -p memoire-orm --features memory,sqlite -- find_by_pk
```

Los resultados se imprimen en el terminal, y Criterion guarda las mediciones sin procesar en `target/criterion/`. Cuando vuelves a ejecutar los benchmarks, los compara con la ejecución anterior y te indica si el rendimiento ha cambiado.

> **Consejo** Cierra otros programas pesados mientras haces los benchmarks y enchufa tu portátil: el ahorro de energía y el trabajo en segundo plano meten ruido en los resultados.

## Resultados

<!-- BENCH_TABLE -->

## Cómo interpretar las cifras

- **Los tiempos son por operación**: cuanto más bajos, mejor. `ns` es un nanosegundo (una milmillonésima de segundo), `µs` un microsegundo (1000 ns) y `ms` un milisegundo (1000 µs).
- **Criterion imprime tres valores**, por ejemplo `time: [961 ns 987 ns 1.01 µs]`. El del medio es la mejor estimación; los dos extremos delimitan el intervalo de confianza del 95 %. Cuando los intervalos de dos benchmarks se solapan, considéralos iguales.
- **La proporción** compara Memoire con la referencia escrita a mano. `1.10×` significa que Memoire tarda un 10 % más que el código escrito directamente contra el driver; por debajo de `1×`, Memoire es más rápido.
- **Compara operaciones, no backends.** El motor en memoria y SQLite hacen cantidades de trabajo distintas (SQLite analiza SQL, gestiona páginas y un journal). Las cifras muestran el coste de cada operación en cada backend, no qué base de datos es "mejor".

## Una nota honesta

Las cifras de los benchmarks dependen mucho de la máquina (procesador, memoria, sistema operativo), de la versión de Rust y de los datos. Los tiempos absolutos en tu ordenador serán distintos, quizá mucho. Lo que suele mantenerse estable de una máquina a otra es la *proporción* entre Memoire y la referencia escrita a mano.

Además, los microbenchmarks miden operaciones pequeñas y aisladas. En una aplicación real, la latencia de red hasta un servidor de base de datos, la velocidad del disco y las propias consultas suelen importar mucho más que la capa del ORM. Mide tu propia carga de trabajo antes de sacar conclusiones, y consulta [Por qué es rápido](performance) para ver consejos que marcan una diferencia real.

## Próximos pasos

- [Por qué es rápido](performance): las decisiones de diseño que hay detrás de las cifras.
- [SQLite](sqlite): el backend usado para las comparaciones.
- [Motor en memoria](memory): los algoritmos que usa `MemoryDb`.
