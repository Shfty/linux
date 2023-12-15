use std::{
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

/// Wrapper type for thread-safe change tracking
#[derive(Debug, Default)]
pub struct AtomicChanged<T> {
    value: T,
    changed: AtomicBool,
}

impl<T> From<T> for AtomicChanged<T> {
    fn from(value: T) -> Self {
        AtomicChanged {
            value,
            changed: AtomicBool::new(true),
        }
    }
}

impl<T> AtomicChanged<T> {
    /// Run the provided function if the value has changed, then reset the changed flag
    pub fn if_changed<R>(&self, f: impl FnOnce(&T) -> R, ordering: Ordering) -> Option<R> {
        if self.changed.load(ordering) {
            let out = f(&self.value);
            self.changed.store(false, ordering);
            Some(out)
        } else {
            None
        }
    }

    /// Change the underlying value via immutable reference
    pub fn change(&self, f: impl FnOnce(&T) -> bool, ordering: Ordering) {
        let changed = f(&self.value);
        if changed {
            self.changed.store(true, ordering);
        }
    }

    /// Change the underlying value via mutable reference
    pub fn change_mut<R>(&mut self, f: impl FnOnce(&mut T) -> R, ordering: Ordering) -> R {
        let out = f(&mut self.value);
        self.changed.store(true, ordering);
        out
    }
}

impl<T> Deref for AtomicChanged<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T> DerefMut for AtomicChanged<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}
