#![cfg_attr(target_arch = "arm", feature(stdarch_arm_feature_detection))]
#![cfg_attr(
    any(target_arch = "riscv32", target_arch = "riscv64"),
    feature(stdarch_riscv_feature_detection)
)]
#![allow(clippy::derivable_impls, clippy::erasing_op, clippy::ptr_eq)]
// The SIMD wrappers name their lane operations add, sub and mul.
#![allow(clippy::should_implement_trait, clippy::items_after_test_module)]
#![allow(
    clippy::borrow_deref_ref,
    clippy::borrowed_box,
    clippy::cast_abs_to_unsigned,
    clippy::clone_on_copy,
    clippy::collapsible_else_if,
    clippy::collapsible_if,
    clippy::doc_overindented_list_items,
    clippy::duplicate_underscore_argument,
    clippy::explicit_auto_deref,
    clippy::identity_op,
    clippy::incompatible_msrv,
    clippy::int_plus_one,
    clippy::into_iter_on_ref,
    clippy::large_const_arrays,
    clippy::large_enum_variant,
    clippy::len_without_is_empty,
    clippy::len_zero,
    clippy::let_and_return,
    clippy::let_underscore_lock,
    clippy::manual_div_ceil,
    clippy::manual_range_contains,
    clippy::manual_saturating_arithmetic,
    clippy::module_inception,
    clippy::misrefactored_assign_op,
    clippy::needless_borrow,
    clippy::needless_late_init,
    clippy::needless_lifetimes,
    clippy::needless_option_as_deref,
    clippy::needless_range_loop,
    clippy::needless_return,
    clippy::neg_multiply,
    clippy::nonminimal_bool,
    clippy::partialeq_to_none,
    clippy::precedence,
    clippy::redundant_closure,
    clippy::redundant_pattern_matching,
    clippy::redundant_static_lifetimes,
    clippy::search_is_some,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::unit_arg,
    clippy::uninlined_format_args,
    clippy::unnecessary_cast,
    clippy::unnecessary_fallible_conversions,
    clippy::unnecessary_map_on_constructor,
    clippy::unnecessary_mut_passed,
    clippy::unnecessary_lazy_evaluations,
    clippy::unneeded_wildcard_pattern,
    clippy::upper_case_acronyms,
    clippy::useless_conversion,
    clippy::zero_prefixed_literal
)]
#![deny(unsafe_op_in_unsafe_fn, clippy::missing_safety_doc)]
//! What the decoder and the encoder share: headers, tables, contexts and the DSP of intra
//! prediction and the inverse transforms. Public for them, not an API.

pub mod align;
pub mod attributes;
pub mod bitdepth;
pub mod cdf;
pub mod const_fn;
pub mod cpu;
pub mod data;
pub mod dump;
pub mod dequant_tables;
pub mod disjoint_mut;
pub mod enum_map;
pub mod env;
pub mod error;
pub mod extensions;
pub mod filter_trials;
pub mod headers;
pub mod in_range;
pub mod intra_edge;
pub mod ipred;
pub mod ipred_prepare;
pub mod iter;
pub mod itx;
pub mod itx_1d;
pub mod itx_simd;
pub mod intops;
pub mod levels;
pub mod msac;
pub mod pic_or_buf;
pub mod picture;
pub mod pixels;
pub mod pool;
pub mod props;
pub mod qm;
pub mod relaxed_atomic;
pub mod scan;
pub mod shared_bytes;
pub mod simd128;
pub mod strided;
pub mod tables;
pub mod validate;
pub mod with_offset;
pub mod wrap_fn_ptr;

use std::sync::Arc;

/// Runs a worker on one of the caller's threads and says whether one took it. Work it does not
/// take is dropped unrun.
pub type Spawner = Arc<dyn Fn(Box<dyn FnOnce() + Send>) -> bool + Send + Sync>;
