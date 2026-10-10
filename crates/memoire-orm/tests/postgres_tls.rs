#![cfg(feature = "postgres-tls")]
//! TLS modes. Runs when `MEMOIRE_PG_URL` points to a server with SSL enabled
//! (its certificate is usually self-signed, so `verify-full` must fail).

use memoire_orm::prelude::*;

fn url(mode: &str) -> Option<String> {
    let base = std::env::var("MEMOIRE_PG_URL").ok().filter(|s| !s.is_empty())?;
    let sep = if base.contains('?') { '&' } else { '?' };
    Some(format!("{base}{sep}sslmode={mode}"))
}

async fn is_ssl(db: &PgPool) -> bool {
    let rows: Vec<bool> = db.raw("SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()", &[]).await.unwrap();
    rows.first().copied().unwrap_or(false)
}

#[tokio::test]
async fn tls_modes() {
    let Some(require) = url("require") else {
        eprintln!("MEMOIRE_PG_URL not set: skipping");
        return;
    };
    let server_has_ssl = match PgPool::connect_with(&require, 1).await {
        Ok(db) => {
            assert!(is_ssl(&db).await, "sslmode=require must encrypt");
            true
        }
        Err(e) => {
            eprintln!("server without SSL ({e}): skipping");
            false
        }
    };
    if !server_has_ssl {
        return;
    }
    let prefer = PgPool::connect_with(&url("prefer").unwrap(), 1).await.unwrap();
    assert!(is_ssl(&prefer).await, "sslmode=prefer uses TLS when available");
    let plain = PgPool::connect_with(&url("disable").unwrap(), 1).await.unwrap();
    assert!(!is_ssl(&plain).await, "sslmode=disable stays in plain text");
    // A self-signed certificate is rejected when verification is requested.
    assert!(PgPool::connect_with(&url("verify-full").unwrap(), 1).await.is_err());
}
