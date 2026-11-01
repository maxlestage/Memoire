//! [`Executor::push`]: brings a table up to date with its model, like
//! `drizzle-kit push` or `prisma db push`, without migration files.
//!
//! Only safe changes are made: a missing table is created, missing columns
//! and indexes are added. Nothing is ever dropped, renamed or retyped (a
//! column removed from the model stays in the table): for those, write a
//! [`Migration`](crate::Migration).

use crate::ast::Query;
use crate::error::{Error, Result};
use crate::executor::Executor;
use crate::model::Model;
use crate::schema::{ColumnDef, Dialect, TableSchema};
use crate::sql;
use crate::value::Value;

/// What [`Executor::push`] changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PushReport {
    /// The table did not exist and was created (with all its indexes).
    pub created: bool,
    /// Columns added to an existing table.
    pub added_columns: Vec<&'static str>,
    /// Indexes added to an existing table.
    pub added_indexes: Vec<String>,
}

impl PushReport {
    /// Nothing had to change: the table already matched the model.
    pub fn is_unchanged(&self) -> bool {
        !self.created && self.added_columns.is_empty() && self.added_indexes.is_empty()
    }
}

pub(crate) async fn push<E: Executor + ?Sized, M: Model>(db: &E) -> Result<PushReport> {
    let t = M::SCHEMA;
    let Some(d) = db.dialect() else {
        // The in-memory engine, Redis and MongoDB: the schema lives with the model.
        return match db.execute(Query::CreateTable { table: t, if_not_exists: false }).await {
            Ok(_) => Ok(PushReport { created: true, ..PushReport::default() }),
            Err(Error::TableExists(_)) => Ok(PushReport::default()),
            Err(e) => Err(e),
        };
    };

    let existing = names(db, columns_sql(d), t.name).await?;
    if existing.is_empty() {
        db.execute(Query::CreateTable { table: t, if_not_exists: true }).await?;
        return Ok(PushReport { created: true, ..PushReport::default() });
    }
    let has = |list: &[String], name: &str| list.iter().any(|n| n.eq_ignore_ascii_case(name));

    // Every missing column is checked before anything changes.
    let missing: Vec<&ColumnDef> = t.columns.iter().filter(|c| !has(&existing, c.name)).collect();
    for c in &missing {
        if c.primary_key || c.auto_increment {
            return Err(refuse(t, c, "it is the primary key: write a Migration"));
        }
        if !c.nullable && c.default.is_none() {
            return Err(refuse(
                t,
                c,
                "existing rows would have no value for it: make the field an `Option<T>`, or give it \
                 `#[memoire(default = \"...\")]`",
            ));
        }
    }

    let mut report = PushReport::default();
    for c in &missing {
        db.raw_execute(&sql::add_column(t, c, d), &[]).await?;
        report.added_columns.push(c.name);
    }

    // Indexes: those of the model, the unique ones of the added columns, and
    // PostgreSQL's full-text ones.
    let index = |name: &str, unique: bool, cols: &[&str]| match d {
        Dialect::MySql => sql::mysql_index_with_prefixes(t, name, unique, cols),
        _ => sql::create_index(d, t.name, name, unique, cols, false),
    };
    let mut wanted: Vec<(String, String)> = sql::secondary_indexes(t)
        .into_iter()
        .map(|(name, unique, cols)| {
            let stmt = index(&name, unique, &cols);
            (name, stmt)
        })
        .collect();
    for c in missing.iter().filter(|c| c.unique) {
        let name = format!("uq_{}_{}", t.name, c.name);
        let stmt = index(&name, true, &[c.name]);
        wanted.push((name, stmt));
    }
    if d == Dialect::Postgres {
        wanted.extend(t.columns.iter().filter(|c| c.fulltext).map(|c| sql::fulltext_index(t, c, false)));
    }
    if !wanted.is_empty() {
        let indexes = names(db, indexes_sql(d), t.name).await?;
        for (name, stmt) in wanted {
            if !has(&indexes, &name) {
                db.raw_execute(&stmt, &[]).await?;
                report.added_indexes.push(name);
            }
        }
    }
    Ok(report)
}

fn refuse(t: &TableSchema, c: &ColumnDef, why: &str) -> Error {
    Error::Unsupported(format!("push cannot add column `{}.{}`: {why}", t.name, c.name))
}

/// The names (first column) returned by an introspection query.
async fn names<E: Executor + ?Sized>(db: &E, sql: &str, table: &str) -> Result<Vec<String>> {
    let rows: Vec<Value> = db.raw(sql, &[Value::Text(table.to_owned())]).await?;
    Ok(rows
        .into_iter()
        .filter_map(|v| match v {
            Value::Text(s) => Some(s),
            // MySQL reports `information_schema` names as binary strings.
            Value::Bytes(b) => Some(String::from_utf8_lossy(&b).into_owned()),
            _ => None,
        })
        .collect())
}

fn columns_sql(d: Dialect) -> &'static str {
    match d {
        Dialect::Sqlite => "SELECT name FROM pragma_table_info(?)",
        Dialect::Postgres => {
            "SELECT column_name::text FROM information_schema.columns \
             WHERE table_schema = current_schema() AND table_name = $1"
        }
        Dialect::MySql => {
            "SELECT column_name FROM information_schema.columns \
             WHERE table_schema = DATABASE() AND table_name = ?"
        }
    }
}

fn indexes_sql(d: Dialect) -> &'static str {
    match d {
        Dialect::Sqlite => "SELECT name FROM pragma_index_list(?)",
        Dialect::Postgres => {
            "SELECT indexname::text FROM pg_indexes WHERE schemaname = current_schema() AND tablename = $1"
        }
        Dialect::MySql => {
            "SELECT DISTINCT index_name FROM information_schema.statistics \
             WHERE table_schema = DATABASE() AND table_name = ?"
        }
    }
}
