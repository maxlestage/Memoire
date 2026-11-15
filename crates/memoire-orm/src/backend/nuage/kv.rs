//! `NuageKv`: the key-value store of a Nuage DB application, in memory on the
//! server, the way Redis is used: a cache, sessions, counters, rate limits,
//! locks.
//!
//! ```no_run
//! # async fn demo() -> memoire_orm::Result<()> {
//! use std::time::Duration;
//! use memoire_orm::NuageKv;
//!
//! let kv = NuageKv::from_env()?; // NUAGE_URL, NUAGE_TOKEN (or `db.kv()` from a NuageDb)
//! kv.set_for("session:42", "ada", Duration::from_secs(1800)).await?;
//! let user = kv.get_string("session:42").await?; // Some("ada")
//! // 100 requests a minute for each visitor:
//! let hits = kv.incr_by("rate:203.0.113.7", 1, Some(Duration::from_secs(60))).await?;
//! // One worker at a time:
//! if kv.set_if_absent("lock:report", "me", Some(Duration::from_secs(30))).await? {
//!     // ... the work ...
//!     kv.delete("lock:report").await?;
//! }
//! # Ok(()) }
//! ```
//!
//! The store is in the server's memory: a read or a write costs one round
//! trip and no disk, and a restart of the server empties it. What must survive
//! goes in the database. Each application has its own store (the token picks
//! it), bounded in memory: past the limit the server drops the keys that
//! expired, then the least recently used ones.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value as Json;

use super::{HttpResponse, Inner, NuageDb, wire};
use crate::error::{Error, Result};

/// The key-value store of a Nuage DB application. See the [module](self) docs.
///
/// Cloning it is cheap: the clones share their connections.
#[derive(Clone)]
pub struct NuageKv {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for NuageKv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the headers: they carry the token.
        f.debug_struct("NuageKv").field("base", &self.inner.base).finish_non_exhaustive()
    }
}

fn bad_answer(what: &str) -> Error {
    Error::Conversion(format!("unexpected answer from the Nuage DB server: no {what}"))
}

/// A key in a path: everything but letters, digits, `-`, `_`, `.`, `~` and
/// `:` as `%XX`.
fn encode(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for b in key.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b':') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(char::from_digit(u32::from(b >> 4), 16).unwrap_or('0').to_ascii_uppercase());
            out.push(char::from_digit(u32::from(b & 15), 16).unwrap_or('0').to_ascii_uppercase());
        }
    }
    out
}

/// A duration as the seconds of a `ttl`: `60`, `0.25`.
fn seconds(ttl: Duration) -> String {
    // At least a millisecond: the server refuses 0.
    format!("{}", ttl.as_millis().max(1) as f64 / 1000.0)
}

impl NuageDb {
    /// The key-value store of this application, over the same connections.
    pub fn kv(&self) -> NuageKv {
        NuageKv { inner: self.inner.clone() }
    }
}

impl NuageKv {
    /// The key-value store of the application `token` opens, on the server at
    /// `url`. Nothing is sent before the first call.
    pub fn new(url: &str, token: &str) -> Result<NuageKv> {
        Ok(NuageDb::builder(url).token(token).build()?.kv())
    }

    /// [`new`](Self::new) with `NUAGE_URL` and `NUAGE_TOKEN`.
    pub fn from_env() -> Result<NuageKv> {
        let var = |name: &str| {
            std::env::var(name)
                .ok()
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| Error::Conversion(format!("{name} is not set")))
        };
        NuageKv::new(&var("NUAGE_URL")?, &var("NUAGE_TOKEN")?)
    }

    async fn raw(&self, method: &'static str, path: &str, body: Vec<u8>) -> Result<HttpResponse> {
        let path = format!("{}/v1/kv{path}", self.inner.base);
        self.inner.transport.request(method, &path, &self.inner.headers, body).await
    }

    /// A request whose answer is JSON.
    async fn call(&self, method: &'static str, path: &str, body: Vec<u8>) -> Result<Json> {
        let res = self.raw(method, path, body).await?;
        if !(200..300).contains(&res.status) {
            return Err(wire::error_from_json(res.status, &res.body));
        }
        serde_json::from_slice(&res.body).map_err(|e| Error::Conversion(format!("invalid JSON from the server: {e}")))
    }

    /// The value of `key`, as it was written; `None` when there is none (or
    /// it expired).
    pub async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let res = self.raw("GET", &format!("/{}", encode(key)), Vec::new()).await?;
        match res.status {
            200 => Ok(Some(res.body)),
            404 => Ok(None),
            status => Err(wire::error_from_json(status, &res.body)),
        }
    }

    /// The value of `key` as text.
    pub async fn get_string(&self, key: &str) -> Result<Option<String>> {
        match self.get(key).await? {
            Some(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| Error::Conversion(format!("the value of `{key}` is not UTF-8 text"))),
            None => Ok(None),
        }
    }

    async fn put(&self, key: &str, value: &[u8], query: &str) -> Result<bool> {
        let answer = self.call("PUT", &format!("/{}{query}", encode(key)), value.to_vec()).await?;
        answer.get("set").and_then(Json::as_bool).ok_or_else(|| bad_answer("`set`"))
    }

    /// Writes `value` under `key`, until it is deleted or overwritten.
    pub async fn set(&self, key: &str, value: impl AsRef<[u8]>) -> Result<()> {
        self.put(key, value.as_ref(), "").await.map(|_| ())
    }

    /// Writes `value` under `key`; it expires after `ttl`.
    pub async fn set_for(&self, key: &str, value: impl AsRef<[u8]>, ttl: Duration) -> Result<()> {
        self.put(key, value.as_ref(), &format!("?ttl={}", seconds(ttl))).await.map(|_| ())
    }

    /// Writes `value` under `key` only when the key does not exist; `true`
    /// when it was written. With a `ttl`, a lock that frees itself if its
    /// holder never does.
    pub async fn set_if_absent(&self, key: &str, value: impl AsRef<[u8]>, ttl: Option<Duration>) -> Result<bool> {
        let query = match ttl {
            Some(ttl) => format!("?nx&ttl={}", seconds(ttl)),
            None => "?nx".to_owned(),
        };
        self.put(key, value.as_ref(), &query).await
    }

    /// Deletes `key`; `true` when it existed.
    pub async fn delete(&self, key: &str) -> Result<bool> {
        let answer = self.call("DELETE", &format!("/{}", encode(key)), Vec::new()).await?;
        Ok(answer.get("deleted").and_then(Json::as_u64).ok_or_else(|| bad_answer("`deleted`"))? > 0)
    }

    /// Adds 1 to the counter `key` (0 when it does not exist) and returns it.
    pub async fn incr(&self, key: &str) -> Result<i64> {
        self.incr_by(key, 1, None).await
    }

    /// Adds `by` to the counter `key` and returns it. `ttl_if_new` is the
    /// expiry of a counter this call creates: `incr_by(key, 1, Some(window))`
    /// counts the events of a window, a rate limit in one round trip.
    pub async fn incr_by(&self, key: &str, by: i64, ttl_if_new: Option<Duration>) -> Result<i64> {
        let ttl = ttl_if_new.map(|ttl| format!("&ttl={}", seconds(ttl))).unwrap_or_default();
        let answer = self.call("POST", &format!("/{}?incr={by}{ttl}", encode(key)), Vec::new()).await?;
        answer.get("value").and_then(Json::as_i64).ok_or_else(|| bad_answer("`value`"))
    }

    /// Makes `key` expire after `ttl`; `false` when it does not exist.
    pub async fn expire(&self, key: &str, ttl: Duration) -> Result<bool> {
        let answer = self.call("POST", &format!("/{}?expire={}", encode(key), seconds(ttl)), Vec::new()).await?;
        answer.get("set").and_then(Json::as_bool).ok_or_else(|| bad_answer("`set`"))
    }

    /// Makes `key` never expire; `false` when it does not exist.
    pub async fn persist(&self, key: &str) -> Result<bool> {
        let answer = self.call("POST", &format!("/{}?persist", encode(key)), Vec::new()).await?;
        answer.get("set").and_then(Json::as_bool).ok_or_else(|| bad_answer("`set`"))
    }

    /// How long `key` still lives: `None` when it never expires, or does not
    /// exist.
    pub async fn ttl(&self, key: &str) -> Result<Option<Duration>> {
        let ms = self.command(&["PTTL", key]).await?.as_i64().ok_or_else(|| bad_answer("number"))?;
        Ok(u64::try_from(ms).ok().map(Duration::from_millis))
    }

    /// `key` exists.
    pub async fn exists(&self, key: &str) -> Result<bool> {
        Ok(self.command(&["EXISTS", key]).await?.as_i64() == Some(1))
    }

    /// The keys that start with `prefix` (all of them for `""`), sorted,
    /// 10 000 at most.
    pub async fn keys(&self, prefix: &str) -> Result<Vec<String>> {
        let answer = self.call("GET", &format!("?limit=10000&prefix={}", encode(prefix)), Vec::new()).await?;
        answer
            .get("keys")
            .and_then(Json::as_array)
            .ok_or_else(|| bad_answer("`keys`"))?
            .iter()
            .map(|k| k.get("key").and_then(Json::as_str).map(str::to_owned).ok_or_else(|| bad_answer("`key`")))
            .collect()
    }

    /// Deletes every key of the application; returns how many there were.
    pub async fn clear(&self) -> Result<u64> {
        let answer = self.call("DELETE", "", Vec::new()).await?;
        answer.get("deleted").and_then(Json::as_u64).ok_or_else(|| bad_answer("`deleted`"))
    }

    /// One command of Redis, its name first: `kv.command(&["SET", "k", "v",
    /// "EX", "60"])`. Its answer as JSON: text, a number, `null` for nothing,
    /// a list. A command the server refuses is an [`Error::Validation`].
    pub async fn command<A: AsRef<str>>(&self, args: &[A]) -> Result<Json> {
        let body: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
        let body = serde_json::to_vec(&body).map_err(Error::conversion)?;
        let mut answer = self.call("POST", "", body).await?;
        answer.get_mut("result").map(Json::take).ok_or_else(|| bad_answer("`result`"))
    }

    /// Several commands in one round trip, each answered in turn. One that
    /// the server refuses answers `{"error": "..."}` and does not stop the
    /// others.
    pub async fn pipeline<A: AsRef<str>>(&self, commands: &[Vec<A>]) -> Result<Vec<Json>> {
        let body: Vec<Vec<&str>> = commands.iter().map(|c| c.iter().map(AsRef::as_ref).collect()).collect();
        let body = serde_json::to_vec(&serde_json::json!({ "commands": body })).map_err(Error::conversion)?;
        let mut answer = self.call("POST", "", body).await?;
        match answer.get_mut("results").map(Json::take) {
            Some(Json::Array(results)) => Ok(results),
            _ => Err(bad_answer("`results`")),
        }
    }

    /// The value of `key`, read from the JSON [`set_json`](Self::set_json)
    /// wrote.
    pub async fn get_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        match self.get(key).await? {
            Some(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| Error::Conversion(format!("the value of `{key}` is not the JSON expected: {e}"))),
            None => Ok(None),
        }
    }

    /// Writes `value` under `key` as JSON; with a `ttl`, it expires.
    pub async fn set_json<T: serde::Serialize + ?Sized>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<()> {
        let bytes = serde_json::to_vec(value).map_err(Error::conversion)?;
        match ttl {
            Some(ttl) => self.set_for(key, bytes, ttl).await,
            None => self.set(key, bytes).await,
        }
    }

    /// A cache in one call: the value kept under `key`, or else what
    /// `compute` answers, kept for `ttl`.
    ///
    /// ```no_run
    /// # async fn demo(kv: memoire_orm::NuageKv) -> memoire_orm::Result<()> {
    /// # async fn slow_report() -> memoire_orm::Result<Vec<u32>> { Ok(vec![]) }
    /// use std::time::Duration;
    /// let report: Vec<u32> = kv.remember("report", Duration::from_secs(300), slow_report).await?;
    /// # Ok(()) }
    /// ```
    ///
    /// A value kept that does not read as `T` (the type changed) is computed
    /// again and replaced.
    pub async fn remember<T, F, Fut>(&self, key: &str, ttl: Duration, compute: F) -> Result<T>
    where
        T: serde::Serialize + serde::de::DeserializeOwned,
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        if let Some(bytes) = self.get(key).await?
            && let Ok(value) = serde_json::from_slice(&bytes)
        {
            return Ok(value);
        }
        let value = compute().await?;
        self.set_json(key, &value, Some(ttl)).await?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_durations_in_urls() {
        assert_eq!(encode("session:42"), "session:42");
        assert_eq!(encode("a/b c?d&e=é%"), "a%2Fb%20c%3Fd%26e%3D%C3%A9%25");
        assert_eq!(seconds(Duration::from_secs(60)), "60");
        assert_eq!(seconds(Duration::from_millis(250)), "0.25");
        assert_eq!(seconds(Duration::from_nanos(1)), "0.001");
        assert_eq!(seconds(Duration::from_secs(86_400 * 30)), "2592000");
    }
}
