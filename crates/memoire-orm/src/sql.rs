//! Rendering of [`Query`] values to SQL text for each [`Dialect`].
//!
//! Parameters are never copied: the renderer returns references to the values
//! stored in the statement, and primary-key CRUD reuses the SQL strings generated
//! at compile time by `#[derive(Model)]`.

use std::borrow::Cow;

use crate::ast::{DeleteAst, InsertAst, JoinKind, Projection, Query, SelectAst, UpdateAst};
use crate::error::{Error, Result};
use crate::expr::{AggFunc, BinOp, Expr, Nulls, Order, TextSearch};
use crate::schema::{ColumnDef, Dialect, TableSchema};
use crate::value::{ColumnType, Value};

/// A rendered statement.
#[derive(Clone, Debug, PartialEq)]
pub struct Rendered<'a> {
    /// SQL text.
    pub sql: Cow<'a, str>,
    /// Bound parameters, in placeholder order.
    pub params: Vec<&'a Value>,
}

impl Rendered<'_> {
    /// Clones the parameters into owned values.
    pub fn owned_params(&self) -> Vec<Value> {
        self.params.iter().map(|v| (*v).clone()).collect()
    }
}

/// Quotes an identifier for a dialect.
pub fn quote_ident(dialect: Dialect, ident: &str, out: &mut String) {
    let q = if dialect == Dialect::MySql { '`' } else { '"' };
    out.push(q);
    if ident.contains(q) {
        for c in ident.chars() {
            if c == q {
                out.push(q);
            }
            out.push(c);
        }
    } else {
        out.push_str(ident);
    }
    out.push(q);
}

/// Appends `n` in decimal (placeholders: `$12`), without the formatting machinery.
#[inline]
fn push_number(out: &mut String, mut n: u64) {
    let mut digits = [0u8; 20];
    let mut at = digits.len();
    loop {
        at -= 1;
        digits[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    out.push_str(std::str::from_utf8(&digits[at..]).unwrap_or_default());
}

struct Writer<'a> {
    d: Dialect,
    sql: String,
    params: Vec<&'a Value>,
}

impl<'a> Writer<'a> {
    fn new(d: Dialect, capacity: usize) -> Self {
        Writer { d, sql: String::with_capacity(capacity), params: Vec::with_capacity(8) }
    }

    #[inline]
    fn push(&mut self, s: &str) {
        self.sql.push_str(s);
    }

    #[inline]
    fn ident(&mut self, name: &str) {
        quote_ident(self.d, name, &mut self.sql);
    }

    #[inline]
    fn param(&mut self, v: &'a Value) {
        self.params.push(v);
        match self.d {
            Dialect::Postgres => {
                self.sql.push('$');
                push_number(&mut self.sql, self.params.len() as u64);
            }
            _ => self.sql.push('?'),
        }
    }

    fn column(&mut self, table: &str, name: &str) {
        self.ident(table);
        self.sql.push('.');
        self.ident(name);
    }

    fn expr(&mut self, e: &'a Expr) {
        match e {
            Expr::Column(c) => self.column(c.table, c.name),
            Expr::Value(v) => self.param(v),
            Expr::Binary { op, left, right } => {
                let wrap = matches!(op, BinOp::And | BinOp::Or | BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div);
                if wrap {
                    self.sql.push('(');
                }
                self.expr(left);
                self.sql.push(' ');
                self.push(op.as_sql());
                self.sql.push(' ');
                self.expr(right);
                if wrap {
                    self.sql.push(')');
                }
            }
            Expr::Not(inner) => {
                self.push("NOT (");
                self.expr(inner);
                self.sql.push(')');
            }
            Expr::IsNull { expr, negated } => {
                self.expr(expr);
                self.push(if *negated { " IS NOT NULL" } else { " IS NULL" });
            }
            Expr::InList { expr, list, negated } => {
                if list.is_empty() {
                    self.push(if *negated { "1 = 1" } else { "1 = 0" });
                    return;
                }
                self.expr(expr);
                self.push(if *negated { " NOT IN (" } else { " IN (" });
                for (i, v) in list.iter().enumerate() {
                    if i > 0 {
                        self.push(", ");
                    }
                    self.param(v);
                }
                self.sql.push(')');
            }
            Expr::Between { expr, low, high, negated } => {
                self.expr(expr);
                self.push(if *negated { " NOT BETWEEN " } else { " BETWEEN " });
                self.expr(low);
                self.push(" AND ");
                self.expr(high);
            }
            Expr::Like { expr, pattern, case_insensitive, negated, escaped } => {
                let native_ilike = *case_insensitive && self.d == Dialect::Postgres;
                let lower = *case_insensitive && !native_ilike;
                if lower {
                    self.push("LOWER(");
                }
                self.expr(expr);
                if lower {
                    self.sql.push(')');
                }
                self.push(match (*negated, native_ilike) {
                    (false, false) => " LIKE ",
                    (true, false) => " NOT LIKE ",
                    (false, true) => " ILIKE ",
                    (true, true) => " NOT ILIKE ",
                });
                if lower {
                    self.push("LOWER(");
                }
                self.expr(pattern);
                if lower {
                    self.sql.push(')');
                }
                if *escaped {
                    self.push(if self.d == Dialect::MySql { " ESCAPE '\\\\'" } else { " ESCAPE '\\'" });
                }
            }
            Expr::Match(m) => self.text_search(m),
            Expr::Aggregate { func, arg, distinct } => self.aggregate(*func, arg.as_deref(), *distinct),
            Expr::InSubquery { expr, query, negated } => {
                self.expr(expr);
                self.push(if *negated { " NOT IN (" } else { " IN (" });
                self.select(query);
                self.sql.push(')');
            }
        }
    }

    /// Full-text search, with the word rule of [`TextSearch`] on every database.
    fn text_search(&mut self, m: &'a TextSearch) {
        if m.words.is_empty() {
            self.push("1 = 0");
            return;
        }
        match self.d {
            // Words are runs of alphanumerics: every other character becomes a
            // space before the `simple` parser (no stemming, no stop words) splits
            // the text. A `#[memoire(fulltext)]` GIN index covers this expression.
            Dialect::Postgres => {
                self.push("to_tsvector('simple', regexp_replace(");
                self.expr(&m.expr);
                self.push(", '[^[:alnum:]]+', ' ', 'g')) @@ plainto_tsquery('simple', ");
                self.param(&m.query);
                self.sql.push(')');
            }
            // A function registered on every connection by `SqliteDb`.
            Dialect::Sqlite => {
                self.push("memoire_match(");
                self.expr(&m.expr);
                self.push(", ");
                self.param(&m.query);
                self.sql.push(')');
            }
            Dialect::MySql => {
                self.sql.push('(');
                for (i, p) in m.patterns.iter().enumerate() {
                    if i > 0 {
                        self.push(" AND ");
                    }
                    self.push("LOWER(");
                    self.expr(&m.expr);
                    self.push(") REGEXP ");
                    self.param(p);
                }
                self.sql.push(')');
            }
        }
    }

    fn aggregate(&mut self, func: AggFunc, arg: Option<&'a Expr>, distinct: bool) {
        let name = match func {
            AggFunc::Count => "COUNT(",
            AggFunc::Sum => "SUM(",
            AggFunc::Avg => "AVG(",
            AggFunc::Min => "MIN(",
            AggFunc::Max => "MAX(",
        };
        // Normalise result types so they decode into natural Rust types:
        // integer SUMs into i64 and AVG into f64 on every database.
        let cast = match (func, self.d) {
            (AggFunc::Avg, Dialect::Postgres) => Some("DOUBLE PRECISION"),
            (AggFunc::Avg, Dialect::MySql) => Some("DOUBLE"),
            (AggFunc::Sum, d) if d != Dialect::Sqlite => match arg.and_then(expr_kind) {
                Some(k) if k.is_integer() => Some(if d == Dialect::MySql { "SIGNED" } else { "BIGINT" }),
                Some(k) if k.is_float() => Some(if d == Dialect::MySql { "DOUBLE" } else { "DOUBLE PRECISION" }),
                _ => None,
            },
            _ => None,
        };
        if cast.is_some() {
            self.push("CAST(");
        }
        self.push(name);
        if distinct {
            self.push("DISTINCT ");
        }
        match arg {
            Some(a) => self.expr(a),
            None => self.sql.push('*'),
        }
        self.sql.push(')');
        if let Some(ty) = cast {
            self.push(" AS ");
            self.push(ty);
            self.sql.push(')');
        }
    }

    fn order(&mut self, o: &'a Order) {
        if self.d == Dialect::MySql
            && let Some(n) = o.nulls
        {
            // MySQL has no NULLS FIRST/LAST: sort on `expr IS NULL` first.
            self.expr(&o.expr);
            self.push(if n == Nulls::First { " IS NULL DESC, " } else { " IS NULL ASC, " });
        }
        self.expr(&o.expr);
        self.push(if o.desc { " DESC" } else { " ASC" });
        if self.d != Dialect::MySql {
            match o.nulls {
                Some(Nulls::First) => self.push(" NULLS FIRST"),
                Some(Nulls::Last) => self.push(" NULLS LAST"),
                None => {}
            }
        }
    }

    fn where_clause(&mut self, filter: Option<&'a Expr>) {
        if let Some(f) = filter {
            self.push(" WHERE ");
            self.expr(f);
        }
    }

    fn select(&mut self, s: &'a SelectAst) {
        self.push("SELECT ");
        if s.distinct {
            self.push("DISTINCT ");
        }
        match &s.projection {
            Projection::Tables(tables) => {
                for (i, t) in tables.iter().enumerate() {
                    if i > 0 {
                        self.push(", ");
                    }
                    self.push(t.sql(self.d).columns);
                }
            }
            Projection::Exprs(exprs) => {
                for (i, e) in exprs.iter().enumerate() {
                    if i > 0 {
                        self.push(", ");
                    }
                    self.expr(e);
                }
            }
        }
        self.push(" FROM ");
        self.ident(s.from.name);
        for j in &s.joins {
            self.push(match j.kind {
                JoinKind::Inner => " INNER JOIN ",
                JoinKind::Left => " LEFT JOIN ",
            });
            self.ident(j.table.name);
            self.push(" ON ");
            self.expr(&j.on);
        }
        self.where_clause(s.filter.as_ref());
        if !s.group_by.is_empty() {
            self.push(" GROUP BY ");
            for (i, e) in s.group_by.iter().enumerate() {
                if i > 0 {
                    self.push(", ");
                }
                self.expr(e);
            }
        }
        if let Some(h) = &s.having {
            self.push(" HAVING ");
            self.expr(h);
        }
        if !s.order.is_empty() {
            self.push(" ORDER BY ");
            for (i, o) in s.order.iter().enumerate() {
                if i > 0 {
                    self.push(", ");
                }
                self.order(o);
            }
        }
        self.limit_offset(s.limit, s.offset);
    }

    fn limit_offset(&mut self, limit: Option<u64>, offset: Option<u64>) {
        match (limit, offset) {
            (Some(l), _) => {
                self.push(" LIMIT ");
                push_number(&mut self.sql, l);
            }
            (None, Some(_)) => match self.d {
                Dialect::Sqlite => self.push(" LIMIT -1"),
                Dialect::MySql => self.push(" LIMIT 18446744073709551615"),
                Dialect::Postgres => {}
            },
            (None, None) => {}
        }
        if let Some(o) = offset {
            self.push(" OFFSET ");
            push_number(&mut self.sql, o);
        }
    }

    fn insert(&mut self, ins: &'a InsertAst) -> Result<()> {
        let t = ins.table;
        let skip = t.auto_increment().then_some(t.primary_key);
        let cols: Vec<(usize, &ColumnDef)> = t.columns.iter().enumerate().filter(|(i, _)| Some(*i) != skip).collect();
        if cols.is_empty() {
            return Err(Error::Unsupported(format!(
                "multi-row insert into `{}`, which only has an auto-increment key",
                t.name
            )));
        }
        self.push("INSERT INTO ");
        self.ident(t.name);
        self.push(" (");
        for (n, (_, c)) in cols.iter().enumerate() {
            if n > 0 {
                self.push(", ");
            }
            self.ident(c.name);
        }
        self.push(") VALUES ");
        for (r, row) in ins.rows.iter().enumerate() {
            if row.len() != t.columns.len() {
                return Err(Error::Conversion(format!(
                    "insert into `{}`: expected {} values, got {}",
                    t.name,
                    t.columns.len(),
                    row.len()
                )));
            }
            if r > 0 {
                self.push(", ");
            }
            self.sql.push('(');
            for (n, (i, _)) in cols.iter().enumerate() {
                if n > 0 {
                    self.push(", ");
                }
                self.param(&row[*i]);
            }
            self.sql.push(')');
        }
        Ok(())
    }

    fn update(&mut self, u: &'a UpdateAst) -> Result<()> {
        if u.sets.is_empty() {
            return Err(Error::Unsupported("UPDATE without any SET clause".into()));
        }
        self.push("UPDATE ");
        self.ident(u.table.name);
        self.push(" SET ");
        for (n, (i, e)) in u.sets.iter().enumerate() {
            if n > 0 {
                self.push(", ");
            }
            let col =
                u.table.columns.get(*i).ok_or_else(|| {
                    Error::Conversion(format!("column index {i} out of range for `{}`", u.table.name))
                })?;
            self.ident(col.name);
            self.push(" = ");
            self.expr(e);
        }
        self.where_clause(u.filter.as_ref());
        Ok(())
    }

    fn delete(&mut self, d: &'a DeleteAst) {
        self.push("DELETE FROM ");
        self.ident(d.table.name);
        self.where_clause(d.filter.as_ref());
    }
}

/// Best-effort type of an expression (used to pick aggregate casts).
fn expr_kind(e: &Expr) -> Option<ColumnType> {
    match e {
        Expr::Column(c) => Some(c.kind),
        Expr::Binary { op: BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div, left, .. } => expr_kind(left),
        _ => None,
    }
}

/// Renders a statement to SQL.
///
/// DDL statements ([`Query::CreateTable`]) may need several SQL statements; use
/// [`ddl`] for them.
pub fn render<'a>(query: &Query<'a>, d: Dialect) -> Result<Rendered<'a>> {
    match *query {
        Query::Select(s) => {
            let mut w = Writer::new(d, 128);
            w.select(s);
            Ok(Rendered { sql: Cow::Owned(w.sql), params: w.params })
        }
        Query::Insert(i) => {
            let mut w = Writer::new(d, 64 + i.rows.len() * 16);
            w.insert(i)?;
            Ok(Rendered { sql: Cow::Owned(w.sql), params: w.params })
        }
        Query::Update(u) => {
            let mut w = Writer::new(d, 96);
            w.update(u)?;
            Ok(Rendered { sql: Cow::Owned(w.sql), params: w.params })
        }
        Query::Delete(del) => {
            let mut w = Writer::new(d, 64);
            w.delete(del);
            Ok(Rendered { sql: Cow::Owned(w.sql), params: w.params })
        }
        Query::FindByKey { table, key } => Ok(Rendered { sql: Cow::Borrowed(table.sql(d).find), params: vec![key] }),
        Query::DeleteByKey { table, key } => {
            Ok(Rendered { sql: Cow::Borrowed(table.sql(d).delete), params: vec![key] })
        }
        Query::InsertRow { table, values } => {
            check_width(table, values)?;
            let skip = table.auto_increment().then_some(table.primary_key);
            let params = values.iter().enumerate().filter(|(i, _)| Some(*i) != skip).map(|(_, v)| v).collect();
            Ok(Rendered { sql: Cow::Borrowed(table.sql(d).insert), params })
        }
        Query::UpsertRow { table, values } => {
            check_width(table, values)?;
            Ok(Rendered { sql: Cow::Borrowed(table.sql(d).upsert), params: values.iter().collect() })
        }
        Query::UpdateRow { table, values } => {
            check_width(table, values)?;
            let sql = table.sql(d).update;
            if sql.is_empty() {
                return Err(Error::Unsupported(format!("`{}` has no column besides its primary key", table.name)));
            }
            let pk = table.primary_key;
            let mut params: Vec<&Value> = Vec::with_capacity(values.len());
            params.extend(values.iter().enumerate().filter(|(i, _)| *i != pk).map(|(_, v)| v));
            params.push(&values[pk]);
            Ok(Rendered { sql: Cow::Borrowed(sql), params })
        }
        Query::Raw { sql, params } => Ok(Rendered { sql: Cow::Borrowed(sql), params: params.iter().collect() }),
        Query::Script { sql } => Ok(Rendered { sql: Cow::Borrowed(sql), params: Vec::new() }),
        Query::CreateTable { .. } | Query::DropTable { .. } => {
            let stmts = ddl(query, d)?;
            Ok(Rendered { sql: Cow::Owned(stmts.join(";\n")), params: Vec::new() })
        }
    }
}

fn check_width(table: &TableSchema, values: &[Value]) -> Result<()> {
    if values.len() == table.columns.len() {
        Ok(())
    } else {
        Err(Error::Conversion(format!("`{}` expects {} values, got {}", table.name, table.columns.len(), values.len())))
    }
}

/// `in_index`: the column belongs to a multi-column index (MySQL cannot index
/// `TEXT`/`BLOB` columns without a prefix length, so they become `VARCHAR`).
fn column_sql_type(c: &ColumnDef, d: Dialect, in_index: bool) -> &'static str {
    if let Some(t) = c.sql_type {
        return t;
    }
    let keyish = in_index || c.primary_key || c.unique || c.indexed || c.references.is_some();
    match (c.kind, d) {
        (ColumnType::Bool, Dialect::Sqlite) => "INTEGER",
        (ColumnType::Bool, _) => "BOOLEAN",
        (ColumnType::SmallInt, Dialect::Sqlite) => "INTEGER",
        (ColumnType::SmallInt, _) => "SMALLINT",
        (ColumnType::Int, Dialect::MySql) => "INT",
        (ColumnType::Int, _) => "INTEGER",
        (ColumnType::BigInt, Dialect::Sqlite) => "INTEGER",
        (ColumnType::BigInt, _) => "BIGINT",
        (ColumnType::Real, Dialect::MySql) => "FLOAT",
        (ColumnType::Real, _) => "REAL",
        (ColumnType::Double, Dialect::Postgres) => "DOUBLE PRECISION",
        (ColumnType::Double, Dialect::Sqlite) => "REAL",
        (ColumnType::Double, Dialect::MySql) => "DOUBLE",
        (ColumnType::Text, Dialect::MySql) if keyish => "VARCHAR(255)",
        (ColumnType::Text, _) => "TEXT",
        (ColumnType::Bytes, Dialect::Postgres) => "BYTEA",
        (ColumnType::Bytes, Dialect::MySql) if keyish => "VARBINARY(255)",
        (ColumnType::Bytes, _) => "BLOB",
        (ColumnType::Uuid, Dialect::Postgres) => "UUID",
        (ColumnType::Uuid, Dialect::Sqlite) => "TEXT",
        (ColumnType::Uuid, Dialect::MySql) => "CHAR(36)",
        (ColumnType::Timestamp, Dialect::Postgres) => "TIMESTAMP",
        (ColumnType::TimestampTz, Dialect::Postgres) => "TIMESTAMPTZ",
        (ColumnType::Timestamp | ColumnType::TimestampTz, Dialect::Sqlite) => "TEXT",
        (ColumnType::Timestamp | ColumnType::TimestampTz, Dialect::MySql) => "DATETIME(6)",
        (ColumnType::Date, Dialect::Sqlite) => "TEXT",
        (ColumnType::Date, _) => "DATE",
        (ColumnType::Time, Dialect::Sqlite) => "TEXT",
        (ColumnType::Time, Dialect::MySql) => "TIME(6)",
        (ColumnType::Time, _) => "TIME",
        (ColumnType::Json, Dialect::Postgres) => "JSONB",
        (ColumnType::Json, Dialect::Sqlite) => "TEXT",
        (ColumnType::Json, Dialect::MySql) => "JSON",
    }
}

fn column_ddl(c: &ColumnDef, d: Dialect, in_index: bool, out: &mut String) {
    quote_ident(d, c.name, out);
    out.push(' ');
    if c.auto_increment {
        match d {
            Dialect::Sqlite => {
                // Must be spelled exactly `INTEGER PRIMARY KEY` to alias the rowid.
                out.push_str("INTEGER PRIMARY KEY");
                return;
            }
            Dialect::Postgres => {
                out.push_str(column_sql_type(c, d, in_index));
                out.push_str(" GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY");
                return;
            }
            Dialect::MySql => {
                out.push_str(column_sql_type(c, d, in_index));
                out.push_str(" NOT NULL AUTO_INCREMENT PRIMARY KEY");
                return;
            }
        }
    }
    out.push_str(column_sql_type(c, d, in_index));
    if c.primary_key {
        out.push_str(" PRIMARY KEY");
    } else if !c.nullable {
        out.push_str(" NOT NULL");
    }
    if c.unique && !c.primary_key {
        out.push_str(" UNIQUE");
    }
    if let Some(def) = c.default {
        out.push_str(" DEFAULT ");
        out.push_str(def);
    }
}

/// Renders DDL statements (`CREATE TABLE` + `CREATE INDEX`, or `DROP TABLE`).
pub fn ddl(query: &Query<'_>, d: Dialect) -> Result<Vec<String>> {
    match *query {
        Query::CreateTable { table, if_not_exists } => Ok(create_table(table, d, if_not_exists)),
        Query::DropTable { name, if_exists } => {
            let mut s = String::from(if if_exists { "DROP TABLE IF EXISTS " } else { "DROP TABLE " });
            quote_ident(d, name, &mut s);
            Ok(vec![s])
        }
        _ => Err(Error::Unsupported("not a DDL statement".into())),
    }
}

/// `CREATE TABLE` statement(s) for a table.
pub fn create_table(t: &TableSchema, d: Dialect, if_not_exists: bool) -> Vec<String> {
    let mut s = String::with_capacity(256);
    s.push_str(if if_not_exists { "CREATE TABLE IF NOT EXISTS " } else { "CREATE TABLE " });
    quote_ident(d, t.name, &mut s);
    s.push_str(" (\n");
    for (i, c) in t.columns.iter().enumerate() {
        if i > 0 {
            s.push_str(",\n");
        }
        s.push_str("    ");
        let in_index = t.indexes.iter().any(|ix| ix.columns.contains(&i));
        column_ddl(c, d, in_index, &mut s);
    }
    for c in t.columns.iter() {
        if let Some(fk) = c.references {
            s.push_str(",\n    FOREIGN KEY (");
            quote_ident(d, c.name, &mut s);
            s.push_str(") REFERENCES ");
            quote_ident(d, fk.table, &mut s);
            s.push_str(" (");
            quote_ident(d, fk.column, &mut s);
            s.push(')');
            if let Some(a) = fk.on_delete {
                s.push_str(" ON DELETE ");
                s.push_str(a);
            }
            if let Some(a) = fk.on_update {
                s.push_str(" ON UPDATE ");
                s.push_str(a);
            }
        }
    }
    // (name, unique, columns) of every secondary index.
    let mut indexes: Vec<(String, bool, Vec<&str>)> = t
        .columns
        .iter()
        .filter(|c| c.indexed && !c.primary_key && !c.unique)
        .map(|c| (format!("idx_{}_{}", t.name, c.name), false, vec![c.name]))
        .collect();
    indexes.extend(
        t.indexes
            .iter()
            .map(|ix| (ix.name.to_owned(), ix.unique, ix.columns.iter().map(|&i| t.columns[i].name).collect())),
    );
    let mut extra = Vec::new();
    for (idx_name, unique, columns) in &indexes {
        let mut cols = String::new();
        for (n, c) in columns.iter().enumerate() {
            if n > 0 {
                cols.push_str(", ");
            }
            quote_ident(d, c, &mut cols);
        }
        if d == Dialect::MySql {
            s.push_str(if *unique { ",\n    UNIQUE INDEX " } else { ",\n    INDEX " });
            quote_ident(d, idx_name, &mut s);
            s.push_str(" (");
            s.push_str(&cols);
            s.push(')');
        } else {
            let mut stmt = String::from(match (*unique, if_not_exists) {
                (false, false) => "CREATE INDEX ",
                (false, true) => "CREATE INDEX IF NOT EXISTS ",
                (true, false) => "CREATE UNIQUE INDEX ",
                (true, true) => "CREATE UNIQUE INDEX IF NOT EXISTS ",
            });
            quote_ident(d, idx_name, &mut stmt);
            stmt.push_str(" ON ");
            quote_ident(d, t.name, &mut stmt);
            stmt.push_str(" (");
            stmt.push_str(&cols);
            stmt.push(')');
            extra.push(stmt);
        }
    }
    // Full-text: a GIN index over the same expression `Column::matches` searches.
    if d == Dialect::Postgres {
        for c in t.columns.iter().filter(|c| c.fulltext) {
            let mut stmt = String::from(if if_not_exists { "CREATE INDEX IF NOT EXISTS " } else { "CREATE INDEX " });
            quote_ident(d, &format!("fts_{}_{}", t.name, c.name), &mut stmt);
            stmt.push_str(" ON ");
            quote_ident(d, t.name, &mut stmt);
            stmt.push_str(" USING GIN (to_tsvector('simple', regexp_replace(");
            quote_ident(d, c.name, &mut stmt);
            stmt.push_str(", '[^[:alnum:]]+', ' ', 'g')))");
            extra.push(stmt);
        }
    }
    s.push_str("\n)");
    if d == Dialect::MySql {
        // Case-sensitive, byte-ordered comparisons like every other backend.
        s.push_str(" DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin");
    }
    let mut out = Vec::with_capacity(1 + extra.len());
    out.push(s);
    out.extend(extra);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn push_number_writes_decimal() {
        for n in [0u64, 7, 10, 99, 12345, u64::MAX] {
            let mut s = String::new();
            super::push_number(&mut s, n);
            assert_eq!(s, n.to_string());
        }
    }
}
