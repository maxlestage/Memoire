//! [`NuageAdmin`]: the management API of a Nuage DB server (`/api/apps`), from Rust.
//!
//! A Nuage DB server holds several applications, each with a database of its own; the token
//! an application sends picks its database. `NuageAdmin` makes them, and their tokens, with
//! the server's management token (`NUAGE_ADMIN_TOKEN`, set on the server):
//!
//! ```no_run
//! # async fn demo() -> memoire_orm::Result<()> {
//! use memoire_orm::prelude::*;
//!
//! let admin = NuageAdmin::new("https://nuage.example.com", "the server's NUAGE_ADMIN_TOKEN")?;
//! let shop = admin.create_app("shop", "The shop").await?;
//! println!("{}", shop.env()); // NUAGE_URL=https://nuage.example.com\nNUAGE_TOKEN=nuage_…
//!
//! // The new application's own database, with its own token.
//! let db = NuageDb::connect(&shop.url, &shop.token).await?;
//! # let _ = db; Ok(()) }
//! ```
//!
//! The management token makes and revokes the tokens of every application: keep it with the
//! server's secrets, in a deploy script or a terminal, never in an application or a page. An
//! application's token never opens this API.

use std::time::Duration;

use serde_json::{Value as Json, json};

use super::{Endpoint, Transport};
use crate::backend::nuage::wire;
use crate::error::{Error, Result};

/// A client of the management API of a Nuage DB server.
pub struct NuageAdmin {
    transport: Transport,
    base: String,
    url: String,
    headers: Vec<(String, String)>,
}

impl std::fmt::Debug for NuageAdmin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the headers: they carry the management token.
        f.debug_struct("NuageAdmin").field("url", &self.url).finish()
    }
}

/// An application of the server, as the management API shows it: never a token itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInfo {
    /// Its name, also the name of its database.
    pub name: String,
    /// A note shown in the console.
    pub description: String,
    /// The server's own application (`NUAGE_NAME`).
    pub first: bool,
    /// When it was made, Unix time in milliseconds.
    pub created_at: i64,
    /// Its tokens, by id.
    pub tokens: Vec<TokenInfo>,
    /// The server's `NUAGE_TOKEN` opens it too (the first application).
    pub env_token: bool,
    /// Websites whose pages may call it from the browser (CORS).
    pub origins: Vec<String>,
}

/// A token of an application, as listed: its id, never the token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenInfo {
    /// The 8 characters after `nuage_`: enough to tell tokens apart, not to guess one.
    pub id: String,
    /// When it was made, Unix time in milliseconds.
    pub created_at: i64,
    /// It reads and never writes (for the JavaScript of a website).
    pub read_only: bool,
}

/// A new application, with its first token: the only time the token is shown.
#[derive(Clone, PartialEq, Eq)]
pub struct NewApp {
    /// The application.
    pub app: AppInfo,
    /// Its token (`nuage_…`): what the application connects with.
    pub token: String,
    /// The server's address, as the application should use it (`NUAGE_URL`).
    pub url: String,
}

impl NewApp {
    /// The two lines to paste in the application's configuration:
    /// `NUAGE_URL=…` and `NUAGE_TOKEN=…`.
    pub fn env(&self) -> String {
        format!("NUAGE_URL={}\nNUAGE_TOKEN={}", self.url, self.token)
    }
}

impl std::fmt::Debug for NewApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewApp").field("app", &self.app).field("url", &self.url).finish_non_exhaustive()
    }
}

/// A new token of an application.
#[derive(Clone, PartialEq, Eq)]
pub struct NewToken {
    /// The application.
    pub app: String,
    /// The token, shown this once.
    pub token: String,
    /// It reads and never writes.
    pub read_only: bool,
    /// The server's address (`NUAGE_URL`).
    pub url: String,
}

impl std::fmt::Debug for NewToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewToken")
            .field("app", &self.app)
            .field("read_only", &self.read_only)
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

fn bad_answer(what: &str) -> Error {
    Error::Conversion(format!(
        "the management API answered without {what}: is this a Nuage DB server of 0.4.2 or later?"
    ))
}

fn text(j: &Json, key: &str) -> String {
    j.get(key).and_then(Json::as_str).unwrap_or_default().to_owned()
}

fn token_info(j: &Json) -> TokenInfo {
    TokenInfo {
        id: text(j, "id"),
        created_at: j.get("created_at").and_then(Json::as_i64).unwrap_or_default(),
        read_only: j.get("read_only").and_then(Json::as_bool).unwrap_or_default(),
    }
}

fn app_info(j: &Json) -> Result<AppInfo> {
    let name = j.get("name").and_then(Json::as_str).ok_or_else(|| bad_answer("an application name"))?;
    Ok(AppInfo {
        name: name.to_owned(),
        description: text(j, "description"),
        first: j.get("first").and_then(Json::as_bool).unwrap_or_default(),
        created_at: j.get("created_at").and_then(Json::as_i64).unwrap_or_default(),
        tokens: j
            .get("tokens")
            .and_then(Json::as_array)
            .map(|t| t.iter().map(token_info).collect())
            .unwrap_or_default(),
        env_token: j.get("env_token").and_then(Json::as_bool).unwrap_or_default(),
        origins: j
            .get("origins")
            .and_then(Json::as_array)
            .map(|o| o.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
            .unwrap_or_default(),
    })
}

/// `env.NUAGE_URL` of an answer, else the address the client was given.
fn url_of(answer: &Json, fallback: &str) -> String {
    answer.pointer("/env/NUAGE_URL").and_then(Json::as_str).map_or_else(|| fallback.to_owned(), str::to_owned)
}

/// A path segment: an application's name or a token's id, as the server reads them.
fn segment(s: &str) -> Result<&str> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return Err(Error::Validation(format!("`{s}` is not a valid name: 1 to 63 of a-z, 0-9, `-` and `_`")));
    }
    Ok(s)
}

impl NuageAdmin {
    /// A client of the management API of the server at `url` (`https://nuage.example.com`),
    /// with its management token (`NUAGE_ADMIN_TOKEN`). Nothing is sent yet.
    pub fn new(url: &str, admin_token: &str) -> Result<Self> {
        let url = url.trim().trim_end_matches('/');
        let endpoint = Endpoint::parse(url)?;
        let base = endpoint.base.clone();
        let transport = Transport::new(endpoint, Duration::from_secs(30))?;
        let headers = vec![
            ("Content-Type".to_owned(), "application/json".to_owned()),
            ("Authorization".to_owned(), format!("Bearer {}", admin_token.trim())),
        ];
        Ok(NuageAdmin { transport, base, url: url.to_owned(), headers })
    }

    /// The same as [`new`](Self::new), from `NUAGE_URL` and `NUAGE_ADMIN_TOKEN` (native only).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_env() -> Result<Self> {
        let var = |k: &str| {
            std::env::var(k)
                .ok()
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| Error::Validation(format!("{k} is not set")))
        };
        Self::new(&var("NUAGE_URL")?, &var("NUAGE_ADMIN_TOKEN")?)
    }

    async fn call(&self, method: &'static str, path: &str, body: Option<Json>) -> Result<Json> {
        let body = body.map(|b| b.to_string().into_bytes()).unwrap_or_default();
        let path = format!("{}{path}", self.base);
        let res = self.transport.request(method, &path, &self.headers, body).await?;
        if !(200..300).contains(&res.status) {
            return Err(wire::error_from_json(res.status, &res.body));
        }
        serde_json::from_slice(&res.body).map_err(|e| Error::Conversion(format!("invalid JSON from the server: {e}")))
    }

    /// Every application of the server: the first one first, then by name.
    pub async fn apps(&self) -> Result<Vec<AppInfo>> {
        let answer = self.call("GET", "/api/apps", None).await?;
        answer
            .get("applications")
            .and_then(Json::as_array)
            .ok_or_else(|| bad_answer("`applications`"))?
            .iter()
            .map(app_info)
            .collect()
    }

    /// The application `name`, or `None` when the server has none such.
    pub async fn app(&self, name: &str) -> Result<Option<AppInfo>> {
        match self.call("GET", &format!("/api/apps/{}", segment(name)?), None).await {
            Ok(answer) => app_info(&answer).map(Some),
            Err(Error::NotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Creates the application `name` (1 to 63 of `a-z 0-9 - _`), with its own encrypted
    /// database, and its first token. A name already taken is
    /// [`Error::UniqueViolation`]; see [`app`](Self::app) to check first.
    pub async fn create_app(&self, name: &str, description: &str) -> Result<NewApp> {
        let answer =
            self.call("POST", "/api/apps", Some(json!({ "name": segment(name)?, "description": description }))).await?;
        Ok(NewApp {
            app: app_info(answer.get("application").ok_or_else(|| bad_answer("`application`"))?)?,
            token: answer.get("token").and_then(Json::as_str).ok_or_else(|| bad_answer("`token`"))?.to_owned(),
            url: url_of(&answer, &self.url),
        })
    }

    /// Another token for the application `app`; `read_only`: it reads, never writes (for
    /// the JavaScript of a website).
    pub async fn new_token(&self, app: &str, read_only: bool) -> Result<NewToken> {
        let path = format!("/api/apps/{}/tokens", segment(app)?);
        let answer = self.call("POST", &path, Some(json!({ "read_only": read_only }))).await?;
        Ok(NewToken {
            app: app.to_owned(),
            token: answer.get("token").and_then(Json::as_str).ok_or_else(|| bad_answer("`token`"))?.to_owned(),
            read_only,
            url: url_of(&answer, &self.url),
        })
    }

    /// Revokes the token `id` (as [`TokenInfo::id`] lists it) of the application `app`:
    /// `false` when it has none such.
    pub async fn revoke_token(&self, app: &str, id: &str) -> Result<bool> {
        match self.call("DELETE", &format!("/api/apps/{}/tokens/{}", segment(app)?, segment(id)?), None).await {
            Ok(_) => Ok(true),
            Err(Error::NotFound) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A server of a few lines that answers like `/api/apps`, and records what it is sent.
    fn server() -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let public = url.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let (log, public) = (log.clone(), public.clone());
                std::thread::spawn(move || {
                    let mut buf = Vec::new();
                    loop {
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
                        let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
                        let length: usize = head
                            .to_lowercase()
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
                        let line = head.lines().next().unwrap_or_default().to_owned();
                        log.lock().unwrap().push(format!("{line} {body}"));
                        let authorized = head.contains("Bearer admin-0123456789abcdef0123456789abcdef");
                        let app = r#"{"name":"shop","description":"La boutique","first":false,"created_at":5,"tokens":[{"id":"3f9a12c0","created_at":5,"read_only":false}],"env_token":false,"origins":[]}"#;
                        let (status, answer) = if !authorized {
                            (
                                "401 Unauthorized",
                                r#"{"error":{"kind":"unauthorized","message":"needs NUAGE_ADMIN_TOKEN"}}"#.to_owned(),
                            )
                        } else if line.starts_with("POST /api/apps ") && body.contains("\"taken\"") {
                            ("409 Conflict", r#"{"error":{"kind":"unique_violation","table":"applications","column":"name","message":"there is already an application named `taken`"}}"#.to_owned())
                        } else if line.starts_with("POST /api/apps ") {
                            (
                                "201 Created",
                                format!(
                                    r#"{{"application":{app},"token":"nuage_3f9a12c0secret","env":{{"NUAGE_URL":"{public}","NUAGE_TOKEN":"nuage_3f9a12c0secret"}},"message":"ok"}}"#
                                ),
                            )
                        } else if line.starts_with("GET /api/apps ") {
                            (
                                "200 OK",
                                format!(
                                    r#"{{"applications":[{{"name":"nuage","first":true,"env_token":true,"tokens":[]}},{app}]}}"#
                                ),
                            )
                        } else if line.starts_with("GET /api/apps/shop ") {
                            ("200 OK", app.to_owned())
                        } else if line.starts_with("POST /api/apps/shop/tokens ") {
                            (
                                "201 Created",
                                format!(
                                    r#"{{"application":"shop","token":"nuage_77777777read","read_only":true,"env":{{"NUAGE_URL":"{public}","NUAGE_TOKEN":"nuage_77777777read"}}}}"#
                                ),
                            )
                        } else if line.starts_with("DELETE /api/apps/shop/tokens/3f9a12c0 ") {
                            ("200 OK", r#"{"revoked":true}"#.to_owned())
                        } else {
                            ("404 Not Found", r#"{"error":{"kind":"not_found","message":"no"}}"#.to_owned())
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
        (url, seen)
    }

    #[tokio::test]
    async fn the_management_api() {
        let (url, seen) = server();
        let admin = NuageAdmin::new(&format!("{url}/"), " admin-0123456789abcdef0123456789abcdef ").unwrap();
        assert!(!format!("{admin:?}").contains("admin-0123"), "Debug never shows the token");

        let shop = admin.create_app("shop", "La boutique").await.unwrap();
        assert_eq!(shop.token, "nuage_3f9a12c0secret");
        assert_eq!(shop.url, url);
        assert_eq!(shop.env(), format!("NUAGE_URL={url}\nNUAGE_TOKEN=nuage_3f9a12c0secret"));
        assert_eq!(shop.app.name, "shop");
        assert_eq!(shop.app.tokens, [TokenInfo { id: "3f9a12c0".into(), created_at: 5, read_only: false }]);
        assert!(!format!("{shop:?}").contains("secret"), "Debug never shows the token");
        // The body as JSON: its key order depends on serde_json's `preserve_order`.
        let sent = seen.lock().unwrap()[0].clone();
        assert!(sent.starts_with("POST /api/apps HTTP/1.1 "), "{sent}");
        let body: Json = serde_json::from_str(&sent[sent.find('{').unwrap()..]).unwrap();
        assert_eq!(body, json!({ "name": "shop", "description": "La boutique" }));

        let taken = admin.create_app("taken", "").await.unwrap_err();
        assert!(taken.is_unique_violation(), "{taken:?}");
        assert!(matches!(admin.create_app("Not Valid", "").await, Err(Error::Validation(_))));

        let apps = admin.apps().await.unwrap();
        assert_eq!(apps.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["nuage", "shop"]);
        assert!(apps[0].first && apps[0].env_token);
        assert_eq!(admin.app("shop").await.unwrap().unwrap().description, "La boutique");
        assert_eq!(admin.app("nothing").await.unwrap(), None);

        let reader = admin.new_token("shop", true).await.unwrap();
        assert_eq!((reader.token.as_str(), reader.read_only), ("nuage_77777777read", true));
        assert!(admin.revoke_token("shop", "3f9a12c0").await.unwrap());
        assert!(!admin.revoke_token("shop", "00000000").await.unwrap());

        let wrong = NuageAdmin::new(&url, "an application token").unwrap();
        let refused = wrong.apps().await.unwrap_err();
        assert!(
            matches!(&refused, Error::Database(d) if d.downcast_ref::<wire::NuageError>().is_some_and(wire::NuageError::is_unauthorized)),
            "{refused:?}"
        );
        assert!(NuageAdmin::new("ftp://x", "t").is_err());
    }
}
