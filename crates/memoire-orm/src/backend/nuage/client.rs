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

use std::collections::{HashMap, HashSet};
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
    /// The schemas sent whole in a statement the server accepted.
    known: Mutex<Known>,
    /// Times the server did not know a reference. A server restarted forgets
    /// them and is told again; one that keeps forgetting is sent schemas whole.
    unknown: AtomicU32,
}

/// References refused before this client stops sending any.
const UNKNOWN_SCHEMAS_MAX: u32 = 8;

/// What the server has.
#[derive(Default)]
struct Known {
    /// The references of the schemas it was sent.
    references: HashSet<String>,
    /// The reference of a schema by its address, worked out once. One table's
    /// schema may sit at several addresses (a constant, named in several places).
    by_address: HashMap<usize, String>,
}

impl Schemas {
    /// The reference of `t`, when the server has the schema.
    fn reference(&self, t: &'static TableSchema) -> Option<String> {
        let address = std::ptr::from_ref(t) as usize;
        let mut known = self.known.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(reference) = known.by_address.get(&address) {
            return Some(reference.clone());
        }
        if known.references.is_empty() {
            return None;
        }
        let reference = wire::schema_ref(t);
        if !known.references.contains(&reference) {
            return None;
        }
        known.by_address.insert(address, reference.clone());
        Some(reference)
    }

    /// The server accepted a statement that described these schemas: it has them.
    fn learn(&self, described: Vec<&'static TableSchema>) {
        if described.is_empty() {
            return;
        }
        let mut known = self.known.lock().unwrap_or_else(|e| e.into_inner());
        for t in described {
            let reference = wire::schema_ref(t);
            known.by_address.insert(std::ptr::from_ref(t) as usize, reference.clone());
            known.references.insert(reference);
        }
    }

    /// The server did not know a reference: it has none of them any more.
    fn forget(&self) {
        *self.known.lock().unwrap_or_else(|e| e.into_inner()) = Known::default();
        if self.unknown.fetch_add(1, Ordering::Relaxed) + 1 >= UNKNOWN_SCHEMAS_MAX {
            self.by_reference.store(false, Ordering::Relaxed);
        }
    }
}

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
        NuageBatch { db: self, statements: Vec::new(), error: None, schemas: Vec::new(), described: Vec::new() }
    }

    async fn request(&self, method: &'static str, path: &str, tx: Option<&str>, body: Json) -> Result<Json> {
        let body = if body.is_null() { Vec::new() } else { body.to_string().into_bytes() };
        self.request_bytes(method, path, tx, body).await
    }

    /// [`request`](Self::request) with the body already written.
    async fn request_bytes(&self, method: &'static str, path: &str, tx: Option<&str>, body: Vec<u8>) -> Result<Json> {
        let mut headers = self.inner.headers.clone();
        if let Some(branch) = &self.branch {
            headers.push((BRANCH_HEADER.to_owned(), branch.clone()));
        }
        if let Some(tx) = tx {
            headers.push((TRANSACTION_HEADER.to_owned(), tx.to_owned()));
        }
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
        let body = wire::query_to_json_with(query, &|t| match schemas.reference(t) {
            Some(reference) => Json::String(reference),
            None => {
                described.lock().unwrap_or_else(|e| e.into_inner()).push(t);
                wire::schema_to_json(t)
            }
        })?;
        match self.request("POST", path, tx, json!({ "query": body })).await {
            Err(e) if wire::is_unknown_schema(&e) => {
                // The server lost its schemas (a restart): everything whole again.
                schemas.forget();
                self.request("POST", path, tx, json!({ "query": wire::query_to_json(query)? })).await
            }
            Err(e) => Err(e),
            Ok(answer) => {
                schemas.learn(described.into_inner().unwrap_or_else(|e| e.into_inner()));
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
    /// The schemas its statements name, each once.
    schemas: Vec<&'static TableSchema>,
    /// Those of them written whole, because the server does not have them yet.
    described: Vec<&'static TableSchema>,
}

impl std::fmt::Debug for NuageBatch<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NuageBatch").field("statements", &self.statements.len()).finish()
    }
}

impl NuageBatch<'_> {
    fn push(mut self, kind: &str, query: Query<'_>) -> Self {
        let refs = &self.db.inner.schemas;
        let encoded = if refs.by_reference.load(Ordering::Relaxed) {
            // As single statements do: by reference what the server has, whole the rest.
            let named = std::cell::RefCell::new(Vec::new());
            let encoded = wire::query_to_json_with(&query, &|t| {
                let reference = refs.reference(t);
                named.borrow_mut().push((t, reference.is_some()));
                match reference {
                    Some(reference) => Json::String(reference),
                    None => wire::schema_to_json(t),
                }
            });
            for (t, by_reference) in named.into_inner() {
                if !self.schemas.iter().any(|s| std::ptr::eq(*s, t)) {
                    self.schemas.push(t);
                    if !by_reference {
                        self.described.push(t);
                    }
                }
            }
            encoded
        } else {
            wire::query_to_json(&query)
        };
        match encoded {
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
        let body = json!({ "statements": self.statements }).to_string().into_bytes();
        let refs = &self.db.inner.schemas;
        let res = match self.db.request_bytes("POST", "/v1/batch", None, body.clone()).await {
            Err(e) if wire::is_unknown_schema(&e) => {
                // The server lost its schemas (a restart), and a batch refused for that
                // did nothing. It is told each schema again (a lookup that finds no
                // row describes one), then sent the same batch.
                refs.forget();
                for t in &self.schemas {
                    let describe = json!({ "findByKey": wire::schema_to_json(t), "key": wire::value_to_json(&crate::Value::Null) });
                    match self.db.request("POST", "/v1/fetch", None, json!({ "query": describe })).await {
                        Ok(_) | Err(Error::NoSuchTable(_)) => {}
                        Err(e) => return Err(e),
                    }
                }
                let res = self.db.request_bytes("POST", "/v1/batch", None, body).await?;
                refs.learn(self.schemas);
                res
            }
            Err(e) => return Err(e),
            Ok(res) => {
                refs.learn(self.described);
                res
            }
        };
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

    /// A server that keeps schemas by reference gets each one whole once, then
    /// its reference; when it has forgotten them (a restart), it says so and
    /// gets the statement again with its schemas whole.
    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn schemas_go_by_reference_once_the_server_has_them() {
        use std::io::{Read, Write};
        use std::sync::atomic::AtomicBool;

        use crate::prelude::*;

        #[derive(Model, Debug, Clone, PartialEq)]
        #[memoire(table = "client_notes")]
        struct Note {
            #[memoire(primary_key)]
            id: i64,
            text: String,
        }

        // A server of a few lines: it records what it is sent, and can forget its schemas.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let bodies: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let forgot = Arc::new(AtomicBool::new(false));
        let (seen, lost) = (bodies.clone(), forgot.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let (seen, lost) = (seen.clone(), lost.clone());
                std::thread::spawn(move || {
                    let mut buf = Vec::new();
                    loop {
                        // One request: its head, then as many bytes as it announces.
                        let head_end = loop {
                            if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                break at + 4;
                            }
                            let mut chunk = [0u8; 4096];
                            match stream.read(&mut chunk) {
                                Ok(n) if n > 0 => buf.extend_from_slice(&chunk[..n]),
                                _ => return,
                            }
                        };
                        let head = String::from_utf8_lossy(&buf[..head_end]).to_lowercase();
                        let length: usize = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .map_or(0, |v| v.trim().parse().unwrap());
                        while buf.len() < head_end + length {
                            let mut chunk = [0u8; 4096];
                            match stream.read(&mut chunk) {
                                Ok(n) if n > 0 => buf.extend_from_slice(&chunk[..n]),
                                _ => return,
                            }
                        }
                        let body = String::from_utf8_lossy(&buf[head_end..head_end + length]).into_owned();
                        buf.drain(..head_end + length);
                        let (status, answer) = if head.starts_with("get /v1/info") {
                            ("200 OK", r#"{"schemaRefs":true}"#.to_owned())
                        } else {
                            let by_reference = body.contains(r#""findByKey":""#);
                            seen.lock().unwrap().push(body);
                            if by_reference && lost.swap(false, Ordering::SeqCst) {
                                let message = format!("{} `client_notes#0`: send the schema", wire::UNKNOWN_SCHEMA);
                                (
                                    "400 Bad Request",
                                    json!({ "error": { "kind": "unsupported", "message": message } }).to_string(),
                                )
                            } else if head.starts_with("post /v1/batch") {
                                ("200 OK", r#"{"results":[{"rows":[]}]}"#.to_owned())
                            } else {
                                ("200 OK", r#"{"rows":[]}"#.to_owned())
                            }
                        };
                        let reply = format!(
                            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{answer}",
                            answer.len()
                        );
                        if stream.write_all(reply.as_bytes()).is_err() {
                            return;
                        }
                    }
                });
            }
        });

        let db = NuageDb::connect(&url, "token").await.unwrap();
        let whole = |body: &str| body.contains(r#""findByKey":{"#);
        let reference = format!(r#""findByKey":"{}""#, wire::schema_ref(Note::SCHEMA));
        // Whole the first time, by reference after.
        assert_eq!(Note::find(&db, 1).await.unwrap(), None);
        assert_eq!(Note::find(&db, 2).await.unwrap(), None);
        // The server forgot: told so, the client sends the statement again, whole.
        forgot.store(true, Ordering::SeqCst);
        assert_eq!(Note::find(&db, 3).await.unwrap(), None);
        // Then whole once more (the server learns it again), and by reference.
        assert_eq!(Note::find(&db, 4).await.unwrap(), None);
        assert_eq!(Note::find(&db, 5).await.unwrap(), None);
        let sent = bodies.lock().unwrap().clone();
        let kinds: Vec<&str> = sent
            .iter()
            .map(|b| match (whole(b), b.contains(&reference)) {
                (true, false) => "whole",
                (false, true) => "reference",
                _ => "?",
            })
            .collect();
        assert_eq!(kinds, ["whole", "reference", "reference", "whole", "whole", "reference"], "{sent:#?}");
        assert!(
            sent[1].len() * 3 < sent[0].len(),
            "a reference is much shorter: {} then {}",
            sent[0].len(),
            sent[1].len()
        );
        // A batch names its schemas the same way. Refused for a reference the server
        // lost, it did nothing: the server is told the schema again, then sent the same batch.
        let key = crate::Value::I64(8);
        let batch = || db.batch().fetch(Query::FindByKey { table: Note::SCHEMA, key: &key });
        assert_eq!(batch().run().await.unwrap().len(), 1);
        forgot.store(true, Ordering::SeqCst);
        assert_eq!(batch().run().await.unwrap().len(), 1);
        assert_eq!(batch().run().await.unwrap().len(), 1);
        let sent = bodies.lock().unwrap()[6..].to_vec();
        let kinds: Vec<&str> = sent
            .iter()
            .map(|b| match (whole(b), b.contains(&reference), b.contains("statements")) {
                (false, true, true) => "batch by reference",
                (true, false, false) => "schema told again",
                _ => "?",
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "batch by reference",
                "batch by reference",
                "schema told again",
                "batch by reference",
                "batch by reference"
            ],
            "{sent:#?}"
        );
        // A client that did not ask the server what it knows describes its schemas each time.
        let lazy = NuageDb::builder(&url).token("token").build().unwrap();
        for id in [6, 7] {
            assert_eq!(Note::find(&lazy, id).await.unwrap(), None);
        }
        assert!(bodies.lock().unwrap()[11..].iter().all(|b| whole(b)));
    }
}
