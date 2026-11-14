//! The HTTP(S) client of Nuage DB (feature `nuage`): [`NuageDb`] and its transactions.
//!
//! ```no_run
//! # async fn demo() -> memoire_orm::Result<()> {
//! use memoire_orm::prelude::*;
//!
//! #[derive(Model, Debug)]
//! struct Note { #[memoire(primary_key, auto_increment)] id: i64, text: String }
//!
//! let db = NuageDb::connect("https://nuage.example.com", "s3cret-token").await?;
//! db.create_table_if_not_exists::<Note>().await?;
//! Note { id: 0, text: "hello".into() }.insert(&db).await?;
//! # Ok(()) }
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value as Json, json};

use super::{BRANCH_HEADER, Branch, TRANSACTION_HEADER, wire};
use crate::ast::Query;
use crate::error::{Error, Result};
use crate::executor::{Database, ExecResult, Executor, Transaction};
use crate::row::{FromRow, ValueRow};
use crate::schema::{Dialect, TableSchema};

#[cfg(not(target_arch = "wasm32"))]
#[path = "native.rs"]
mod native;
#[cfg(not(target_arch = "wasm32"))]
use native::Transport;

#[cfg(target_arch = "wasm32")]
#[path = "wasm.rs"]
mod wasm;
#[cfg(target_arch = "wasm32")]
use wasm::Transport;

/// An HTTP answer, whatever the transport.
pub(crate) struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Where requests go: scheme, host, port and the path prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Endpoint {
    pub https: bool,
    pub host: String,
    pub port: u16,
    pub base: String,
}

impl Endpoint {
    /// `https://host`, `http://127.0.0.1:4444`, `https://[::1]:8443/nuage`...
    pub(crate) fn parse(url: &str) -> Result<Endpoint> {
        let bad = |why: &str| Error::Conversion(format!("invalid Nuage DB URL `{url}`: {why}"));
        let (https, rest) = if let Some(rest) = url.strip_prefix("https://") {
            (true, rest)
        } else if let Some(rest) = url.strip_prefix("http://") {
            (false, rest)
        } else {
            return Err(bad("it must start with https:// or http://"));
        };
        let (authority, path) = rest.split_at(rest.find(['/', '?', '#']).unwrap_or(rest.len()));
        let path = path.split(['?', '#']).next().unwrap_or_default();
        let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
            let (host, after) = v6.split_once(']').ok_or_else(|| bad("unclosed `[`"))?;
            (host, after.strip_prefix(':'))
        } else {
            match authority.rsplit_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (authority, None),
            }
        };
        if host.is_empty() || authority.contains('@') {
            return Err(bad("no host (the token is given separately, never in the URL)"));
        }
        let port = match port {
            Some(p) => p.parse().map_err(|_| bad("invalid port"))?,
            None if https => 443,
            None => 80,
        };
        Ok(Endpoint { https, host: host.to_owned(), port, base: path.trim_end_matches('/').to_owned() })
    }

    /// `host[:port]`, as the `Host` header wants it.
    pub(crate) fn authority(&self) -> String {
        let host = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        if self.port == if self.https { 443 } else { 80 } { host } else { format!("{host}:{}", self.port) }
    }
}

/// Settings of a [`NuageDb`], from [`NuageDb::builder`].
#[derive(Clone)]
pub struct NuageBuilder {
    url: String,
    token: Option<String>,
    timeout: Duration,
}

impl std::fmt::Debug for NuageBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NuageBuilder").field("url", &self.url).field("timeout", &self.timeout).finish()
    }
}

impl NuageBuilder {
    /// The server's token (`NUAGE_TOKEN` on the server), sent as
    /// `Authorization: Bearer <token>`.
    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into()).filter(|t: &String| !t.is_empty());
        self
    }

    /// How long a request may take, connection included (default: 30
    /// seconds). Not applied on wasm32, where the JavaScript host decides.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Builds the client without contacting the server.
    pub fn build(self) -> Result<NuageDb> {
        let endpoint = Endpoint::parse(&self.url)?;
        let base = endpoint.base.clone();
        let transport = Transport::new(endpoint, self.timeout)?;
        let mut headers = vec![("Content-Type".to_owned(), "application/json".to_owned())];
        if let Some(token) = &self.token {
            headers.push(("Authorization".to_owned(), format!("Bearer {token}")));
        }
        Ok(NuageDb { inner: Arc::new(Inner { transport, base, headers, schemas: Schemas::default() }), branch: None })
    }

    /// Builds the client and checks the server and the token.
    pub async fn connect(self) -> Result<NuageDb> {
        let db = self.build()?;
        let info = db.request("GET", "/v1/info", None, Json::Null).await?;
        // A server that keeps schemas by reference is sent each one whole once.
        if info.get("schemaRefs").and_then(Json::as_bool) == Some(true) {
            db.inner.schemas.by_reference.store(true, Ordering::Relaxed);
        }
        Ok(db)
    }
}

struct Inner {
    transport: Transport,
    base: String,
    /// Headers sent with every request.
    headers: Vec<(String, String)>,
    schemas: Schemas,
}

/// The schemas the server already has: a statement names them by their
/// [reference](wire::schema_ref) instead of describing them each time.
#[derive(Default)]
struct Schemas {
    /// The server said (`/v1/info`) that it keeps schemas by reference.
    by_reference: AtomicBool,
    /// The schemas sent whole in a statement the server accepted, by their
    /// address, with their reference.
    known: Mutex<HashMap<usize, String>>,
    /// Times the server did not know a reference. A server restarted forgets
    /// them and is told again; one that keeps forgetting is sent schemas whole.
    unknown: AtomicU32,
}

/// References refused before this client stops sending any.
const UNKNOWN_SCHEMAS_MAX: u32 = 8;

/// A Nuage DB database reached over HTTP(S). See the [module](self) docs.
///
/// Cloning is cheap: clones share their kept-alive connections.
#[derive(Clone)]
pub struct NuageDb {
    inner: Arc<Inner>,
    branch: Option<String>,
}

impl std::fmt::Debug for NuageDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the headers: they carry the token.
        f.debug_struct("NuageDb").field("base", &self.inner.base).field("branch", &self.branch).finish()
    }
}

impl NuageDb {
    /// Connects to a Nuage DB server (`https://nuage.example.com`) with its
    /// token, and checks both.
    pub async fn connect(url: &str, token: &str) -> Result<Self> {
        Self::builder(url).token(token).connect().await
    }

    /// Settings for a server given by URL.
    pub fn builder(url: &str) -> NuageBuilder {
        NuageBuilder { url: url.to_owned(), token: None, timeout: Duration::from_secs(30) }
    }

    /// The same database seen through a branch: every request of the
    /// returned handle runs on it.
    pub fn on_branch(&self, name: &str) -> NuageDb {
        NuageDb { inner: self.inner.clone(), branch: Some(name.to_owned()) }
    }

    /// The branch this handle runs on, if any.
    pub fn branch_name(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    /// Creates a branch of the main database, or of the branch `from`:
    /// instant, it shares the data and only stores its own changes.
    pub async fn create_branch(&self, name: &str, from: Option<&str>) -> Result<Branch> {
        let res = self.request("POST", "/v1/branches", None, json!({ "name": name, "from": from })).await?;
        Branch::from_json(&res)
    }

    /// The branches.
    pub async fn branches(&self) -> Result<Vec<Branch>> {
        let res = self.request("GET", "/v1/branches", None, Json::Null).await?;
        res.get("branches")
            .and_then(Json::as_array)
            .ok_or_else(|| Error::Conversion("invalid list of branches".into()))?
            .iter()
            .map(Branch::from_json)
            .collect()
    }

    /// Deletes a branch and its own data. Returns `false` when there was no
    /// such branch.
    pub async fn delete_branch(&self, name: &str) -> Result<bool> {
        match self.request("DELETE", &format!("/v1/branches/{name}"), None, Json::Null).await {
            Ok(_) => Ok(true),
            Err(Error::NotFound) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Throws away a branch's changes: it becomes a fresh copy of its parent.
    pub async fn reset_branch(&self, name: &str) -> Result<()> {
        self.request("POST", &format!("/v1/branches/{name}/reset"), None, Json::Null).await.map(|_| ())
    }

    /// Several statements run in one transaction, in one request: all of
    /// them apply, or none. Works with every server; interactive
    /// transactions ([`Database::begin`]) work too.
    pub fn batch(&self) -> NuageBatch<'_> {
        NuageBatch { db: self, statements: Vec::new(), error: None }
    }

    async fn request(&self, method: &'static str, path: &str, tx: Option<&str>, body: Json) -> Result<Json> {
        let mut headers = self.inner.headers.clone();
        if let Some(branch) = &self.branch {
            headers.push((BRANCH_HEADER.to_owned(), branch.clone()));
        }
        if let Some(tx) = tx {
            headers.push((TRANSACTION_HEADER.to_owned(), tx.to_owned()));
        }
        let body = if body.is_null() { Vec::new() } else { body.to_string().into_bytes() };
        let path = format!("{}{path}", self.inner.base);
        let res = self.inner.transport.request(method, &path, &headers, body).await?;
        if !(200..300).contains(&res.status) {
            return Err(wire::error_from_json(res.status, &res.body));
        }
        if res.body.is_empty() {
            return Ok(Json::Null);
        }
        serde_json::from_slice(&res.body).map_err(|e| Error::Conversion(format!("invalid answer from Nuage DB: {e}")))
    }

    /// Sends a statement: the schemas the server has go by reference, the
    /// others whole (and by reference from the next statement on).
    async fn send(&self, path: &str, query: &Query<'_>, tx: Option<&str>) -> Result<Json> {
        let schemas = &self.inner.schemas;
        if !schemas.by_reference.load(Ordering::Relaxed) {
            return self.request("POST", path, tx, json!({ "query": wire::query_to_json(query)? })).await;
        }
        // Described in this statement: known to the server once it has accepted it.
        let described: Mutex<Vec<&'static TableSchema>> = Mutex::new(Vec::new());
        let body = {
            let known = schemas.known.lock().unwrap_or_else(|e| e.into_inner());
            wire::query_to_json_with(query, &|t| match known.get(&(std::ptr::from_ref(t) as usize)) {
                Some(reference) => Json::String(reference.clone()),
                None => {
                    described.lock().unwrap_or_else(|e| e.into_inner()).push(t);
                    wire::schema_to_json(t)
                }
            })?
        };
        match self.request("POST", path, tx, json!({ "query": body })).await {
            Err(e) if wire::is_unknown_schema(&e) => {
                // The server lost its schemas (a restart): everything whole again.
                schemas.known.lock().unwrap_or_else(|e| e.into_inner()).clear();
                if schemas.unknown.fetch_add(1, Ordering::Relaxed) + 1 >= UNKNOWN_SCHEMAS_MAX {
                    schemas.by_reference.store(false, Ordering::Relaxed);
                }
                self.request("POST", path, tx, json!({ "query": wire::query_to_json(query)? })).await
            }
            Err(e) => Err(e),
            Ok(answer) => {
                let described = described.into_inner().unwrap_or_else(|e| e.into_inner());
                if !described.is_empty() {
                    let mut known = schemas.known.lock().unwrap_or_else(|e| e.into_inner());
                    for t in described {
                        known.entry(std::ptr::from_ref(t) as usize).or_insert_with(|| wire::schema_ref(t));
                    }
                }
                Ok(answer)
            }
        }
    }

    async fn fetch_rows(&self, query: &Query<'_>, tx: Option<&str>) -> Result<Vec<Vec<crate::Value>>> {
        let res = self.send("/v1/fetch", query, tx).await?;
        wire::rows_from_json(res.get("rows").unwrap_or(&Json::Null))
    }

    async fn exec(&self, query: &Query<'_>, tx: Option<&str>) -> Result<ExecResult> {
        let res = self.send("/v1/execute", query, tx).await?;
        wire::exec_result_from_json(&res)
    }
}

fn decode<O: FromRow>(rows: Vec<Vec<crate::Value>>) -> Result<Vec<O>> {
    rows.into_iter().map(|r| O::from_row(&mut ValueRow(r), 0)).collect()
}

impl Executor for NuageDb {
    /// `None`: Nuage DB evaluates Memoire statements itself, like the
    /// in-memory engine, without SQL.
    fn dialect(&self) -> Option<Dialect> {
        None
    }

    async fn fetch_all<O>(&self, query: Query<'_>) -> Result<Vec<O>>
    where
        O: FromRow + Send,
    {
        decode(self.fetch_rows(&query, None).await?)
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        self.exec(&query, None).await
    }
}

impl Database for NuageDb {
    type Tx<'c> = NuageTransaction;

    async fn begin(&self) -> Result<NuageTransaction> {
        let res = self.request("POST", "/v1/transactions", None, json!({})).await?;
        let id = res
            .get("id")
            .and_then(Json::as_str)
            .ok_or_else(|| Error::Conversion("the server answered no transaction id".into()))?
            .to_owned();
        Ok(NuageTransaction { db: self.clone(), id: Some(id) })
    }
}

/// An interactive transaction on a Nuage DB server.
///
/// Dropping it without committing rolls it back (in the background); the
/// server also rolls back a transaction left idle for too long.
pub struct NuageTransaction {
    db: NuageDb,
    id: Option<String>,
}

impl std::fmt::Debug for NuageTransaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NuageTransaction").field("open", &self.id.is_some()).finish()
    }
}

impl NuageTransaction {
    fn id(&self) -> Result<&str> {
        self.id.as_deref().ok_or(Error::TransactionFinished)
    }

    async fn finish(mut self, action: &str) -> Result<()> {
        let id = self.id.take().ok_or(Error::TransactionFinished)?;
        self.db.request("POST", &format!("/v1/transactions/{id}/{action}"), None, Json::Null).await.map(|_| ())
    }
}

impl Drop for NuageTransaction {
    fn drop(&mut self) {
        let Some(id) = self.id.take() else { return };
        let db = self.db.clone();
        let rollback = async move {
            let _ = db.request("POST", &format!("/v1/transactions/{id}/rollback"), None, Json::Null).await;
        };
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(rollback);
        }
        // Without a runtime, the server's transaction timeout rolls it back.
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(rollback);
    }
}

impl Executor for NuageTransaction {
    fn dialect(&self) -> Option<Dialect> {
        None
    }

    async fn fetch_all<O>(&self, query: Query<'_>) -> Result<Vec<O>>
    where
        O: FromRow + Send,
    {
        decode(self.db.fetch_rows(&query, Some(self.id()?)).await?)
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        self.db.exec(&query, Some(self.id()?)).await
    }
}

impl Transaction for NuageTransaction {
    fn commit(self) -> impl Future<Output = Result<()>> + Send {
        self.finish("commit")
    }

    fn rollback(self) -> impl Future<Output = Result<()>> + Send {
        self.finish("rollback")
    }
}

/// The result of one statement of a [`NuageBatch`].
#[derive(Clone, Debug, PartialEq)]
pub enum BatchResult {
    /// Rows of a statement added with [`NuageBatch::fetch`].
    Rows(Vec<Vec<crate::Value>>),
    /// Outcome of a statement added with [`NuageBatch::execute`].
    Done(ExecResult),
}

impl BatchResult {
    /// Decodes the rows into `O` (an error for a write).
    pub fn decode<O: FromRow>(self) -> Result<Vec<O>> {
        match self {
            BatchResult::Rows(rows) => decode(rows),
            BatchResult::Done(_) => Err(Error::Unsupported("this statement returns no rows".into())),
        }
    }

    /// Rows affected by a write (`0` for a read).
    pub fn rows_affected(&self) -> u64 {
        match self {
            BatchResult::Done(r) => r.rows_affected,
            BatchResult::Rows(_) => 0,
        }
    }
}

/// Statements sent in one request and run in one transaction, from
/// [`NuageDb::batch`].
#[must_use = "a batch does nothing until `run` is called"]
pub struct NuageBatch<'a> {
    db: &'a NuageDb,
    statements: Vec<Json>,
    error: Option<Error>,
}

impl std::fmt::Debug for NuageBatch<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NuageBatch").field("statements", &self.statements.len()).finish()
    }
}

impl NuageBatch<'_> {
    fn push(mut self, kind: &str, query: Query<'_>) -> Self {
        match wire::query_to_json(&query) {
            Ok(q) => {
                let mut statement = serde_json::Map::new();
                statement.insert(kind.to_owned(), q);
                self.statements.push(Json::Object(statement));
            }
            Err(e) => self.error = self.error.or(Some(e)),
        }
        self
    }

    /// Adds a statement returning rows.
    pub fn fetch(self, query: Query<'_>) -> Self {
        self.push("fetch", query)
    }

    /// Adds a write.
    pub fn execute(self, query: Query<'_>) -> Self {
        self.push("execute", query)
    }

    /// Number of statements.
    pub fn len(&self) -> usize {
        self.statements.len()
    }

    /// `true` when the batch holds no statement.
    pub fn is_empty(&self) -> bool {
        self.statements.is_empty()
    }

    /// Sends the batch; one result per statement.
    pub async fn run(self) -> Result<Vec<BatchResult>> {
        if let Some(e) = self.error {
            return Err(e);
        }
        let res = self.db.request("POST", "/v1/batch", None, json!({ "statements": self.statements })).await?;
        res.get("results")
            .and_then(Json::as_array)
            .ok_or_else(|| Error::Conversion("invalid batch answer".into()))?
            .iter()
            .map(|r| match r.get("rows") {
                Some(rows) => Ok(BatchResult::Rows(wire::rows_from_json(rows)?)),
                None => Ok(BatchResult::Done(wire::exec_result_from_json(r)?)),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints() {
        let e = Endpoint::parse("https://db.example.com").unwrap();
        assert_eq!((e.https, e.host.as_str(), e.port, e.base.as_str()), (true, "db.example.com", 443, ""));
        assert_eq!(e.authority(), "db.example.com");
        let e = Endpoint::parse("http://127.0.0.1:4444/").unwrap();
        assert_eq!((e.port, e.base.as_str(), e.authority().as_str()), (4444, "", "127.0.0.1:4444"));
        let e = Endpoint::parse("https://[::1]:8443/nuage/").unwrap();
        assert_eq!((e.host.as_str(), e.base.as_str(), e.authority().as_str()), ("::1", "/nuage", "[::1]:8443"));
        assert!(Endpoint::parse("ftp://x").is_err());
        assert!(Endpoint::parse("https://user:pw@x").is_err());
    }
}
