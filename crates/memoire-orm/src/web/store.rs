//! [`Store`]: application state kept in browser storage, in the spirit of Pinia.

use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock};

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::local::LocalSubscription;
use super::storage::{KeyValueStorage, LocalStorage, Watch};
use crate::error::{Error, Result};

/// The state a [`Store`] can hold: any `serde` type.
pub trait StoreState: Serialize + DeserializeOwned + Clone + Send + Sync + 'static {}
impl<S: Serialize + DeserializeOwned + Clone + Send + Sync + 'static> StoreState for S {}

type Listener<S> = Arc<dyn Fn(&S) + Send + Sync>;
type LocalListener<S> = Rc<dyn Fn(&S)>;
type Migration = Box<dyn Fn(u32, serde_json::Value) -> serde_json::Value + Send + Sync>;

struct Inner<S> {
    /// Identifies the store among the thread-bound listeners.
    uid: u64,
    id: String,
    key: String,
    storage: Arc<dyn KeyValueStorage>,
    init: Box<dyn Fn() -> S + Send + Sync>,
    version: u32,
    migrate: Option<Migration>,
    state: RwLock<S>,
    listeners: Mutex<Vec<(u64, Listener<S>)>>,
    next_listener: AtomicU64,
    last_error: Mutex<Option<String>>,
    watch: Mutex<Option<Watch>>,
}

/// Every store, by storage scope and key: `Store::new("cart")` anywhere in the
/// program returns the same store, like Pinia's `useStore()`. A store lives
/// until [`Store::dispose`], with its subscribers.
static REGISTRY: LazyLock<Mutex<HashMap<String, Arc<dyn Any + Send + Sync>>>> = LazyLock::new(Default::default);

/// Application state kept in browser storage, in the spirit of
/// [Pinia](https://pinia.vuejs.org/): one store per id, read through getters,
/// changed through actions, saved on every change, restored on the next visit,
/// and kept in step across the tabs of the site.
///
/// ```
/// use memoire_orm::web::{MemoryStorage, Store};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Clone, Default, Serialize, Deserialize)]
/// struct Cart {
///     items: Vec<(String, u32)>,
/// }
///
/// impl Cart {
///     fn count(&self) -> u32 { // a getter
///         self.items.iter().map(|(_, n)| n).sum()
///     }
/// }
///
/// // `Store::new("cart")` in a browser; any `KeyValueStorage` elsewhere.
/// let storage = MemoryStorage::new();
/// let cart: Store<Cart> = Store::builder("cart").storage(storage.clone()).build();
/// let _sub = cart.subscribe(|c| println!("{} item(s)", c.count()));
///
/// cart.update(|c| c.items.push(("book".into(), 2))); // an action: saved, then announced
/// assert_eq!(cart.get(Cart::count), 2);
///
/// // Next visit: the state comes back.
/// cart.dispose();
/// let again: Store<Cart> = Store::builder("cart").storage(storage).build();
/// assert_eq!(again.get(Cart::count), 2);
/// ```
///
/// Saving is best-effort, like Pinia's persistence plugins: if the storage
/// refuses a write (full, disabled), the state still changes in memory and
/// [`last_error`](Self::last_error) tells why; [`save`](Self::save) retries
/// and reports.
pub struct Store<S> {
    inner: Arc<Inner<S>>,
}

impl<S> Clone for Store<S> {
    fn clone(&self) -> Self {
        Store { inner: Arc::clone(&self.inner) }
    }
}

impl<S: fmt::Debug> fmt::Debug for Store<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.inner.state.read().unwrap_or_else(|e| e.into_inner());
        f.debug_struct("Store").field("id", &self.inner.id).field("state", &*state).finish()
    }
}

/// Options of a [`Store`], from [`Store::builder`].
pub struct StoreBuilder<S> {
    id: String,
    key: Option<String>,
    storage: Arc<dyn KeyValueStorage>,
    init: Option<Box<dyn Fn() -> S + Send + Sync>>,
    version: u32,
    migrate: Option<Migration>,
}

impl<S: StoreState> StoreBuilder<S> {
    /// Where the state is kept (browser `localStorage` by default).
    pub fn storage(mut self, storage: impl KeyValueStorage) -> Self {
        self.storage = Arc::new(storage);
        self
    }

    /// The storage key (`memoire:store:<id>` by default).
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// The initial state, also used by [`Store::reset`] (`S::default()` by
    /// default).
    pub fn init(mut self, init: impl Fn() -> S + Send + Sync + 'static) -> Self {
        self.init = Some(Box::new(init));
        self
    }

    /// Version of the state's shape. A state saved by an older version goes
    /// through `migrate(saved_version, json)` before being read, so a changed
    /// struct does not lose what visitors already have.
    pub fn version(
        mut self,
        version: u32,
        migrate: impl Fn(u32, serde_json::Value) -> serde_json::Value + Send + Sync + 'static,
    ) -> Self {
        self.version = version;
        self.migrate = Some(Box::new(migrate));
        self
    }

    /// The store: the one already created for this id and storage (the other
    /// options are then ignored), or a new one loaded from the storage. It
    /// stays alive, with its subscribers, until [`Store::dispose`].
    ///
    /// # Panics
    ///
    /// If a store with the same id and storage is alive with another state type.
    pub fn build(self) -> Store<S> {
        let key = self.key.unwrap_or_else(|| format!("memoire:store:{}", self.id));
        let slot = format!("{}\u{0}{key}", self.storage.scope());
        let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(alive) = registry.get(&slot).cloned() {
            match alive.downcast::<Inner<S>>() {
                Ok(inner) => return Store { inner },
                Err(_) => panic!("the store `{}` already exists with another state type", self.id),
            }
        }
        let init = self.init.unwrap_or_else(|| Box::new(|| panic!("no initial state")));
        let inner = Arc::new(Inner {
            uid: super::local::next_id(),
            state: RwLock::new(init()),
            id: self.id,
            key,
            storage: self.storage,
            init,
            version: self.version,
            migrate: self.migrate,
            listeners: Mutex::new(Vec::new()),
            next_listener: AtomicU64::new(0),
            last_error: Mutex::new(None),
            watch: Mutex::new(None),
        });
        let store = Store { inner };
        if let Err(e) = store.load() {
            store.fail(e);
        }
        // Another tab saved this store: take its state.
        let weak = Arc::downgrade(&store.inner);
        let key = store.inner.key.clone();
        let watch = store.inner.storage.watch(Arc::new(move |changed: Option<&str>| {
            if changed.is_none_or(|k| k == key)
                && let Some(inner) = weak.upgrade()
            {
                let store = Store { inner };
                if let Err(e) = store.load() {
                    store.fail(e);
                }
                store.notify();
            }
        }));
        *store.inner.watch.lock().unwrap_or_else(|e| e.into_inner()) = Some(watch);
        let erased: Arc<dyn Any + Send + Sync> = store.inner.clone();
        registry.insert(slot, erased);
        store
    }
}

impl<S: StoreState + Default> Store<S> {
    /// The store `id`, kept in the browser's `localStorage` under
    /// `memoire:store:<id>`, starting from `S::default()`. Calling it again
    /// with the same id, anywhere, returns the same store: there is no need to
    /// keep the handle.
    pub fn new(id: impl Into<String>) -> Store<S> {
        Store::builder(id).build()
    }
}

impl<S: StoreState> Store<S> {
    /// A store with options: storage, key, initial state, version.
    pub fn builder(id: impl Into<String>) -> StoreBuilder<S>
    where
        S: Default,
    {
        StoreBuilder {
            id: id.into(),
            key: None,
            storage: Arc::new(LocalStorage),
            init: Some(Box::new(S::default)),
            version: 0,
            migrate: None,
        }
    }

    /// [`builder`](Self::builder) for a state without `Default`: `init` gives
    /// the initial state.
    pub fn builder_with(id: impl Into<String>, init: impl Fn() -> S + Send + Sync + 'static) -> StoreBuilder<S> {
        StoreBuilder {
            id: id.into(),
            key: None,
            storage: Arc::new(LocalStorage),
            init: Some(Box::new(init)),
            version: 0,
            migrate: None,
        }
    }

    /// The store id.
    pub fn id(&self) -> &str {
        &self.inner.id
    }

    /// The storage key holding the state.
    pub fn key(&self) -> &str {
        &self.inner.key
    }

    /// Reads the state, like a Pinia getter: `cart.get(|c| c.items.len())`,
    /// or `cart.get(Cart::total)` for a method.
    pub fn get<R>(&self, read: impl FnOnce(&S) -> R) -> R {
        read(&self.inner.state.read().unwrap_or_else(|e| e.into_inner()))
    }

    /// A copy of the whole state (Pinia's `$state`).
    pub fn state(&self) -> S {
        self.get(S::clone)
    }

    /// Changes the state, like a Pinia action: `f` gets it mutably, then the
    /// new state is saved and announced to the subscribers. Returns what `f`
    /// returns.
    pub fn update<R>(&self, f: impl FnOnce(&mut S) -> R) -> R {
        let result = {
            let mut state = self.inner.state.write().unwrap_or_else(|e| e.into_inner());
            let r = f(&mut state);
            // Saved under the lock, so saves land in the order of the changes.
            if let Err(e) = self.write(&state) {
                self.fail(e);
            }
            r
        };
        self.notify();
        result
    }

    /// Replaces the whole state.
    pub fn set(&self, state: S) {
        self.update(|s| *s = state);
    }

    /// Back to the initial state (Pinia's `$reset`).
    pub fn reset(&self) {
        let fresh = (self.inner.init)();
        self.set(fresh);
    }

    /// Changes some fields at once from a JSON object merged into the state
    /// (Pinia's `$patch`, RFC 7396: `null` removes a field, objects merge,
    /// anything else replaces). Fails, leaving the state as it was, if the
    /// result is not a valid state.
    pub fn patch(&self, partial: serde_json::Value) -> Result<()> {
        let current = self.get(|s| serde_json::to_value(s)).map_err(Error::conversion)?;
        let mut merged = current;
        merge_patch(&mut merged, partial);
        let next: S = serde_json::from_value(merged).map_err(Error::conversion)?;
        self.set(next);
        Ok(())
    }

    /// Calls `listener` with the new state after every change, including the
    /// changes made in another tab, until the returned subscription is
    /// dropped (Pinia's `$subscribe`).
    #[must_use = "the listener is removed when the subscription is dropped"]
    pub fn subscribe(&self, listener: impl Fn(&S) + Send + Sync + 'static) -> StoreSubscription {
        let id = self.inner.next_listener.fetch_add(1, Ordering::Relaxed);
        self.inner.listeners.lock().unwrap_or_else(|e| e.into_inner()).push((id, Arc::new(listener)));
        let weak = Arc::downgrade(&self.inner);
        StoreSubscription {
            stop: Some(Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    inner.listeners.lock().unwrap_or_else(|e| e.into_inner()).retain(|(i, _)| *i != id);
                }
            })),
        }
    }

    /// [`subscribe`](Self::subscribe) for a listener that is not `Send`, such
    /// as one updating a signal of a UI framework (an Active `State`, a Yew or
    /// Dioxus handle): it runs on the thread that registered it, the only one
    /// in a browser.
    ///
    /// ```ignore
    /// let count = use_state(cart.get(Cart::count)); // Active
    /// let _sub = cart.subscribe_local(move |c| count.set(c.count()));
    /// ```
    #[must_use = "the listener is removed when the subscription is dropped"]
    pub fn subscribe_local(&self, listener: impl Fn(&S) + 'static) -> LocalSubscription {
        let listener: LocalListener<S> = Rc::new(listener);
        super::local::add(self.inner.uid, listener)
    }

    /// Writes the state to the storage now and reports a failure.
    pub fn save(&self) -> Result<()> {
        let state = self.inner.state.read().unwrap_or_else(|e| e.into_inner());
        let r = self.write(&state);
        *self.inner.last_error.lock().unwrap_or_else(|e| e.into_inner()) = r.as_ref().err().map(ToString::to_string);
        r
    }

    /// Reads the state back from the storage (it is read once when the store
    /// is created, and again when another tab changes it).
    pub fn reload(&self) -> Result<()> {
        self.load()?;
        self.notify();
        Ok(())
    }

    /// Back to the initial state and removes the saved copy.
    pub fn clear(&self) -> Result<()> {
        {
            let mut state = self.inner.state.write().unwrap_or_else(|e| e.into_inner());
            *state = (self.inner.init)();
            self.inner.storage.remove(&self.inner.key)?;
        }
        self.notify();
        Ok(())
    }

    /// Forgets this store: its subscribers stop hearing changes, and the next
    /// `Store::new` with the same id reads the storage again (Pinia's
    /// `$dispose`). The saved state is kept.
    pub fn dispose(self) {
        let slot = format!("{}\u{0}{}", self.inner.storage.scope(), self.inner.key);
        let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        if registry.get(&slot).is_some_and(|alive| std::ptr::addr_eq(Arc::as_ptr(alive), Arc::as_ptr(&self.inner))) {
            registry.remove(&slot);
        }
        drop(registry);
        self.inner.listeners.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.inner.watch.lock().unwrap_or_else(|e| e.into_inner()).take();
        super::local::clear(self.inner.uid);
    }

    /// Why the last save or load failed, if it did.
    pub fn last_error(&self) -> Option<String> {
        self.inner.last_error.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn fail(&self, e: Error) {
        *self.inner.last_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(e.to_string());
    }

    /// Saved form: `{"v": <version>, "state": <state>}`.
    fn write(&self, state: &S) -> Result<()> {
        let json = serde_json::json!({ "v": self.inner.version, "state": state });
        self.inner.storage.set(&self.inner.key, &json.to_string())?;
        *self.inner.last_error.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(())
    }

    /// Reads the saved state; the initial state when there is none. A saved
    /// state that cannot be read keeps the current one and is reported.
    fn load(&self) -> Result<()> {
        let Some(text) = self.inner.storage.get(&self.inner.key)? else {
            // Nothing saved (or removed by another tab): the initial state.
            *self.inner.state.write().unwrap_or_else(|e| e.into_inner()) = (self.inner.init)();
            return Ok(());
        };
        let saved: serde_json::Value = serde_json::from_str(&text).map_err(Error::conversion)?;
        let (version, mut state) = match saved {
            serde_json::Value::Object(mut o) if o.contains_key("state") => {
                let v = o.get("v").and_then(serde_json::Value::as_u64).unwrap_or(0) as u32;
                (v, o.remove("state").unwrap_or_default())
            }
            other => (0, other),
        };
        if version < self.inner.version
            && let Some(migrate) = &self.inner.migrate
        {
            state = migrate(version, state);
        }
        let state: S = serde_json::from_value(state).map_err(|e| {
            Error::Conversion(format!("the saved state of store `{}` does not fit its type: {e}", self.inner.id))
        })?;
        *self.inner.state.write().unwrap_or_else(|e| e.into_inner()) = state;
        Ok(())
    }

    fn notify(&self) {
        let listeners: Vec<Listener<S>> =
            self.inner.listeners.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|(_, l)| Arc::clone(l)).collect();
        let local: Vec<LocalListener<S>> = super::local::listeners(self.inner.uid);
        if listeners.is_empty() && local.is_empty() {
            return;
        }
        // A copy, so listeners may read or change the store themselves.
        let state = self.state();
        for l in listeners {
            l(&state);
        }
        for l in local {
            l(&state);
        }
    }
}

/// Keeps a [`Store::subscribe`] listener registered; dropping it unsubscribes.
pub struct StoreSubscription {
    stop: Option<Box<dyn FnOnce() + Send + Sync>>,
}

impl StoreSubscription {
    /// Removes the listener now (same as dropping the subscription).
    pub fn unsubscribe(self) {}
}

impl fmt::Debug for StoreSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StoreSubscription")
    }
}

impl Drop for StoreSubscription {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop();
        }
    }
}

/// JSON Merge Patch (RFC 7396).
fn merge_patch(target: &mut serde_json::Value, patch: serde_json::Value) {
    use serde_json::Value as J;
    match patch {
        J::Object(fields) => {
            if !target.is_object() {
                *target = J::Object(Default::default());
            }
            if let J::Object(t) = target {
                for (k, v) in fields {
                    if v.is_null() {
                        t.remove(&k);
                    } else {
                        merge_patch(t.entry(k).or_insert(J::Null), v);
                    }
                }
            }
        }
        other => *target = other,
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::web::MemoryStorage;

    #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
    struct Cart {
        items: Vec<String>,
        coupon: Option<String>,
    }

    fn cart(storage: &MemoryStorage) -> Store<Cart> {
        Store::builder("cart").storage(storage.clone()).build()
    }

    #[test]
    fn one_store_per_id_and_storage() {
        let s = MemoryStorage::new();
        let a = cart(&s);
        let b = cart(&s);
        a.update(|c| c.items.push("book".into()));
        assert_eq!(b.get(|c| c.items.len()), 1, "the same store");
        let other: Store<Cart> = Store::builder("cart").storage(MemoryStorage::new()).build();
        assert_eq!(other.get(|c| c.items.len()), 0, "another storage, another store");
    }

    #[test]
    fn state_is_saved_and_restored() {
        let s = MemoryStorage::new();
        {
            let c = cart(&s);
            c.update(|c| c.items.push("pen".into()));
            c.patch(serde_json::json!({ "coupon": "WELCOME" })).unwrap();
            c.dispose(); // a new visit reads the storage again
        }
        let saved: serde_json::Value = serde_json::from_str(&s.get("memoire:store:cart").unwrap().unwrap()).unwrap();
        assert_eq!(saved, serde_json::json!({ "v": 0, "state": { "coupon": "WELCOME", "items": ["pen"] } }));
        let c = cart(&s);
        assert_eq!(c.state(), Cart { items: vec!["pen".into()], coupon: Some("WELCOME".into()) });
        c.patch(serde_json::json!({ "coupon": null })).unwrap();
        assert_eq!(c.get(|c| c.coupon.clone()), None);
        assert!(c.patch(serde_json::json!({ "items": 3 })).is_err(), "not a valid state");
        assert_eq!(c.get(|c| c.items.len()), 1, "unchanged");
        c.reset();
        assert_eq!(c.state(), Cart::default());
        c.update(|c| c.items.push("x".into()));
        c.clear().unwrap();
        assert_eq!(s.get("memoire:store:cart").unwrap(), None);
    }

    #[test]
    fn subscribers_hear_every_change_and_other_tabs() {
        let s = MemoryStorage::new();
        let c = cart(&s);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let sub = c.subscribe(move |c| log.lock().unwrap().push(c.items.len()));
        c.update(|c| c.items.push("a".into()));
        c.set(Cart { items: vec!["a".into(), "b".into()], coupon: None });

        // Another tab writes the same key: this tab reloads and announces it.
        let other_tab = s.tab();
        other_tab.set("memoire:store:cart", r#"{"v":0,"state":{"items":["a","b","c"],"coupon":null}}"#).unwrap();
        assert_eq!(c.get(|c| c.items.len()), 3);
        other_tab.set("unrelated", "1").unwrap();
        assert_eq!(*seen.lock().unwrap(), vec![1, 2, 3]);

        drop(sub);
        c.update(|c| c.items.clear());
        assert_eq!(seen.lock().unwrap().len(), 3);
    }

    #[test]
    fn local_listeners_capture_values_that_are_not_send() {
        let s = MemoryStorage::new();
        let c = cart(&s);
        let seen = Rc::new(std::cell::Cell::new(0usize)); // not Send
        let log = Rc::clone(&seen);
        let sub = c.subscribe_local(move |c| log.set(c.items.len()));
        c.update(|c| c.items.push("a".into()));
        assert_eq!(seen.get(), 1);
        s.tab().set("memoire:store:cart", r#"{"v":0,"state":{"items":["a","b"],"coupon":null}}"#).unwrap();
        assert_eq!(seen.get(), 2, "other tabs too");
        drop(sub);
        c.update(|c| c.items.clear());
        assert_eq!(seen.get(), 2);
    }

    #[test]
    fn listeners_may_change_the_store() {
        let s = MemoryStorage::new();
        let c = cart(&s);
        let inner = c.clone();
        let _sub = c.subscribe(move |state| {
            if state.items.len() > 2 {
                inner.update(|c| c.items.truncate(2)); // re-entrant
            }
        });
        c.set(Cart { items: vec!["1".into(), "2".into(), "3".into()], coupon: None });
        assert_eq!(c.get(|c| c.items.len()), 2);
    }

    #[test]
    fn a_full_storage_keeps_the_change_in_memory_and_reports_it() {
        let s = MemoryStorage::with_quota(100);
        let c = cart(&s);
        c.update(|c| c.items.push("small".into()));
        assert_eq!(c.last_error(), None);
        c.update(|c| c.items.push("x".repeat(100)));
        assert_eq!(c.get(|c| c.items.len()), 2, "still changed in memory");
        assert!(c.last_error().unwrap().contains("quota"));
        assert!(c.save().is_err());
        c.update(|c| c.items.truncate(1));
        assert_eq!(c.last_error(), None, "saved again");
    }

    #[test]
    fn old_states_are_migrated() {
        #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
        struct Prefs {
            theme: String,
            font_size: u32,
        }
        let s = MemoryStorage::new();
        // Version 0 saved `{"dark": true}`.
        s.set("memoire:store:prefs", r#"{"v":0,"state":{"dark":true}}"#).unwrap();
        let prefs: Store<Prefs> = Store::builder("prefs")
            .storage(s.clone())
            .version(1, |from, old| {
                assert_eq!(from, 0);
                let dark = old["dark"].as_bool().unwrap_or(false);
                serde_json::json!({ "theme": if dark { "dark" } else { "light" }, "font_size": 16 })
            })
            .build();
        assert_eq!(prefs.state(), Prefs { theme: "dark".into(), font_size: 16 });
        prefs.update(|p| p.font_size = 18);
        assert!(s.get("memoire:store:prefs").unwrap().unwrap().contains(r#""v":1"#));
    }

    #[test]
    fn unreadable_saves_fall_back_to_the_initial_state() {
        let s = MemoryStorage::new();
        s.set("memoire:store:cart", "not json").unwrap();
        let c = cart(&s);
        assert_eq!(c.state(), Cart::default());
        assert!(c.last_error().is_some());
        let custom: Store<Vec<u8>> = Store::builder_with("bytes", || vec![1, 2]).storage(s.clone()).build();
        assert_eq!(custom.state(), vec![1, 2]);
    }

    #[test]
    #[should_panic(expected = "another state type")]
    fn one_id_one_type() {
        let s = MemoryStorage::new();
        let _a = cart(&s);
        let _b: Store<Vec<u8>> = Store::builder("cart").storage(s.clone()).build();
    }
}
