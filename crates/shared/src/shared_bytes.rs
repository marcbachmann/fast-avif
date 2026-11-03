use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::NonNull;
use std::slice::SliceIndex;
use std::sync::Arc;

/// Bytes read in place from whoever owns them, shared by the tiles and sliced without touching the
/// owner. The view is stored so that reading it does not go through the [`Arc`].
#[derive(Clone)]
pub struct SharedBytes {
    _owner: Arc<dyn Send + Sync>,
    view: NonNull<[u8]>,
}

// SAFETY: the view only reads what `owner` lends, and `owner` is `Send + Sync`.
unsafe impl Send for SharedBytes {}
// SAFETY: as for `Send`.
unsafe impl Sync for SharedBytes {}

/// The bytes drop on whichever thread holds them last, a frame thread too, so a panic in the
/// owner's `Drop` stays here, as for picture memory.
struct Owner<T>(ManuallyDrop<T>);

impl<T> Drop for Owner<T> {
    fn drop(&mut self) {
        // SAFETY: dropped only here, once.
        let _ = catch_unwind(AssertUnwindSafe(|| unsafe { ManuallyDrop::drop(&mut self.0) }));
    }
}

impl SharedBytes {
    pub fn new<T: AsRef<[u8]> + Send + Sync + 'static>(owner: T) -> Self {
        let owner = Arc::new(Owner(ManuallyDrop::new(owner)));
        let view = NonNull::from((*owner.0).as_ref());
        Self { _owner: owner, view }
    }

    #[inline]
    pub fn slice_in_place<I: SliceIndex<[u8], Output = [u8]>>(&mut self, range: I) {
        self.view = NonNull::from(&self.as_ref()[range]);
    }

    pub fn split_at(this: Self, mid: usize) -> (Self, Self) {
        let mut first = this.clone();
        let mut second = this;
        first.slice_in_place(..mid);
        second.slice_in_place(mid..);
        (first, second)
    }
}

impl AsRef<[u8]> for SharedBytes {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        // SAFETY: `view` lies in what `owner` lent; `owner` sits in the `Arc`, unmoved and borrowed
        // only shared, for as long as this clone holds it.
        unsafe { self.view.as_ref() }
    }
}

impl Deref for SharedBytes {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        self.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PanicsOnDrop;

    impl AsRef<[u8]> for PanicsOnDrop {
        fn as_ref(&self) -> &[u8] {
            &[1, 2, 3]
        }
    }

    impl Drop for PanicsOnDrop {
        fn drop(&mut self) {
            panic!("owner dropped");
        }
    }

    #[test]
    fn a_panicking_owner_drops_quietly() {
        let (first, second) = SharedBytes::split_at(SharedBytes::new(PanicsOnDrop), 1);
        assert_eq!((&*first, &*second), (&[1][..], &[2, 3][..]));
        drop(first);
        drop(second);
    }
}
