# Benchmarks

Cette page explique comment Memoire est mesuré, comment lancer les benchmarks sur votre propre machine et comment lire les résultats. L'objectif n'est pas seulement de montrer que Memoire est rapide, mais de mesurer honnêtement ce que coûte la couche ORM par rapport à du code écrit à la main.

## Méthodologie

Les benchmarks se trouvent dans `crates/memoire-orm/benches/orm.rs` et utilisent [Criterion](https://github.com/bheisler/criterion.rs), la bibliothèque de benchmark de référence en Rust, fondée sur les statistiques. Pour chaque opération, Criterion fait une phase de chauffe, exécute le code de nombreuses fois et indique le temps typique par exécution avec un intervalle de confiance.

Ils sont compilés avec le profil `bench` du workspace (`lto = "thin"`, `codegen-units = 1`), proche de la façon dont une application optimisée est compilée.

### Ce qui est mesuré

| Groupe | Benchmark | Ce qu'il fait |
|---|---|---|
| `sql_render` | `select_filter_order_limit` | Construit un `SELECT` avec un filtre, un tri et une limite, et le génère en SQL PostgreSQL (sans base de données) |
| `memory` | `find_by_pk_10k` | `User::find` par clé primaire dans une table de 10 000 lignes |
| `memory` | `filter_order_limit_10k` | Filtre (`between` et `gt`), tri et `limit(20)` sur 10 000 lignes |
| `memory` | `count_where_10k` | `count` avec un filtre sur 10 000 lignes |
| `memory` | `filter_indexed_eq_10k` | Filtre d'égalité sur une colonne `#[memoire(index)]` (une recherche dans l'index) sur 10 000 lignes |
| `memory` | `update_where_10k` | `update_many` avec un filtre et un `increment` sur 10 000 lignes |
| `memory` | `insert_one` | Insère une ligne |
| `memory` | `insert_many_1000` | `insert_many` de 1 000 lignes dans une table vide |
| `sqlite` | `find_by_pk/...` | Trouve une ligne par clé primaire parmi 10 000 lignes |
| `sqlite` | `select_1000_rows/...` | Sélectionne 1 000 lignes et les décode en structs |
| `sqlite` | `insert_one/...` | Insère une ligne et relit l'id généré |
| `sqlite` | `insert_1000_tx/...` | Insère 1 000 lignes dans une seule transaction |

### La référence : `rusqlite` écrit à la main

Chaque benchmark SQLite est exécuté deux fois :

- `.../memoire` : l'opération écrite avec Memoire ;
- `.../raw_rusqlite` : la même opération écrite à la main avec `rusqlite`, le driver qu'utilise Memoire lui-même, avec des requêtes préparées mises en cache et un mapping manuel des lignes.

La version écrite à la main constitue le *plancher* : aucun ORM pour SQLite ne peut être plus rapide que le driver sur lequel il repose. L'écart entre les deux lignes correspond exactement à ce que coûte l'ORM. Pour les insertions en masse, la référence est une boucle sur une requête préparée, c'est-à-dire la façon dont la plupart des gens l'écrivent à la main, tandis que Memoire utilise `insert_many`.

Pendant les benchmarks, SQLite tourne en mémoire, pour que la vitesse du disque ne brouille pas la comparaison.

## Lancer les benchmarks

Depuis la racine du dépôt :

```bash
cargo bench -p memoire-orm --features memory,sqlite
```

Une exécution complète prend quelques minutes. Pour ne lancer que certains benchmarks, passez un filtre après `--` ; il s'applique aux noms des benchmarks :

```bash
# Uniquement le groupe SQLite
cargo bench -p memoire-orm --features memory,sqlite -- sqlite

# Uniquement les recherches par clé primaire, sur tous les backends
cargo bench -p memoire-orm --features memory,sqlite -- find_by_pk
```

Les résultats s'affichent dans le terminal, et Criterion conserve les mesures brutes dans `target/criterion/`. Quand vous relancez les benchmarks, il les compare à l'exécution précédente et vous indique si les performances ont changé.

> **Astuce** Fermez les autres programmes gourmands pendant les benchmarks, et branchez votre portable sur le secteur : l'économie d'énergie et les tâches en arrière-plan rendent les résultats bruités.

## Résultats

<!-- BENCH_TABLE -->

## Comment lire les chiffres

- **Les temps sont donnés par opération** : plus c'est bas, mieux c'est. `ns` désigne une nanoseconde (un milliardième de seconde), `µs` une microseconde (1 000 ns) et `ms` une milliseconde (1 000 µs).
- **Criterion affiche trois valeurs**, par exemple `time: [961 ns 987 ns 1.01 µs]`. Celle du milieu est la meilleure estimation ; les deux autres bornent l'intervalle de confiance à 95 %. Quand les intervalles de deux benchmarks se chevauchent, considérez-les comme égaux.
- **Le ratio** compare Memoire à la référence écrite à la main. `1.10×` signifie que Memoire prend 10 % de temps en plus que du code écrit directement avec le driver ; en dessous de `1×`, Memoire est plus rapide.
- **Comparez des opérations, pas des backends.** Le moteur en mémoire et SQLite n'effectuent pas la même quantité de travail (SQLite analyse le SQL, gère des pages et un journal). Les chiffres montrent le coût de chaque opération sur chaque backend, pas quelle base de données est « meilleure ».

## En toute honnêteté

Les chiffres des benchmarks dépendent fortement de la machine (processeur, mémoire, système d'exploitation), de la version de Rust et des données. Les temps absolus sur votre ordinateur seront différents, peut-être beaucoup. Ce qui reste généralement stable d'une machine à l'autre, c'est le *ratio* entre Memoire et la référence écrite à la main.

Les micro-benchmarks mesurent par ailleurs de petites opérations isolées. Dans une application réelle, la latence réseau vers un serveur de base de données, la vitesse du disque et les requêtes elles-mêmes comptent généralement bien plus que la couche ORM. Mesurez votre propre charge de travail avant de tirer des conclusions, et consultez [Pourquoi c'est rapide](performance) pour des conseils qui font vraiment la différence.

## Étapes suivantes

- [Pourquoi c'est rapide](performance) : les choix de conception derrière les chiffres.
- [SQLite](sqlite) : le backend utilisé pour les comparaisons.
- [Moteur en mémoire](memory) : les algorithmes utilisés par `MemoryDb`.
