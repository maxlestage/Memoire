//! Procedural macros for [Memoire ORM](https://docs.rs/memoire-orm).
//!
//! This crate is an implementation detail: depend on `memoire-orm`, which re-exports
//! [`macro@Model`] and [`macro@FromRow`].
//!
//! All the SQL needed for primary-key CRUD (`find`, `insert`, `update`, `delete`,
//! `upsert`) is generated **at compile time** for every supported dialect, so the
//! hot path of an application never has to build a SQL string at runtime.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Ident, LitBool, LitStr, Type, parse_macro_input, spanned::Spanned};

/// Derives `memoire_orm::Model` (and `FromRow`) for a struct with named fields.
///
/// ```ignore
/// #[derive(Model)]
/// #[memoire(table = "users")]
/// struct User {
///     #[memoire(primary_key, auto_increment)]
///     id: i64,
///     #[memoire(unique)]
///     email: String,
///     name: String,
/// }
/// ```
///
/// Struct attributes: `table = "name"` (defaults to the snake_case struct name),
/// `crate = "path"` (where the generated code finds `memoire_orm`, `::memoire_orm` by
/// default; for crates that re-export it, e.g. `crate = "my_framework::orm"`).
///
/// Field attributes: `primary_key`, `auto_increment`, `unique`, `index`,
/// `column = "name"`, `sql_type = "VARCHAR(255)"`, `default = "SQL expression"`,
/// `references = "table(column)"`, `on_delete = "cascade"`, `on_update = "cascade"`,
/// `skip`.
#[proc_macro_derive(Model, attributes(memoire))]
pub fn derive_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_model(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derives `memoire_orm::FromRow` for a struct with named fields.
///
/// Columns are decoded **by position**, in field declaration order, which makes
/// decoding a single pass with no name lookups. Use it for custom projections and
/// raw SQL queries.
#[proc_macro_derive(FromRow, attributes(memoire))]
pub fn derive_from_row(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_from_row(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

struct FieldInfo {
    ident: Ident,
    ty: Type,
    column: String,
    primary_key: bool,
    auto_increment: Option<bool>,
    unique: bool,
    index: bool,
    sql_type: Option<String>,
    default: Option<String>,
    references: Option<(String, String)>,
    on_delete: Option<String>,
    on_update: Option<String>,
    skip: bool,
}

struct ModelInfo {
    table: String,
    /// Path of the `memoire_orm` crate in the generated code (`crate = "..."`).
    krate: syn::Path,
    fields: Vec<FieldInfo>,
}

fn unraw(ident: &Ident) -> String {
    let s = ident.to_string();
    s.strip_prefix("r#").map(str::to_owned).unwrap_or(s)
}

fn snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    let chars: Vec<char> = name.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            let prev_lower = i > 0 && (chars[i - 1].is_lowercase() || chars[i - 1].is_ascii_digit());
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if i > 0 && (prev_lower || (next_lower && chars[i - 1].is_uppercase())) {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn parse_referential_action(lit: &LitStr) -> syn::Result<String> {
    let v = lit.value().to_ascii_uppercase().replace(['_', '-'], " ");
    match v.as_str() {
        "CASCADE" | "RESTRICT" | "NO ACTION" | "SET NULL" | "SET DEFAULT" => Ok(v),
        _ => Err(syn::Error::new(lit.span(), "expected one of: cascade, restrict, no_action, set_null, set_default")),
    }
}

fn parse_model(input: &DeriveInput) -> syn::Result<ModelInfo> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(input.generics.span(), "memoire: generic structs are not supported"));
    }
    let data = match &input.data {
        Data::Struct(s) => s,
        _ => {
            return Err(syn::Error::new(input.ident.span(), "memoire: only structs with named fields are supported"));
        }
    };
    let named = match &data.fields {
        Fields::Named(n) => n,
        _ => {
            return Err(syn::Error::new(input.ident.span(), "memoire: only structs with named fields are supported"));
        }
    };

    let mut table = snake_case(&input.ident.to_string());
    let mut krate: syn::Path = syn::parse_quote!(::memoire_orm);
    for attr in &input.attrs {
        if !attr.path().is_ident("memoire") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("table") {
                let lit: LitStr = meta.value()?.parse()?;
                if lit.value().is_empty() {
                    return Err(meta.error("table name cannot be empty"));
                }
                table = lit.value();
                Ok(())
            } else if meta.path.is_ident("crate") {
                let lit: LitStr = meta.value()?.parse()?;
                krate = lit.parse()?;
                Ok(())
            } else {
                Err(meta.error("unknown memoire struct attribute (expected `table = \"...\"` or `crate = \"...\"`)"))
            }
        })?;
    }

    let mut fields = Vec::with_capacity(named.named.len());
    for field in &named.named {
        let ident = field.ident.clone().expect("named field");
        let mut info = FieldInfo {
            column: unraw(&ident),
            ident,
            ty: field.ty.clone(),
            primary_key: false,
            auto_increment: None,
            unique: false,
            index: false,
            sql_type: None,
            default: None,
            references: None,
            on_delete: None,
            on_update: None,
            skip: false,
        };
        for attr in &field.attrs {
            if !attr.path().is_ident("memoire") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                let p = &meta.path;
                if p.is_ident("primary_key") || p.is_ident("id") {
                    info.primary_key = true;
                } else if p.is_ident("auto_increment") || p.is_ident("auto") {
                    info.auto_increment = Some(if meta.input.peek(syn::Token![=]) {
                        let b: LitBool = meta.value()?.parse()?;
                        b.value
                    } else {
                        true
                    });
                } else if p.is_ident("unique") {
                    info.unique = true;
                } else if p.is_ident("index") {
                    info.index = true;
                } else if p.is_ident("skip") {
                    info.skip = true;
                } else if p.is_ident("column") || p.is_ident("rename") {
                    let lit: LitStr = meta.value()?.parse()?;
                    if lit.value().is_empty() {
                        return Err(meta.error("column name cannot be empty"));
                    }
                    info.column = lit.value();
                } else if p.is_ident("sql_type") {
                    let lit: LitStr = meta.value()?.parse()?;
                    info.sql_type = Some(lit.value());
                } else if p.is_ident("default") {
                    let lit: LitStr = meta.value()?.parse()?;
                    info.default = Some(lit.value());
                } else if p.is_ident("references") {
                    let lit: LitStr = meta.value()?.parse()?;
                    let v = lit.value();
                    let parsed = if let Some((t, rest)) = v.split_once('(') {
                        rest.strip_suffix(')').map(|c| (t.trim().to_owned(), c.trim().to_owned()))
                    } else {
                        v.split_once('.').map(|(t, c)| (t.trim().to_owned(), c.trim().to_owned()))
                    };
                    match parsed {
                        Some((t, c)) if !t.is_empty() && !c.is_empty() => info.references = Some((t, c)),
                        _ => {
                            return Err(syn::Error::new(
                                lit.span(),
                                "expected `references = \"table(column)\"` or `\"table.column\"`",
                            ));
                        }
                    }
                } else if p.is_ident("on_delete") {
                    let lit: LitStr = meta.value()?.parse()?;
                    info.on_delete = Some(parse_referential_action(&lit)?);
                } else if p.is_ident("on_update") {
                    let lit: LitStr = meta.value()?.parse()?;
                    info.on_update = Some(parse_referential_action(&lit)?);
                } else {
                    return Err(meta.error(
                        "unknown memoire field attribute (expected one of: primary_key, auto_increment, \
                         unique, index, column, sql_type, default, references, on_delete, on_update, skip)",
                    ));
                }
                Ok(())
            })?;
        }
        if info.skip && (info.primary_key || info.unique || info.index || info.references.is_some()) {
            return Err(syn::Error::new(info.ident.span(), "memoire: a `skip` field cannot carry column attributes"));
        }
        if (info.on_delete.is_some() || info.on_update.is_some()) && info.references.is_none() {
            return Err(syn::Error::new(
                info.ident.span(),
                "memoire: `on_delete`/`on_update` require `references = \"table(column)\"`",
            ));
        }
        fields.push(info);
    }

    let mut seen = std::collections::HashSet::new();
    for f in fields.iter().filter(|f| !f.skip) {
        if !seen.insert(f.column.clone()) {
            return Err(syn::Error::new(f.ident.span(), format!("memoire: duplicate column name `{}`", f.column)));
        }
    }

    Ok(ModelInfo { table, krate, fields })
}

fn is_integer_type(ty: &Type) -> bool {
    if let Type::Path(p) = ty
        && p.qself.is_none()
        && let Some(seg) = p.path.segments.last()
    {
        return matches!(seg.ident.to_string().as_str(), "i16" | "i32" | "i64") && seg.arguments.is_empty();
    }
    false
}

#[derive(Clone, Copy)]
enum Dialect {
    Postgres,
    Sqlite,
    MySql,
}

impl Dialect {
    fn quote(self, ident: &str) -> String {
        match self {
            Dialect::MySql => format!("`{}`", ident.replace('`', "``")),
            _ => format!("\"{}\"", ident.replace('"', "\"\"")),
        }
    }

    fn placeholder(self, n: usize) -> String {
        match self {
            Dialect::Postgres => format!("${n}"),
            _ => "?".to_owned(),
        }
    }
}

struct StaticSql {
    columns: String,
    select: String,
    find: String,
    insert: String,
    update: String,
    delete: String,
    upsert: String,
}

fn static_sql(d: Dialect, table: &str, cols: &[&FieldInfo], pk: usize, auto_pk: bool) -> StaticSql {
    let t = d.quote(table);
    let pk_col = d.quote(&cols[pk].column);
    let columns = cols.iter().map(|c| format!("{t}.{}", d.quote(&c.column))).collect::<Vec<_>>().join(", ");
    let select = format!("SELECT {columns} FROM {t}");
    let find = format!("{select} WHERE {t}.{pk_col} = {}", d.placeholder(1));

    // INSERT (the auto-increment primary key is generated by the database)
    let insert_cols: Vec<&&FieldInfo> =
        cols.iter().enumerate().filter(|(i, _)| !(auto_pk && *i == pk)).map(|(_, c)| c).collect();
    let mut insert = if insert_cols.is_empty() {
        match d {
            Dialect::MySql => format!("INSERT INTO {t} () VALUES ()"),
            _ => format!("INSERT INTO {t} DEFAULT VALUES"),
        }
    } else {
        let names = insert_cols.iter().map(|c| d.quote(&c.column)).collect::<Vec<_>>().join(", ");
        let phs = (1..=insert_cols.len()).map(|i| d.placeholder(i)).collect::<Vec<_>>().join(", ");
        format!("INSERT INTO {t} ({names}) VALUES ({phs})")
    };
    if auto_pk && !matches!(d, Dialect::MySql) {
        insert.push_str(&format!(" RETURNING {pk_col}"));
    }

    // UPDATE by primary key
    let non_pk: Vec<&&FieldInfo> = cols.iter().enumerate().filter(|(i, _)| *i != pk).map(|(_, c)| c).collect();
    let update = if non_pk.is_empty() {
        String::new()
    } else {
        let sets = non_pk
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{} = {}", d.quote(&c.column), d.placeholder(i + 1)))
            .collect::<Vec<_>>()
            .join(", ");
        format!("UPDATE {t} SET {sets} WHERE {pk_col} = {}", d.placeholder(non_pk.len() + 1))
    };

    let delete = format!("DELETE FROM {t} WHERE {pk_col} = {}", d.placeholder(1));

    // UPSERT (insert or update by primary key)
    let all_names = cols.iter().map(|c| d.quote(&c.column)).collect::<Vec<_>>().join(", ");
    let all_phs = (1..=cols.len()).map(|i| d.placeholder(i)).collect::<Vec<_>>().join(", ");
    let upsert = match d {
        // `ON DUPLICATE KEY UPDATE` fires on *any* unique key, so with other
        // unique columns it could overwrite a different row. Those tables get
        // a plain INSERT; the backend updates by key when it hits the primary key.
        Dialect::MySql if non_pk.iter().any(|c| c.unique) => {
            format!("INSERT INTO {t} ({all_names}) VALUES ({all_phs})")
        }
        Dialect::MySql => {
            if non_pk.is_empty() {
                format!("INSERT INTO {t} ({all_names}) VALUES ({all_phs}) ON DUPLICATE KEY UPDATE {pk_col} = {pk_col}")
            } else {
                let sets = non_pk
                    .iter()
                    .map(|c| {
                        let q = d.quote(&c.column);
                        format!("{q} = VALUES({q})")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("INSERT INTO {t} ({all_names}) VALUES ({all_phs}) ON DUPLICATE KEY UPDATE {sets}")
            }
        }
        _ => {
            if non_pk.is_empty() {
                format!("INSERT INTO {t} ({all_names}) VALUES ({all_phs}) ON CONFLICT ({pk_col}) DO NOTHING")
            } else {
                let sets = non_pk
                    .iter()
                    .map(|c| {
                        let q = d.quote(&c.column);
                        format!("{q} = EXCLUDED.{q}")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("INSERT INTO {t} ({all_names}) VALUES ({all_phs}) ON CONFLICT ({pk_col}) DO UPDATE SET {sets}")
            }
        }
    };

    StaticSql { columns, select, find, insert, update, delete, upsert }
}

fn upper_snake(name: &str) -> String {
    name.to_ascii_uppercase()
}

fn opt_str(v: &Option<String>) -> TokenStream2 {
    match v {
        Some(s) => quote!(::core::option::Option::Some(#s)),
        None => quote!(::core::option::Option::None),
    }
}

fn expand_model(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let info = parse_model(input)?;
    let krate = &info.krate;
    let name = &input.ident;
    let table = &info.table;
    let cols: Vec<&FieldInfo> = info.fields.iter().filter(|f| !f.skip).collect();
    if cols.is_empty() {
        return Err(syn::Error::new(name.span(), "memoire: a model needs at least one column"));
    }

    // Primary key resolution.
    let explicit: Vec<usize> = cols.iter().enumerate().filter(|(_, f)| f.primary_key).map(|(i, _)| i).collect();
    let (pk, inferred) = match explicit.len() {
        1 => (explicit[0], false),
        0 => match cols.iter().position(|f| f.column == "id" || unraw(&f.ident) == "id") {
            Some(i) => (i, true),
            None => {
                return Err(syn::Error::new(
                    name.span(),
                    "memoire: a model needs a primary key: mark a field with `#[memoire(primary_key)]` \
                     or name it `id`",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new(
                cols[explicit[1]].ident.span(),
                "memoire: composite primary keys are not supported (only one `primary_key` field allowed)",
            ));
        }
    };
    for (i, f) in cols.iter().enumerate() {
        if i != pk && f.auto_increment == Some(true) {
            return Err(syn::Error::new(
                f.ident.span(),
                "memoire: `auto_increment` is only allowed on the primary key",
            ));
        }
    }
    let pk_field = cols[pk];
    let auto_pk = match pk_field.auto_increment {
        Some(v) => v,
        None => inferred && is_integer_type(&pk_field.ty),
    };
    if auto_pk && !is_integer_type(&pk_field.ty) {
        return Err(syn::Error::new(
            pk_field.ty.span(),
            "memoire: `auto_increment` requires an integer primary key (i16, i32 or i64)",
        ));
    }

    let pg = static_sql(Dialect::Postgres, table, &cols, pk, auto_pk);
    let lite = static_sql(Dialect::Sqlite, table, &cols, pk, auto_pk);
    let my = static_sql(Dialect::MySql, table, &cols, pk, auto_pk);
    let sql_tokens = |s: &StaticSql| {
        let StaticSql { columns, select, find, insert, update, delete, upsert } = s;
        quote! {
            #krate::StaticSql {
                columns: #columns,
                select: #select,
                find: #find,
                insert: #insert,
                update: #update,
                delete: #delete,
                upsert: #upsert,
            }
        }
    };
    let (pg, lite, my) = (sql_tokens(&pg), sql_tokens(&lite), sql_tokens(&my));

    let column_defs = cols.iter().enumerate().map(|(i, f)| {
        let col = &f.column;
        let field = unraw(&f.ident);
        let ty = &f.ty;
        let is_pk = i == pk;
        let auto = is_pk && auto_pk;
        let unique = f.unique;
        let indexed = f.index;
        let sql_type = opt_str(&f.sql_type);
        let default = opt_str(&f.default);
        let references = match &f.references {
            Some((t, c)) => {
                let on_delete = opt_str(&f.on_delete);
                let on_update = opt_str(&f.on_update);
                quote!(::core::option::Option::Some(#krate::ForeignKey {
                    table: #t,
                    column: #c,
                    on_delete: #on_delete,
                    on_update: #on_update,
                }))
            }
            None => quote!(::core::option::Option::None),
        };
        quote! {
            #krate::ColumnDef {
                name: #col,
                field: #field,
                kind: <#ty as #krate::SqlType>::KIND,
                nullable: <#ty as #krate::SqlType>::NULLABLE,
                primary_key: #is_pk,
                auto_increment: #auto,
                unique: #unique,
                indexed: #indexed,
                sql_type: #sql_type,
                default: #default,
                references: #references,
            }
        }
    });

    let pk_ident = &pk_field.ident;
    let pk_ty = &pk_field.ty;
    let pk_name = &pk_field.column;
    let width = cols.len();

    let to_values = cols.iter().map(|f| {
        let id = &f.ident;
        quote!(#krate::ToValue::to_value(&self.#id))
    });

    let value_at = cols.iter().enumerate().map(|(i, f)| {
        let id = &f.ident;
        quote!(#i => #krate::ToValue::to_value(&self.#id))
    });

    let decode_fields = decode_fields_tokens(&info.fields, table, krate);

    let column_consts = cols.iter().enumerate().map(|(i, f)| {
        let const_name = format_ident!("{}", upper_snake(&unraw(&f.ident)), span = f.ident.span());
        let ty = &f.ty;
        let col = &f.column;
        let doc = format!("Typed reference to the `{table}.{col}` column.");
        quote! {
            #[doc = #doc]
            #[allow(dead_code)]
            pub const #const_name: #krate::Column<#name, #ty> =
                #krate::Column::new(#table, #col, #i);
        }
    });

    Ok(quote! {
        impl #krate::Model for #name {
            const SCHEMA: &'static #krate::TableSchema = &#krate::TableSchema {
                name: #table,
                columns: &[#(#column_defs),*],
                primary_key: #pk,
                sql: [#pg, #lite, #my],
            };

            type Key = #pk_ty;

            #[inline]
            fn key(&self) -> #krate::Value {
                #krate::ToValue::to_value(&self.#pk_ident)
            }

            #[inline]
            fn set_key(&mut self, value: #krate::Value) -> #krate::Result<()> {
                self.#pk_ident = #krate::__private::convert(value, #pk_name)?;
                ::core::result::Result::Ok(())
            }

            #[inline]
            fn to_values(&self) -> ::std::vec::Vec<#krate::Value> {
                ::std::vec![#(#to_values),*]
            }

            #[inline]
            fn value_at(&self, index: usize) -> #krate::Value {
                match index {
                    #(#value_at,)*
                    _ => #krate::Value::Null,
                }
            }
        }

        impl #krate::FromRow for #name {
            const WIDTH: usize = #width;

            #[inline]
            fn from_row<R: #krate::RowSource + ?Sized>(
                row: &mut R,
                offset: usize,
            ) -> #krate::Result<Self> {
                ::core::result::Result::Ok(Self { #(#decode_fields),* })
            }
        }

        impl #name {
            #(#column_consts)*
        }
    })
}

fn decode_fields_tokens(fields: &[FieldInfo], context: &str, krate: &syn::Path) -> Vec<TokenStream2> {
    let mut idx = 0usize;
    fields
        .iter()
        .map(|f| {
            let id = &f.ident;
            if f.skip {
                quote!(#id: ::core::default::Default::default())
            } else {
                let i = idx;
                idx += 1;
                let label = format!("{context}.{}", f.column);
                quote!(#id: #krate::__private::column(row, offset + #i, #label)?)
            }
        })
        .collect()
}

fn expand_from_row(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let info = parse_model(input)?;
    let krate = &info.krate;
    let name = &input.ident;
    let width = info.fields.iter().filter(|f| !f.skip).count();
    let decode_fields = decode_fields_tokens(&info.fields, &name.to_string(), krate);
    Ok(quote! {
        impl #krate::FromRow for #name {
            const WIDTH: usize = #width;

            #[inline]
            fn from_row<R: #krate::RowSource + ?Sized>(
                row: &mut R,
                offset: usize,
            ) -> #krate::Result<Self> {
                ::core::result::Result::Ok(Self { #(#decode_fields),* })
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case_works() {
        assert_eq!(snake_case("User"), "user");
        assert_eq!(snake_case("BlogPost"), "blog_post");
        assert_eq!(snake_case("HTTPRequest"), "http_request");
        assert_eq!(snake_case("Order2Item"), "order2_item");
    }
}
