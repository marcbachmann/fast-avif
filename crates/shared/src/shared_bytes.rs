use std::ops::Deref;
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

impl SharedBytes {
    pub fn new<T: AsRef<[u8]> + Send + Sync + 'static>(owner: T) -> Self {
        let owner = Arc::new(owner);
        let view = NonNull::from((*owner).as_ref());
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
