use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// A shared vector that returns its elements in rotation.
///
/// Clones share the elements and cursor. This type is `Send` and `Sync` when
/// `T` is `Send + Sync`.
pub struct RotatingVec<T> {
    inner: Arc<RotatingVecInner<T>>,
}

struct RotatingVecInner<T> {
    values: Vec<T>,
    index: AtomicUsize,
}

impl<T> Clone for RotatingVec<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> RotatingVec<T> {
    pub fn new(values: Vec<T>) -> Self {
        Self {
            inner: Arc::new(RotatingVecInner {
                values,
                index: AtomicUsize::new(0),
            }),
        }
    }

    /// Returns the next element, starting at index zero, or `None` when empty.
    pub fn get(&self) -> Option<&T> {
        let len = self.inner.values.len();
        if len == 0 {
            return None;
        }

        let index = self
            .inner
            .index
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |index| {
                Some(match index.checked_add(1) {
                    Some(next) if next < len => next,
                    _ => 0,
                })
            })
            .ok()?;

        self.inner.values.get(index)
    }
}
