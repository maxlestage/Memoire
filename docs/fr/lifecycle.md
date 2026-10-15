# Horodatage, suppression douce et hooks

Trois comportements que l'on active modèle par modèle avec un attribut, dans l'esprit d'Eloquent de Laravel (`$timestamps`, `SoftDeletes`, événements de modèle) et des callbacks de cycle de vie de Doctrine (`prePersist`, `preUpdate`, `preRemove`...). Ils fonctionnent de la même façon sur toutes les bases, et un modèle qui ne les demande pas se comporte exactement comme avant.

```rust
use memoire_orm::lifecycle::Hooks;
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "articles", timestamps, soft_deletes, hooks)]
pub struct Article {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub slug: String,
    pub created_at: Option<i64>, // millisecondes depuis l'epoch Unix
    pub updated_at: Option<i64>,
    pub deleted_at: Option<i64>,
}
```

## Horodatage automatique

`#[memoire(timestamps)]` sur la structure utilise les champs nommés `created_at` et `updated_at`. Pour d'autres noms, marquez plutôt les champs : `#[memoire(created_at)]` et `#[memoire(updated_at)]` (un seul de chaque).

| Opération | `created_at` | `updated_at` |
|---|---|---|
| `insert`, `save` d'une nouvelle ligne | rempli s'il est vide (`None`, `0` ou `""`), réécrit dans la structure | idem |
| `update` | inchangé | maintenant (en base ; la structure empruntée garde sa valeur) |
| `save` d'une ligne existante | inchangé | maintenant, réécrit dans la structure |
| `update_only(&db, colonnes)` | inchangé | maintenant, ajouté aux colonnes |
| `touch(&mut self, &db)` | inchangé | maintenant, et rien d'autre |
| `update_many()` | inchangé | maintenant, sauf si vous le fixez avec `set` |
| `insert_many` | rempli s'il est vide | rempli s'il est vide |
| `upsert` | rempli s'il est vide | maintenant |
| `delete` doux, `restore` | inchangé | maintenant |

```rust
let mut a = Article { id: 0, title: "Bonjour".into(), slug: String::new(),
                      created_at: None, updated_at: None, deleted_at: None };
a.insert(&db).await?;
assert!(a.created_at.is_some() && a.updated_at == a.created_at);

a.title = "Bonjour, le monde".into();
a.save(&db).await?; // updated_at avance, en base et dans `a`
```

La valeur dépend du type du champ ; tout autre type est refusé à la compilation :

| Type du champ | Valeur stockée |
|---|---|
| `i64` | millisecondes depuis l'epoch Unix |
| `i32` | secondes depuis l'epoch Unix |
| `f64` | secondes depuis l'epoch Unix, avec une fraction |
| `String` | RFC 3339 en UTC, `2026-10-15T08:30:00.123Z` (se trie comme le temps) |
| `chrono::DateTime<Utc>`, `NaiveDateTime` (UTC), `NaiveDate` | maintenant (feature `chrono`) |

Chacun peut être un `Option<...>`, la façon la plus claire de dire « pas encore rempli ». En WebAssembly, activez la feature `web` pour que l'heure vienne de l'horloge du navigateur.

## Suppression douce (soft delete)

Avec `#[memoire(soft_deletes)]` (un champ nullable nommé `deleted_at`, ou tout champ horodaté `Option<...>` marqué `#[memoire(soft_delete)]`), supprimer une ligne ne fait que remplir cette colonne, et la ligne disparaît de toutes les requêtes construites avec `Model::query()`, dont `find`, `get`, `count`, `paginate` et le chargement des relations :

```rust
a.delete(&db).await?;                                     // UPDATE ... SET deleted_at = maintenant
assert_eq!(Article::find(&db, a.id).await?, None);         // masquée
assert_eq!(Article::with_trashed().count(&db).await?, 1);  // toujours stockée
let corbeille = Article::only_trashed().all(&db).await?;   // seulement les lignes supprimées

let mut a = corbeille[0].clone();
assert!(a.trashed());
a.restore(&db).await?;      // deleted_at redevient NULL
a.force_delete(&db).await?; // DELETE, définitivement
```

| Méthode | Effet |
|---|---|
| `ligne.delete(&db)` / `Model::delete_by_key(&db, clé)` | Suppression douce ; `false` si déjà supprimée |
| `Model::delete_many().filter(...).execute(&db)` | Supprime en douceur les lignes trouvées |
| `Model::delete_many().force()...` / `ligne.force_delete(&db)` | Vrai `DELETE` |
| `ligne.restore(&db)` | Vide `deleted_at` |
| `ligne.trashed()` | `true` si `deleted_at` est rempli |
| `Model::with_trashed()`, `query().with_trashed()` | Toutes les lignes |
| `Model::only_trashed()`, `query().only_trashed()` | Seulement les lignes supprimées |
| `query().without_trashed()` | Retour au comportement par défaut |

La portée s'applique à toute la condition : `filter(a).or_filter(b)` devient `deleted_at IS NULL AND (a OR b)`, jamais une ligne supprimée qui vérifie `b`. `update_many` et `reload` voient toutes les lignes, comme une table jointe.

> **Astuce** Une colonne `unique` compte toujours les lignes supprimées en douceur : un utilisateur supprimé garde son email. Restaurez la ligne, ou supprimez-la avec `force_delete`, avant de réutiliser la valeur.

## Hooks de cycle de vie

Ajoutez `hooks` à l'attribut de la structure et implémentez `memoire_orm::lifecycle::Hooks`, en ne redéfinissant que les méthodes utiles :

```rust
impl Hooks for Article {
    fn before_save(&mut self) -> Result<()> {
        if self.title.trim().is_empty() {
            return Err(Error::Validation("un article a besoin d'un titre".into()));
        }
        self.slug = self.title.to_lowercase().replace(' ', "-");
        Ok(())
    }

    fn after_delete(&self) {
        println!("article {} supprimé", self.id);
    }
}
```

| Hook | Appelé par | Doctrine / Eloquent |
|---|---|---|
| `before_save(&mut self)` | `insert`, `save` | `saving` |
| `before_insert(&mut self)` / `after_insert(&self)` | `insert`, `save` d'une nouvelle ligne | `prePersist` / `postPersist`, `creating` / `created` |
| `before_update(&self)` / `after_update(&self)` | `update`, `update_only`, `save` d'une ligne existante | `preUpdate` / `postUpdate`, `updating` / `updated` |
| `before_delete(&self)` / `after_delete(&self)` | `delete` (doux aussi), `force_delete` | `preRemove` / `postRemove`, `deleting` / `deleted` |

Un hook `before_*` qui renvoie une erreur annule l'écriture : rien n'est envoyé à la base et l'erreur revient à l'appelant. `Error::Validation(message)` est fait pour cela (`err.is_validation()`).

Les hooks sont synchrones et ne voient que la ligne. Les écritures de plusieurs lignes à la fois (`insert_many`, `upsert`, `update_many`, `delete_many`) n'en déclenchent aucun, comme les mises à jour en masse d'Eloquent. Pour réagir à chaque écriture avec un accès à la base (journal d'audit, cache), utilisez plutôt les [notifications de changements](events).

## `save`

`save(&mut self, &db)` est l'appel unique pour « enregistrer cette ligne », comme `save()` d'Eloquent : une ligne dont la clé `auto_increment` vaut encore `0` est insérée, toute autre ligne est mise à jour (et insérée si aucune ligne n'a sa clé). Il exécute `before_save`, puis les hooks d'insertion ou de mise à jour, et réécrit l'horodatage dans la structure.

## Étapes suivantes

- [Créer, lire, modifier, supprimer](crud) : `first_or_create`, `update_or_create` et les autres écritures.
- [Constructeur de requêtes](queries) : lecture par paquets et pagination par curseur.
- [Journal des requêtes](query-log) : voir les instructions que ces méthodes exécutent.
