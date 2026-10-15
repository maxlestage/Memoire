//! Error type shared by every backend.

use std::fmt;

/// Convenient alias for `Result<T, memoire_orm::Error>`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Every error Memoire can produce.
///
/// Backend specific errors are normalised whenever it makes sense (for example a
/// unique constraint violation is reported as [`Error::UniqueViolation`] by the
/// in-memory engine, SQLite and PostgreSQL alike), so application code can match
/// on them without knowing which database it talks to.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The requested row does not exist.
    #[error("record not found")]
    NotFound,

    /// A `UNIQUE` or `PRIMARY KEY` constraint was violated.
    #[error("unique constraint violated on `{table}.{column}`")]
    UniqueViolation {
        /// Table name.
        table: String,
        /// Column name (or constraint name when the column is unknown).
        column: String,
    },

    /// A `NULL` was read into a non-`Option` field.
    #[error("unexpected NULL in column `{column}` (use an `Option<T>` field for nullable columns)")]
    UnexpectedNull {
        /// `table.column` (or `struct.field`) that was being decoded.
        column: String,
    },

    /// Another constraint (`NOT NULL`, `FOREIGN KEY`, `CHECK`) was violated.
    #[error("constraint violation: {0}")]
    Constraint(String),

    /// A value could not be decoded into the requested Rust type.
    #[error("cannot decode column `{column}`: {message}")]
    Decode {
        /// `table.column` (or `struct.field`) that was being decoded.
        column: String,
        /// Human readable reason.
        message: String,
    },

    /// A value could not be converted between two types.
    #[error("conversion error: {0}")]
    Conversion(String),

    /// The table does not exist.
    #[error("table `{0}` does not exist")]
    NoSuchTable(String),

    /// The table already exists.
    #[error("table `{0}` already exists")]
    TableExists(String),

    /// Two models are not related the way a relation method expects (no
    /// foreign key between them, or several).
    #[error("relation error: {0}")]
    Relation(String),

    /// The browser storage refused an operation (full, disabled, or not
    /// available outside a browser).
    #[error("browser storage error: {0}")]
    Storage(String),

    /// The operation is not supported by the backend.
    #[error("operation not supported by this backend: {0}")]
    Unsupported(String),

    /// An optimistic transaction could not be committed because another writer
    /// modified the same table first. Retrying the transaction is safe.
    #[error("transaction conflict on table `{0}`: it was modified concurrently, retry the transaction")]
    Conflict(String),

    /// The row was rejected before being written, by a lifecycle hook
    /// ([`Hooks`](crate::lifecycle::Hooks)) or your own validation.
    #[error("validation failed: {0}")]
    Validation(String),

    /// Optimistic locking: the row was changed (or deleted and re-created) by
    /// another writer since it was read, so the write was refused. Reload the
    /// row and apply the change again. See `#[memoire(version)]`.
    #[error("stale row `{table}` {key}: another writer changed it since it was read, reload it and retry")]
    StaleRow {
        /// Table name.
        table: String,
        /// Primary key of the row.
        key: String,
    },

    /// The transaction was already committed or rolled back.
    #[error("transaction already finished")]
    TransactionFinished,

    /// A migration failed.
    #[error("migration {version} (`{name}`) failed: {source}")]
    Migration {
        /// Migration version.
        version: i64,
        /// Migration name.
        name: String,
        /// Underlying error.
        #[source]
        source: Box<Error>,
    },

    /// The connection pool could not hand out a connection.
    #[error("connection pool error: {0}")]
    Pool(String),

    /// Any other error reported by the database driver.
    #[error("database error: {0}")]
    Database(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
}

impl Error {
    /// Builds a [`Error::Conversion`] from anything printable.
    pub fn conversion(msg: impl fmt::Display) -> Self {
        Error::Conversion(msg.to_string())
    }

    /// Builds a [`Error::Database`] from a driver error.
    pub fn database<E>(err: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Error::Database(Box::new(err))
    }

    /// `true` for [`Error::NotFound`].
    pub fn is_not_found(&self) -> bool {
        matches!(self, Error::NotFound)
    }

    /// `true` for [`Error::UniqueViolation`].
    pub fn is_unique_violation(&self) -> bool {
        matches!(self, Error::UniqueViolation { .. })
    }

    /// `true` for [`Error::Conflict`] (the transaction may simply be retried).
    pub fn is_conflict(&self) -> bool {
        matches!(self, Error::Conflict(_))
    }

    /// `true` for [`Error::Validation`].
    pub fn is_validation(&self) -> bool {
        matches!(self, Error::Validation(_))
    }

    /// `true` for [`Error::StaleRow`] (optimistic locking: reload and retry).
    pub fn is_stale(&self) -> bool {
        matches!(self, Error::StaleRow { .. })
    }

    /// Attaches a column label to conversion errors raised while decoding.
    pub(crate) fn in_column(self, column: &str) -> Self {
        match self {
            Error::UnexpectedNull { .. } => Error::UnexpectedNull { column: column.to_owned() },
            Error::Conversion(message) => Error::Decode { column: column.to_owned(), message },
            other => other,
        }
    }
}
