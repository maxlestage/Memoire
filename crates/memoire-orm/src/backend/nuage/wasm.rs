//! The WebAssembly transport of Nuage: the `fetch` function of the JavaScript
//! host (a browser page, a web worker, an edge function...).
//!
//! JavaScript futures cannot leave their thread, while Memoire's executors
//! return `Send` futures. The request therefore runs in a local task
//! (`spawn_local`) and hands its plain-Rust result over through a one-shot
//! slot, which is `Send`.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use js_sys::{Function, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, Request, RequestInit, Response};

use super::{Endpoint, HttpResponse};
use crate::error::{Error, Result};

pub(crate) struct Transport {
    endpoint: Endpoint,
}

fn js_err(e: JsValue) -> Error {
    let message = e.as_string().or_else(|| e.dyn_ref::<js_sys::Error>().map(|e| String::from(e.message())));
    Error::Pool(format!("fetch failed: {}", message.unwrap_or_else(|| format!("{e:?}"))))
}

async fn fetch(
    method: &'static str,
    url: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
) -> Result<HttpResponse> {
    let init = RequestInit::new();
    init.set_method(method);
    let h = Headers::new().map_err(js_err)?;
    h.set("Content-Type", "application/json").map_err(js_err)?;
    for (name, value) in &headers {
        h.set(name, value).map_err(js_err)?;
    }
    init.set_headers(&h);
    if !body.is_empty() {
        init.set_body(&Uint8Array::from(body.as_slice()));
    }
    let request = Request::new_with_str_and_init(&url, &init).map_err(js_err)?;

    // `fetch` lives on `window` in a page and on `self` in workers and edge
    // runtimes: read it from the global object, whatever it is.
    let global = js_sys::global();
    let fetch: Function = Reflect::get(&global, &JsValue::from_str("fetch"))
        .map_err(js_err)?
        .dyn_into()
        .map_err(|_| Error::Unsupported("this JavaScript host has no `fetch` function".into()))?;
    let promise: js_sys::Promise = fetch.call1(&global, &request).map_err(js_err)?.dyn_into().map_err(js_err)?;
    let response: Response = JsFuture::from(promise).await.map_err(js_err)?.dyn_into().map_err(js_err)?;

    let status = response.status();
    let buffer = JsFuture::from(response.array_buffer().map_err(js_err)?).await.map_err(js_err)?;
    let body = Uint8Array::new(&buffer).to_vec();
    Ok(HttpResponse { status, body })
}

/// The result of the local task, and the waker of whoever waits for it.
struct Slot {
    result: Option<Result<HttpResponse>>,
    waker: Option<Waker>,
}

struct Pending(Arc<Mutex<Slot>>);

impl Future for Pending {
    type Output = Result<HttpResponse>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        match slot.result.take() {
            Some(result) => Poll::Ready(result),
            None => {
                slot.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl Transport {
    pub(crate) fn new(endpoint: Endpoint, _timeout: std::time::Duration) -> Result<Self> {
        Ok(Transport { endpoint })
    }

    pub(crate) async fn request(
        &self,
        method: &'static str,
        path: &str,
        headers: &[(String, String)],
        body: Vec<u8>,
    ) -> Result<HttpResponse> {
        let url =
            format!("{}://{}{path}", if self.endpoint.https { "https" } else { "http" }, self.endpoint.authority());
        let slot = Arc::new(Mutex::new(Slot { result: None, waker: None }));
        let task_slot = slot.clone();
        let headers = headers.to_vec();
        wasm_bindgen_futures::spawn_local(async move {
            let result = fetch(method, url, headers, body).await;
            let mut slot = task_slot.lock().unwrap_or_else(|e| e.into_inner());
            slot.result = Some(result);
            if let Some(waker) = slot.waker.take() {
                waker.wake();
            }
        });
        Pending(slot).await
    }
}
