//! The native transport of Nuage: HTTP/1.1 over TCP (`http://`) or rustls
//! (`https://`), with kept-alive connections reused between requests.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::client::conn::http1::{self, SendRequest};
use hyper::{Request, header};
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use super::{Endpoint, HttpResponse};
use crate::error::{Error, Result};

/// Idle connections kept per client: enough for the usual burst of
/// concurrent requests, few enough to stay friendly with hosted endpoints.
const MAX_IDLE: usize = 16;

pub(crate) struct Transport {
    endpoint: Endpoint,
    tls: Option<TlsConnector>,
    idle: Mutex<Vec<SendRequest<Full<Bytes>>>>,
    timeout: Duration,
}

fn transport_err(endpoint: &Endpoint, e: impl std::fmt::Display) -> Error {
    Error::Pool(format!("cannot reach the Nuage DB server {}: {e}", endpoint.authority()))
}

impl Transport {
    pub(crate) fn new(endpoint: Endpoint, timeout: Duration) -> Result<Self> {
        let tls = if endpoint.https {
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
            let mut config = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map_err(|e| Error::Pool(format!("TLS configuration: {e}")))?
                .with_root_certificates(roots)
                .with_no_client_auth();
            config.alpn_protocols = vec![b"http/1.1".to_vec()];
            Some(TlsConnector::from(Arc::new(config)))
        } else {
            None
        };
        Ok(Transport { endpoint, tls, idle: Mutex::new(Vec::new()), timeout })
    }

    async fn open(&self) -> Result<SendRequest<Full<Bytes>>> {
        let tcp = TcpStream::connect((self.endpoint.host.as_str(), self.endpoint.port))
            .await
            .map_err(|e| transport_err(&self.endpoint, e))?;
        let _ = tcp.set_nodelay(true);
        let sender = match &self.tls {
            Some(tls) => {
                let name = rustls::pki_types::ServerName::try_from(self.endpoint.host.clone())
                    .map_err(|e| transport_err(&self.endpoint, e))?;
                let stream = tls.connect(name, tcp).await.map_err(|e| transport_err(&self.endpoint, e))?;
                let (sender, conn) =
                    http1::handshake(TokioIo::new(stream)).await.map_err(|e| transport_err(&self.endpoint, e))?;
                tokio::spawn(async move {
                    let _ = conn.await;
                });
                sender
            }
            None => {
                let (sender, conn) =
                    http1::handshake(TokioIo::new(tcp)).await.map_err(|e| transport_err(&self.endpoint, e))?;
                tokio::spawn(async move {
                    let _ = conn.await;
                });
                sender
            }
        };
        Ok(sender)
    }

    fn take_idle(&self) -> Option<SendRequest<Full<Bytes>>> {
        let mut idle = self.idle.lock().unwrap_or_else(|e| e.into_inner());
        while let Some(sender) = idle.pop() {
            if !sender.is_closed() {
                return Some(sender);
            }
        }
        None
    }

    fn give_back(&self, sender: SendRequest<Full<Bytes>>) {
        if sender.is_closed() {
            return;
        }
        let mut idle = self.idle.lock().unwrap_or_else(|e| e.into_inner());
        if idle.len() < MAX_IDLE {
            idle.push(sender);
        }
    }

    pub(crate) async fn request(
        &self,
        method: &'static str,
        path: &str,
        headers: &[(String, String)],
        body: Vec<u8>,
    ) -> Result<HttpResponse> {
        tokio::time::timeout(self.timeout, self.send(method, path, headers, body))
            .await
            .map_err(|_| transport_err(&self.endpoint, format!("no answer within {:?}", self.timeout)))?
    }

    async fn send(
        &self,
        method: &'static str,
        path: &str,
        headers: &[(String, String)],
        body: Vec<u8>,
    ) -> Result<HttpResponse> {
        let body = Bytes::from(body);
        // A kept-alive connection may have been closed by the server in the
        // meantime: when it fails before the request was sent, a new
        // connection is opened. A request that may have reached the server is
        // never sent twice.
        let mut sender = match self.take_idle() {
            Some(mut sender) => match sender.ready().await {
                Ok(()) => sender,
                Err(_) => self.open().await?,
            },
            None => self.open().await?,
        };
        sender.ready().await.map_err(|e| transport_err(&self.endpoint, e))?;
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, self.endpoint.authority())
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json");
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        let request = request.body(Full::new(body)).map_err(|e| transport_err(&self.endpoint, e))?;
        let response = sender.send_request(request).await.map_err(|e| transport_err(&self.endpoint, e))?;
        let status = response.status().as_u16();
        let body = response.into_body().collect().await.map_err(|e| transport_err(&self.endpoint, e))?.to_bytes();
        self.give_back(sender);
        Ok(HttpResponse { status, body: body.to_vec() })
    }
}
