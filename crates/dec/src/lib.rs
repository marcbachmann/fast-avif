#![cfg_attr(
    all(target_arch = "aarch64", rav1d_sdot_feature),
    feature(stdarch_neon_dotprod)
)]
#![allow(clippy::derivable_impls, clippy::erasing_op, clippy::ptr_eq)]
#![allow(
    clippy::identity_op,
    clippy::large_const_arrays,
    clippy::large_enum_variant,
    clippy::module_inception,
    clippy::needless_late_init,
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::upper_case_acronyms
)]
#![deny(unsafe_op_in_unsafe_fn, clippy::missing_safety_doc)]

#[cfg(not(any(feature = "bitdepth_8", feature = "bitdepth_16")))]
compile_error!("No bitdepths enabled. Enable one or more of the following features: `bitdepth_8`, `bitdepth_16`");

pub mod settings;

pub use fast_avif_shared::{align, cpu, Spawner};

mod cdef;
mod cdef_apply;
mod ctx;
mod cursor;
pub mod decode;
mod fg_apply;
mod filmgrain;
mod getbits;
mod internal;
mod lf_apply;
mod lf_mask;
pub(crate) mod log;
mod loopfilter;
mod looprestoration;
mod lr_apply;
mod mc;
mod obu;
mod pal;
mod picture;
mod recon;
mod refmvs;
/// This API copies that of `dav1d-rs`, and should function as a drop-in replacement, aside from `dav1d::Error` being renamed to `Rav1dError`
pub mod rust_api;
mod thread_task;
mod warpmv;
mod wedge;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Once};
use std::{cmp, mem, thread};

#[cfg(feature = "bitdepth_16")]
use fast_avif_shared::bitdepth::BitDepth16;
#[cfg(feature = "bitdepth_8")]
use fast_avif_shared::bitdepth::BitDepth8;
use fast_avif_shared::cpu::{rav1d_init_cpu, rav1d_num_logical_processors};
use fast_avif_shared::data::Rav1dData;
pub use fast_avif_shared::error::{Rav1dError, Rav1dResult};
use fast_avif_shared::extensions::OptionError as _;
use fast_avif_shared::in_range::InRange;
use fast_avif_shared::intops::clip;
use fast_avif_shared::iter::wrapping_iter;
use fast_avif_shared::picture::Rav1dPicture;
use fast_avif_shared::sync::Mutex;
use fast_avif_shared::validate::validate_input;
pub use rust_api::*;

use crate::decode::{rav1d_abandon_rows, rav1d_decode_frame_exit};
use crate::internal::{
    Helping, Rav1dBitDepthDSPContext, Rav1dContext, Rav1dContextFrameThread,
    Rav1dContextTaskThread, Rav1dContextTaskType, Rav1dFrameContext, Rav1dState, Rav1dTaskContext,
    Rav1dTaskContextTaskThread, TaskThreadData,
};
use crate::log::Rav1dLog as _;
use crate::obu::rav1d_parse_obus;
use crate::picture::{rav1d_picture_alloc_copy, PictureFlags};
use crate::settings::{
    Rav1dDecodeFrameType, Rav1dInloopFilterType, Rav1dSettings, RAV1D_MAX_THREADS,
};
use crate::thread_task::{
    rav1d_helper_task, rav1d_task_delayed_fg, rav1d_worker_task, FRAME_ERROR,
};

#[cold]
fn init_internal() {
    rav1d_init_cpu();
}

pub const fn rav1d_version() -> &'static str {
    "966d63c1"
}

impl Default for Rav1dSettings {
    fn default() -> Self {
        Self {
            n_threads: InRange::new(0).unwrap(),
            max_frame_delay: InRange::new(0).unwrap(),
            apply_grain: true,
            operating_point: InRange::new(0).unwrap(),
            all_layers: true,
            frame_size_limit: 0,
            allocator: None,
            logger: None,
            strict_std_compliance: false,
            output_invisible_frames: false,
            inloop_filters: Rav1dInloopFilterType::all(),
            decode_frame_type: Rav1dDecodeFrameType::All,
        }
    }
}

struct NumThreads {
    n_tc: u16,
    n_fc: u16,
}

#[cold]
fn get_num_threads(s: &Rav1dSettings) -> NumThreads {
    let n_tc = if s.n_threads.get() != 0 {
        s.n_threads.get() // TODO propagate `InRange`
    } else {
        clip(
            rav1d_num_logical_processors().get(),
            1,
            const { RAV1D_MAX_THREADS as u16 },
        )
    };
    let n_fc = if s.max_frame_delay.get() != 0 {
        cmp::min(s.max_frame_delay.get(), n_tc) // TODO propagate `InRange`
    } else {
        cmp::min((n_tc as f64).sqrt().ceil() as u16, 8)
    };
    NumThreads { n_fc, n_tc }
}

#[cold]
pub(crate) fn rav1d_get_frame_delay(s: &Rav1dSettings) -> u16 {
    get_num_threads(s).n_fc
}

/// Workers handed to `spawner`, up to `most`, until it takes no more, into `taken`. Each is in
/// `taken` before it is handed out, so that one a spawner takes and then unwinds is told to return.
fn spawn_helpers(spawner: &Spawner, ttd: &Arc<TaskThreadData>, most: u16, taken: &mut Starting) {
    for _ in 0..most {
        let thread_data = Arc::new(Rav1dTaskContextTaskThread::new(Arc::clone(ttd)));
        let (helping, theirs) = (Helping::new(ttd), Arc::clone(&thread_data));
        taken.0.push(thread_data);
        if !spawner(Box::new(move || {
            let _helping = helping;
            rav1d_helper_task(theirs);
        })) {
            taken.0.pop();
            break;
        }
    }
}

/// Workers waiting for their context, told to return if it is never made.
struct Starting(Vec<Arc<Rav1dTaskContextTaskThread>>);

impl Drop for Starting {
    fn drop(&mut self) {
        for thread_data in &self.0 {
            let _slot = thread_data.c.lock();
            thread_data.die.set(true);
            thread_data.started.notify_all();
        }
    }
}

#[cold]
pub(crate) fn rav1d_open(
    s: &Rav1dSettings,
    spawner: Option<&Spawner>,
) -> Rav1dResult<Arc<Rav1dContext>> {
    static INITTED: Once = Once::new();
    INITTED.call_once(init_internal);

    // On 32-bit systems, extremely large frame sizes can cause overflows in
    // `rav1d_decode_frame` alloc size calculations. Prevent that from occuring
    // by enforcing a maximum frame size limit, chosen to roughly correspond to
    // the largest size possible to decode without exhausting virtual memory.
    let frame_size_limit;
    if mem::size_of::<usize>() < 8 && s.frame_size_limit.wrapping_sub(1) >= 8192 * 8192 {
        frame_size_limit = 8192 * 8192;
        if s.frame_size_limit != 0 {
            writeln!(
                s.logger,
                "Frame size limit reduced from {} to {}.",
                s.frame_size_limit, frame_size_limit,
            );
        }
    } else {
        frame_size_limit = s.frame_size_limit;
    }

    let NumThreads { n_tc, mut n_fc } = get_num_threads(s);
    if spawner.is_some() {
        n_fc = 1;
    }

    let ttd = TaskThreadData {
        cur: (n_fc as u32).into(),
        reset_task_cur: AtomicU32::new(u32::MAX),
        ..Default::default()
    };
    // TODO fallible allocation
    let task_thread = Arc::new(ttd);
    let mut starting = Starting(Vec::new());
    if let Some(spawner) = spawner.filter(|_| n_tc > 1) {
        spawn_helpers(spawner, &task_thread, n_tc - 1, &mut starting);
    }

    let fc = (0..n_fc)
        .map(|i| {
            let mut fc = Rav1dFrameContext::default(i.into());
            fc.task_thread.finished = AtomicBool::new(true);
            fc.task_thread.ttd = Arc::clone(&task_thread);
            let f = fc.data.get_mut();
            f.lf.last_sharpness = u8::MAX;
            fc
        })
        // TODO fallible allocation
        .collect();

    let state = Mutex::new(Rav1dState {
        frame_thread: Rav1dContextFrameThread {
            out_delayed: if n_fc > 1 {
                (0..n_fc).map(|_| Default::default()).collect()
            } else {
                Box::new([])
            },
            ..Default::default()
        },
        ..Default::default()
    });

    let tc = if spawner.is_some() {
        let caller = Arc::new(Rav1dTaskContextTaskThread::new(Arc::clone(&task_thread)));
        caller.flushed.set(true);
        let task = Mutex::new(Box::new(Rav1dTaskContext::new(Arc::clone(&caller))));
        let caller = Rav1dContextTaskThread {
            task: match starting.0.is_empty() {
                true => Rav1dContextTaskType::Single(task),
                false => Rav1dContextTaskType::Caller(task),
            },
            thread_data: caller,
        };
        starting
            .0
            .iter()
            .map(|thread_data| Rav1dContextTaskThread {
                task: Rav1dContextTaskType::Helper,
                thread_data: Arc::clone(thread_data),
            })
            .chain([caller])
            .collect()
    } else {
        (0..n_tc)
            .map(|n| {
                let task_thread = Arc::clone(&task_thread);
                let thread_data = Arc::new(Rav1dTaskContextTaskThread::new(task_thread));
                let thread_data_copy = Arc::clone(&thread_data);
                let task = if n_tc > 1 {
                    // Counted as a helper, so that closing waits until it let go of the context.
                    let helping = Helping::new(&thread_data.ttd);
                    let handle = thread::Builder::new()
                        // Don't set stack size like `dav1d` does.
                        // See <https://github.com/memorysafety/rav1d/issues/889>.
                        .name(format!("fast-avif-worker-{n}"))
                        .spawn(move || {
                            let _helping = helping;
                            rav1d_worker_task(thread_data_copy)
                        })
                        .unwrap();
                    Rav1dContextTaskType::Worker(handle)
                } else {
                    Rav1dContextTaskType::Single(Mutex::new(Box::new(Rav1dTaskContext::new(
                        thread_data_copy,
                    ))))
                };
                Rav1dContextTaskThread { task, thread_data }
            })
            // TODO fallible allocation
            .collect()
    };

    let c = Rav1dContext {
        allocator: s.allocator.clone().unwrap_or_default(),
        logger: s.logger.clone(),
        apply_grain: s.apply_grain,
        operating_point: s.operating_point.get(), // TODO propagate `InRange`
        all_layers: s.all_layers,
        frame_size_limit,
        strict_std_compliance: s.strict_std_compliance,
        output_invisible_frames: s.output_invisible_frames,
        inloop_filters: s.inloop_filters,
        decode_frame_type: s.decode_frame_type,
        fc,
        task_thread,
        state,
        tc,
        ..Default::default()
    };

    // TODO fallible allocation
    let c = Arc::new(c);

    for tc in c.tc.iter() {
        if let Rav1dContextTaskType::Worker(handle) = &tc.task {
            // Unpark each thread once we set its `thread_data.c`.
            *tc.thread_data.c.lock() = Some(Arc::clone(&c));
            handle.thread().unpark();
        }
    }
    for thread_data in mem::take(&mut starting.0) {
        *thread_data.c.lock() = Some(Arc::clone(&c));
        thread_data.started.notify_one();
    }

    Ok(c)
}

fn output_image(c: &Rav1dContext, state: &mut Rav1dState, out: &mut Rav1dPicture) -> Rav1dResult {
    let mut res = Ok(());

    let use_cache = !c.all_layers && state.max_spatial_id != 0;
    let r#in = if !use_cache {
        &mut state.out
    } else {
        &mut state.cache
    };
    if !c.apply_grain || !r#in.p.has_grain() {
        *out = mem::take(&mut r#in.p);
    } else {
        res = rav1d_apply_grain(c, out, &r#in.p);
    }
    let _ = mem::take(r#in);

    if use_cache && state.out.p.data.is_some() {
        state.cache = mem::take(&mut state.out);
    }
    res
}

fn output_picture_ready(c: &Rav1dContext, state: &mut Rav1dState, drain: bool) -> bool {
    if state.rows.is_some() {
        return false;
    }
    if state.cached_error.is_some() {
        return true;
    }
    if !c.all_layers && state.max_spatial_id != 0 {
        if state.out.p.data.is_some() && state.cache.p.data.is_some() {
            if state.max_spatial_id == state.cache.p.frame_hdr.as_ref().unwrap().spatial_id
                || state.out.flags.contains(PictureFlags::NEW_TEMPORAL_UNIT)
            {
                return true;
            }
            state.cache = mem::take(&mut state.out);
            return false;
        } else {
            if state.cache.p.data.is_some() && drain {
                return true;
            } else {
                if state.out.p.data.is_some() {
                    state.cache = mem::take(&mut state.out);
                    return false;
                }
            }
        }
    }
    state.out.p.data.is_some()
}

fn drain_picture(c: &Rav1dContext, state: &mut Rav1dState, out: &mut Rav1dPicture) -> Rav1dResult {
    let mut drained = false;
    for _ in 0..c.fc.len() {
        let next = state.frame_thread.next;
        let fc = &c.fc[next as usize];
        let mut task_thread_lock = c.task_thread.lock.lock();
        while !fc.task_thread.finished.load(Ordering::SeqCst) {
            fc.task_thread.cond.wait(&mut task_thread_lock);
        }
        let out_delayed = &mut state.frame_thread.out_delayed[next as usize];
        if out_delayed.p.data.is_some() || fc.task_thread.error.load(Ordering::SeqCst) != 0 {
            let first = c.task_thread.first.load(Ordering::SeqCst);
            if first as usize + 1 < c.fc.len() {
                c.task_thread.first.fetch_add(1, Ordering::SeqCst);
            } else {
                c.task_thread.first.store(0, Ordering::SeqCst);
            }
            let _ = c.task_thread.reset_task_cur.compare_exchange(
                first,
                u32::MAX,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
            let cur = c.task_thread.cur.get();
            if cur != 0 && (cur as usize) < c.fc.len() {
                c.task_thread.cur.set(cur - 1);
            }
            drained = true;
        } else if drained {
            break;
        }
        state.frame_thread.next = (state.frame_thread.next + 1) % c.fc.len() as u32;
        drop(task_thread_lock);
        mem::take(&mut *fc.task_thread.retval.try_lock().unwrap())
            .err_or(())
            .inspect_err(|_| {
                state.cached_error_props = out_delayed.p.m.clone();
                let _ = mem::take(out_delayed);
            })?;
        if out_delayed.p.data.is_some() {
            let progress = out_delayed.progress.as_ref().unwrap()[1].load(Ordering::Relaxed);
            if (out_delayed.visible || c.output_invisible_frames) && progress != FRAME_ERROR {
                state.out = out_delayed.clone();
                state.event_flags |= out_delayed.flags.into();
            }
            let _ = mem::take(out_delayed);
            if output_picture_ready(c, state, false) {
                return output_image(c, state, out);
            }
        }
    }
    if output_picture_ready(c, state, true) {
        return output_image(c, state, out);
    }
    Err(Rav1dError::TryAgain)
}

fn gen_picture(c: &Rav1dContext, state: &mut Rav1dState) -> Rav1dResult {
    if state.rows.is_some() || output_picture_ready(c, state, false) {
        return Ok(());
    }
    // Take so we don't have 2 `&mut`s.
    let Rav1dData {
        data: r#in,
        m: props,
    } = mem::take(&mut state.in_0);
    let Some(mut r#in) = r#in else { return Ok(()) };
    while !r#in.is_empty() {
        let len = rav1d_parse_obus(c, state, &r#in, &props);
        if let Ok(len) = len {
            r#in.slice_in_place(len..);
        }
        // Note that [`output_picture_ready`] doesn't read [`Rav1dContext::in_0`].
        if state.rows.is_some() || output_picture_ready(c, state, false) {
            // Restore into `c` when there's still data left.
            if !r#in.is_empty() {
                state.in_0 = Rav1dData {
                    data: Some(r#in),
                    m: props,
                }
            }
            break;
        }
        len?;
    }
    Ok(())
}

pub(crate) fn rav1d_send_data(c: &Rav1dContext, in_0: &mut Rav1dData) -> Rav1dResult {
    let state = &mut *c.state.try_lock().unwrap();
    if let Some(data) = in_0.data.as_ref() {
        let sz = data.len();
        validate_input!((sz > 0 && sz <= usize::MAX / 2, Rav1dError::InvalidArgument))?;
        state.drain = false;
    }
    if state.in_0.data.is_some() {
        return Err(Rav1dError::TryAgain);
    }
    state.in_0 = in_0.clone();
    let res = gen_picture(c, state);
    if res.is_ok() {
        let _ = mem::take(in_0);
    }
    res
}

pub(crate) fn rav1d_get_picture(c: &Rav1dContext, out: &mut Rav1dPicture) -> Rav1dResult {
    let state = &mut *c.state.try_lock().unwrap();
    let drain = mem::replace(&mut state.drain, true);
    gen_picture(c, state)?;
    mem::take(&mut state.cached_error).err_or(())?;
    if output_picture_ready(c, state, c.fc.len() == 1) {
        return output_image(c, state, out);
    }
    if c.fc.len() > 1 && drain {
        return drain_picture(c, state, out);
    }
    Err(Rav1dError::TryAgain)
}

pub(crate) fn rav1d_apply_grain(
    c: &Rav1dContext,
    out: &mut Rav1dPicture,
    in_0: &Rav1dPicture,
) -> Rav1dResult {
    if !in_0.has_grain() {
        *out = in_0.clone();
        return Ok(());
    }
    let res = rav1d_picture_alloc_copy(&c.logger, out, in_0.p.w, in_0);
    if res.is_err() {
        let _ = mem::take(out);
        res
    } else {
        if c.tc.len() > 1 {
            rav1d_task_delayed_fg(c, out, in_0);
        } else {
            let bpc = out.p.bpc;
            let dsp = Rav1dBitDepthDSPContext::get(bpc).unwrap();
            let fg = &dsp.fg;
            use fg_apply::rav1d_apply_grain;
            match bpc {
                #[cfg(feature = "bitdepth_8")]
                8 => rav1d_apply_grain::<BitDepth8>(fg, out, in_0),
                #[cfg(feature = "bitdepth_16")]
                10 | 12 => rav1d_apply_grain::<BitDepth16>(fg, out, in_0),
                _ => {}
            }
        }
        Ok(())
    }
}

pub(crate) fn rav1d_flush(c: &Rav1dContext) {
    let state = &mut *c.state.try_lock().unwrap();

    rav1d_abandon_rows(c, state);
    let old_state = mem::take(state);
    state.by_rows = old_state.by_rows;
    state.rows_spawner = old_state.rows_spawner;
    state.rows_contexts = old_state.rows_contexts;
    state.tiles = old_state.tiles;
    state.n_tiles = old_state.n_tiles;
    state.frame_thread = old_state.frame_thread;
    state.operating_point_idc = old_state.operating_point_idc;
    state.max_spatial_id = old_state.max_spatial_id;
    state.frame_flags = old_state.frame_flags;
    state.event_flags = old_state.event_flags;

    if c.fc.len() == 1 && c.tc.len() == 1 {
        return;
    }
    c.flush.store(true, Ordering::SeqCst);
    if c.tc.len() > 1 {
        let mut task_thread_lock = c.task_thread.lock.lock();
        for tc in c.tc.iter() {
            while !tc.flushed() {
                tc.thread_data.cond.wait(&mut task_thread_lock);
            }
        }
        for fc in c.fc.iter() {
            fc.task_thread.tasks.clear();
        }
        c.task_thread.first.store(0, Ordering::SeqCst);
        c.task_thread.cur.set(c.fc.len() as u32);
        c.task_thread
            .reset_task_cur
            .store(u32::MAX, Ordering::SeqCst);
        c.task_thread.cond_signaled.store(0, Ordering::SeqCst);
    }
    if c.fc.len() > 1 {
        for fc in wrapping_iter(c.fc.iter(), state.frame_thread.next as usize) {
            let _ = rav1d_decode_frame_exit(c, fc, Err(Rav1dError::Other));
            *fc.task_thread.retval.try_lock().unwrap() = None;
            let out_delayed = &mut state.frame_thread.out_delayed[fc.index];
            if out_delayed.p.frame_hdr.is_some() {
                let _ = mem::take(out_delayed);
            }
        }
        state.frame_thread.next = 0;
    }
    c.flush.store(false, Ordering::SeqCst);
}

#[cold]
pub(crate) fn rav1d_close(c: &Rav1dContext) {
    rav1d_flush(c);
    c.tell_worker_threads_to_die();
    let ttd = &*c.task_thread;
    let mut task_thread_lock = ttd.lock.lock();
    while ttd.helpers.load(Ordering::SeqCst) > 0 {
        ttd.gone.wait(&mut task_thread_lock);
    }
}

impl Rav1dContext {
    fn tell_worker_threads_to_die(&self) {
        if self.tc.is_empty() {
            return;
        }
        let ttd = &*self.task_thread;
        let _task_thread_lock = ttd.lock.lock();
        for tc in self.tc.iter() {
            tc.thread_data.die.set(true);
        }
        ttd.cond.notify_all();
    }
}
