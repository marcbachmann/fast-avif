use crate::attributes::{clz, clzll};
use crate::bitdepth::{AsPrimitive, ToPrimitive};

/// # Safety
///
/// `U: Into<T>` and `T: TryInto<U>` must be well-formed
/// such that for all `u: U`, `u.into().try_into() == Ok(u)`.
#[inline]
pub fn clip<T, U>(v: T, min: U, max: U) -> U
where
    T: Copy + Ord + TryInto<U> + ToPrimitive<U>,
    U: Copy + Ord + Into<T>,
{
    debug_assert!(min <= max);
    if v < min.into() {
        min
    } else if v > max.into() {
        max
    } else {
        // Note that `v.try_into().unwrap()` is not always optimized out.
        // We use `.as_()`, a truncating cast, here instead of
        // `v.try_into().unwrap()`, which doesn't always optimized out,
        // or `unsafe { v.try_into().unwrap_unchecked() }`, which is unsafe
        // and depends on correct `{Try,}{From,Into}` `impl`s
        // and `assert!(min <= max)`, which may not always get optimized out.
        v.as_()
    }
}

#[inline]
pub fn clip_u8<T>(v: T) -> u8
where
    T: Copy + Ord + From<u8> + TryInto<u8> + ToPrimitive<u8>,
{
    clip(v, u8::MIN, u8::MAX)
}

#[inline]
pub fn iclip(v: i32, min: i32, max: i32) -> i32 {
    clip(v, min, max)
}

#[inline]
pub fn apply_sign(v: i32, s: i32) -> i32 {
    if s < 0 {
        -v
    } else {
        v
    }
}

#[inline]
pub fn apply_sign64(v: i32, s: i64) -> i32 {
    if s < 0 {
        -v
    } else {
        v
    }
}

#[inline]
pub fn ulog2(v: u32) -> i32 {
    return 31 - clz(v);
}

#[inline]
pub fn u64log2(v: u64) -> i32 {
    return 63 - clzll(v as u64);
}

#[inline]
pub fn inv_recenter(r: u32, v: u32) -> u32 {
    if v > r << 1 {
        v
    } else if v & 1 == 0 {
        (v >> 1).wrapping_add(r)
    } else {
        r.wrapping_sub(v.wrapping_add(1) >> 1)
    }
}
