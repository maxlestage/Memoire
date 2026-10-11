//! Listeners bound to the thread that registered them, so they may capture
//! values that are not `Send` (the signals of UI frameworks, DOM handles).

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};

/// `owner → [(id, listener)]`, the listeners boxed as `Box<dyn Any>`.
type Registry = HashMap<u64, Vec<(u64, Box<dyn Any>)>>;

/// A listener of table changes that is not `Send`.
pub(crate) type LocalOnChange = std::rc::Rc<dyn Fn(Option<&str>)>;

thread_local! {
    static LISTENERS: RefCell<Registry> = RefCell::new(HashMap::new());
}

static NEXT: AtomicU64 = AtomicU64::new(1);

/// A fresh owner or listener id.
pub(crate) fn next_id() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Registers `listener` for `owner` on this thread.
pub(crate) fn add<T: 'static>(owner: u64, listener: T) -> LocalSubscription {
    let id = next_id();
    LISTENERS.with(|l| l.borrow_mut().entry(owner).or_default().push((id, Box::new(listener))));
    LocalSubscription { owner, id, watch: None, _thread: PhantomData }
}

/// The listeners of type `T` that `owner` has on this thread.
pub(crate) fn listeners<T: Clone + 'static>(owner: u64) -> Vec<T> {
    LISTENERS.with(|l| {
        l.borrow()
            .get(&owner)
            .map_or_else(Vec::new, |v| v.iter().filter_map(|(_, f)| f.downcast_ref::<T>().cloned()).collect())
    })
}

/// Forgets every listener of `owner` on this thread.
pub(crate) fn clear(owner: u64) {
    LISTENERS.with(|l| l.borrow_mut().remove(&owner));
}

/// Keeps a thread-bound listener registered (`subscribe_local`,
/// `on_external_change_local`); dropping it removes the listener. It cannot
/// leave the thread that created it.
pub struct LocalSubscription {
    owner: u64,
    id: u64,
    /// The storage watch feeding this listener, if any.
    watch: Option<super::storage::Watch>,
    _thread: PhantomData<*const ()>,
}

impl LocalSubscription {
    /// Removes the listener now (same as dropping the subscription).
    pub fn unsubscribe(self) {}

    pub(crate) fn with_watch(mut self, watch: super::storage::Watch) -> Self {
        self.watch = Some(watch);
        self
    }
}

impl fmt::Debug for LocalSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalSubscription").field("id", &self.id).finish()
    }
}

impl Drop for LocalSubscription {
    fn drop(&mut self) {
        self.watch.take();
        let (owner, id) = (self.owner, self.id);
        // `try_with`: the thread may be tearing its locals down.
        let _ = LISTENERS.try_with(|l| {
            if let Some(v) = l.borrow_mut().get_mut(&owner) {
                v.retain(|(i, _)| *i != id);
            }
        });
    }
}
