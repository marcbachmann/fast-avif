
#[inline]
pub fn ctz(mask: u32) -> i32 {
    mask.trailing_zeros() as i32
}

#[inline]
pub fn clz(mask: u32) -> i32 {
    mask.leading_zeros() as i32
}

#[inline]
pub fn clzll(mask: u64) -> i32 {
    mask.leading_zeros() as i32
}
