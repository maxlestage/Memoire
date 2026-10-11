//! Key-value storages with the semantics of the Web Storage API.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use crate::error::{Error, Result};

/// Called with the key that another tab changed (`None` when it cleared the
/// whole storage).
pub type OnChange = Arc<dyn Fn(Option<&str>) + Send + Sync>;

/// A string key-value storage, like the browser's `localStorage`.
///
/// Implemented by [`LocalStorage`] and [`SessionStorage`] (the browser's, on
/// wasm32) and by [`MemoryStorage`] (everywhere: tests, server rendering).
/// [`Store`](super::Store) and [`LocalStorageDb`](super::LocalStorageDb) work
/// on any implementation.
pub trait KeyValueStorage: Send + Sync + 'static {
    /// Identifies the storage itself: two stores with the same id on storages
    /// with the same scope are the same store.
    fn scope(&self) -> String;

    /// The value of `key`.
    fn get(&self, key: &str) -> Result<Option<String>>;

    /// Stores `value` under `key`. Fails with [`Error::Storage`] when the
    /// storage is full.
    fn set(&self, key: &str, value: &str) -> Result<()>;

    /// Removes `key` (nothing happens if it is absent).
    fn remove(&self, key: &str) -> Result<()>;

    /// Every key, in no particular order.
    fn keys(&self) -> Result<Vec<String>>;

    /// Calls `on_change` when *another* tab (or another handle made with
    /// [`MemoryStorage::tab`]) changes the storage, until the returned
    /// [`Watch`] is dropped. Storages that cannot tell return a `Watch` that
    /// does nothing.
    fn watch(&self, on_change: OnChange) -> Watch {
        let _ = on_change;
        Watch::none()
    }
}

/// Keeps a [`KeyValueStorage::watch`] listener registered; dropping it stops
/// the notifications.
pub struct Watch {
    stop: Option<Box<dyn FnOnce() + Send + Sync>>,
}

impl Watch {
    /// A watch that does nothing.
    pub fn none() -> Self {
        Watch { stop: None }
    }

    /// A watch that runs `stop` when dropped.
    pub fn new(stop: impl FnOnce() + Send + Sync + 'static) -> Self {
        Watch { stop: Some(Box::new(stop)) }
    }
}

impl fmt::Debug for Watch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Watch").field("active", &self.stop.is_some()).finish()
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop();
        }
    }
}

// ---------------------------------------------------------------------------
// The browser's storages
// ---------------------------------------------------------------------------

/// The browser's `localStorage`: kept across reloads and browser restarts,
/// shared by every tab of the same site. On other targets than wasm32 every
/// operation fails with [`Error::Storage`] (use [`MemoryStorage`] there).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocalStorage;

/// The browser's `sessionStorage`: kept across reloads of one tab, dropped when
/// the tab closes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionStorage;

#[cfg(target_arch = "wasm32")]
mod browser {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;

    use super::OnChange;
    use crate::error::{Error, Result};

    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    pub(super) enum Area {
        Local,
        Session,
    }

    pub(super) fn storage(area: Area) -> Result<web_sys::Storage> {
        let window = web_sys::window().ok_or_else(|| Error::Storage("no `window`: not running in a page".into()))?;
        let storage = match area {
            Area::Local => window.local_storage(),
            Area::Session => window.session_storage(),
        };
        storage.map_err(js_error)?.ok_or_else(|| Error::Storage("the storage is disabled by the browser".into()))
    }

    pub(super) fn js_error(e: JsValue) -> Error {
        let name = js_sys::Reflect::get(&e, &"name".into()).ok().and_then(|v| v.as_string()).unwrap_or_default();
        if name == "QuotaExceededError" {
            return Error::Storage("quota exceeded: the browser storage is full".into());
        }
        let message = js_sys::Reflect::get(&e, &"message".into()).ok().and_then(|v| v.as_string());
        Error::Storage(message.or(e.as_string()).unwrap_or_else(|| format!("{name} {e:?}")))
    }

    struct Registry {
        listeners: HashMap<u64, (Area, OnChange)>,
        next: u64,
        installed: bool,
    }

    thread_local! {
        static REGISTRY: RefCell<Registry> =
            RefCell::new(Registry { listeners: HashMap::new(), next: 0, installed: false });
    }

    /// Registers `on_change` for the `storage` events of `area`; one window
    /// listener serves every watch.
    pub(super) fn watch(area: Area, on_change: OnChange) -> Option<u64> {
        let window = web_sys::window()?;
        REGISTRY.with(|r| {
            let mut r = r.borrow_mut();
            if !r.installed {
                let handler = Closure::<dyn Fn(web_sys::StorageEvent)>::new(dispatch);
                window.add_event_listener_with_callback("storage", handler.as_ref().unchecked_ref()).ok()?;
                handler.forget();
                r.installed = true;
            }
            let id = r.next;
            r.next += 1;
            r.listeners.insert(id, (area, on_change));
            Some(id)
        })
    }

    pub(super) fn unwatch(id: u64) {
        REGISTRY.with(|r| r.borrow_mut().listeners.remove(&id));
    }

    fn dispatch(event: web_sys::StorageEvent) {
        let area = match (event.storage_area(), storage(Area::Local).ok(), storage(Area::Session).ok()) {
            (Some(a), Some(local), _) if js_sys::Object::is(&a, &local) => Area::Local,
            (Some(a), _, Some(session)) if js_sys::Object::is(&a, &session) => Area::Session,
            _ => return,
        };
        let key = event.key();
        let targets: Vec<OnChange> = REGISTRY
            .with(|r| r.borrow().listeners.values().filter(|(a, _)| *a == area).map(|(_, f)| f.clone()).collect());
        for f in targets {
            f(key.as_deref());
        }
    }
}

macro_rules! browser_storage {
    ($ty:ident, $area:ident, $scope:literal) => {
        impl KeyValueStorage for $ty {
            fn scope(&self) -> String {
                $scope.to_owned()
            }

            #[cfg(target_arch = "wasm32")]
            fn get(&self, key: &str) -> Result<Option<String>> {
                browser::storage(browser::Area::$area)?.get_item(key).map_err(browser::js_error)
            }

            #[cfg(target_arch = "wasm32")]
            fn set(&self, key: &str, value: &str) -> Result<()> {
                browser::storage(browser::Area::$area)?.set_item(key, value).map_err(browser::js_error)
            }

            #[cfg(target_arch = "wasm32")]
            fn remove(&self, key: &str) -> Result<()> {
                browser::storage(browser::Area::$area)?.remove_item(key).map_err(browser::js_error)
            }

            #[cfg(target_arch = "wasm32")]
            fn keys(&self) -> Result<Vec<String>> {
                let s = browser::storage(browser::Area::$area)?;
                let n = s.length().map_err(browser::js_error)?;
                let mut keys = Vec::with_capacity(n as usize);
                for i in 0..n {
                    if let Some(k) = s.key(i).map_err(browser::js_error)? {
                        keys.push(k);
                    }
                }
                Ok(keys)
            }

            #[cfg(target_arch = "wasm32")]
            fn watch(&self, on_change: OnChange) -> Watch {
                match browser::watch(browser::Area::$area, on_change) {
                    Some(id) => Watch::new(move || browser::unwatch(id)),
                    None => Watch::none(),
                }
            }

            #[cfg(not(target_arch = "wasm32"))]
            fn get(&self, _: &str) -> Result<Option<String>> {
                Err(unavailable())
            }

            #[cfg(not(target_arch = "wasm32"))]
            fn set(&self, _: &str, _: &str) -> Result<()> {
                Err(unavailable())
            }

            #[cfg(not(target_arch = "wasm32"))]
            fn remove(&self, _: &str) -> Result<()> {
                Err(unavailable())
            }

            #[cfg(not(target_arch = "wasm32"))]
            fn keys(&self) -> Result<Vec<String>> {
                Err(unavailable())
            }
        }
    };
}

#[cfg(not(target_arch = "wasm32"))]
fn unavailable() -> Error {
    Error::Storage(
        "the browser storage needs a page: build for wasm32 and run in a browser (or use MemoryStorage)".into(),
    )
}

browser_storage!(LocalStorage, Local, "local");
browser_storage!(SessionStorage, Session, "session");

// ---------------------------------------------------------------------------
// In memory
// ---------------------------------------------------------------------------

static NEXT_MEMORY: AtomicU64 = AtomicU64::new(1);

struct Shared {
    id: u64,
    map: BTreeMap<String, String>,
    /// Maximum size, counted like browsers: UTF-16 code units of keys and values.
    quota: Option<usize>,
    used: usize,
    /// `(tab, listener id, listener)`.
    listeners: Vec<(u64, u64, OnChange)>,
    next_listener: u64,
    next_tab: u64,
}

fn units(s: &str) -> usize {
    s.encode_utf16().count()
}

/// A [`KeyValueStorage`] in memory, behaving like the browser's: the same
/// strings, an optional quota, and [`tab`](Self::tab) handles that see each
/// other's changes as `storage` events. For tests, server-side rendering and
/// native programs.
///
/// Cloning gives another handle on the same storage *in the same tab*.
#[derive(Clone)]
pub struct MemoryStorage {
    shared: Arc<Mutex<Shared>>,
    tab: u64,
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for MemoryStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.lock();
        f.debug_struct("MemoryStorage").field("keys", &s.map.len()).field("tab", &self.tab).finish()
    }
}

impl MemoryStorage {
    /// An empty storage without quota.
    pub fn new() -> Self {
        MemoryStorage {
            shared: Arc::new(Mutex::new(Shared {
                id: NEXT_MEMORY.fetch_add(1, Ordering::Relaxed),
                map: BTreeMap::new(),
                quota: None,
                used: 0,
                listeners: Vec::new(),
                next_listener: 0,
                next_tab: 1,
            })),
            tab: 0,
        }
    }

    /// An empty storage refusing to hold more than `units` UTF-16 code units
    /// of keys and values (browsers allow about 5 million per site).
    pub fn with_quota(units: usize) -> Self {
        let s = Self::new();
        s.lock().quota = Some(units);
        s
    }

    /// Another handle on the same data, as if from another browser tab: the
    /// changes made through one are announced to the watchers of the others.
    pub fn tab(&self) -> MemoryStorage {
        let mut s = self.lock();
        let tab = s.next_tab;
        s.next_tab += 1;
        MemoryStorage { shared: Arc::clone(&self.shared), tab }
    }

    /// The whole content, sorted by key.
    pub fn snapshot(&self) -> BTreeMap<String, String> {
        self.lock().map.clone()
    }

    /// Removes every key (and tells the other tabs).
    pub fn clear(&self) {
        let mut s = self.lock();
        s.map.clear();
        s.used = 0;
        let targets = Self::others(&s, self.tab);
        drop(s);
        for f in targets {
            f(None);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn others(s: &Shared, tab: u64) -> Vec<OnChange> {
        s.listeners.iter().filter(|(t, _, _)| *t != tab).map(|(_, _, f)| Arc::clone(f)).collect()
    }

    fn changed(&self, s: std::sync::MutexGuard<'_, Shared>, key: &str) {
        let targets = Self::others(&s, self.tab);
        drop(s);
        for f in targets {
            f(Some(key));
        }
    }
}

impl KeyValueStorage for MemoryStorage {
    /// Each [`tab`](Self::tab) is its own scope, like separate browser tabs
    /// (each with its own stores).
    fn scope(&self) -> String {
        format!("memory:{}:{}", self.lock().id, self.tab)
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.lock().map.get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        let mut s = self.lock();
        let before = s.map.get(key).map_or(0, |v| units(key) + units(v));
        let after = units(key) + units(value);
        if let Some(q) = s.quota
            && s.used - before + after > q
        {
            return Err(Error::Storage("quota exceeded: the browser storage is full".into()));
        }
        s.used = s.used - before + after;
        s.map.insert(key.to_owned(), value.to_owned());
        self.changed(s, key);
        Ok(())
    }

    fn remove(&self, key: &str) -> Result<()> {
        let mut s = self.lock();
        if let Some(v) = s.map.remove(key) {
            s.used -= units(key) + units(&v);
            self.changed(s, key);
        }
        Ok(())
    }

    fn keys(&self) -> Result<Vec<String>> {
        Ok(self.lock().map.keys().cloned().collect())
    }

    fn watch(&self, on_change: OnChange) -> Watch {
        let mut s = self.lock();
        let id = s.next_listener;
        s.next_listener += 1;
        s.listeners.push((self.tab, id, on_change));
        let weak: Weak<Mutex<Shared>> = Arc::downgrade(&self.shared);
        Watch::new(move || {
            if let Some(shared) = weak.upgrade() {
                shared.lock().unwrap_or_else(|e| e.into_inner()).listeners.retain(|(_, i, _)| *i != id);
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_counts_utf16_units_like_browsers() {
        let s = MemoryStorage::with_quota(10);
        s.set("k", "1234").unwrap(); // 5 units
        s.set("k", "12345678").unwrap(); // replaces: 9 units
        assert!(matches!(s.set("k2", "x").unwrap_err(), Error::Storage(_)));
        assert_eq!(s.get("k").unwrap().as_deref(), Some("12345678"));
        s.remove("k").unwrap();
        s.set("é", "😀😀").unwrap(); // 1 + 4 units
    }

    #[test]
    fn tabs_hear_each_other_but_not_themselves() {
        let a = MemoryStorage::new();
        let b = a.tab();
        let heard = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
        let log = Arc::clone(&heard);
        let _w = b.watch(Arc::new(move |k| log.lock().unwrap().push(k.map(str::to_owned))));
        a.set("x", "1").unwrap();
        b.set("y", "2").unwrap(); // b's own change: not announced to b
        a.remove("x").unwrap();
        a.clear();
        assert_eq!(*heard.lock().unwrap(), vec![Some("x".into()), Some("x".into()), None]);
        assert_eq!(b.get("y").unwrap(), None, "cleared");
    }
}
