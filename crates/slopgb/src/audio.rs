//! Audio output: a cpal stream at the device's default config fed from a
//! lock-free queue of stereo frames, plus a streaming linear resampler to
//! bridge the core's APU sample rate to the device rate.
//!
//! The audio callback only pops from the queue; on underrun it outputs
//! silence. The emulation side pushes; pushes beyond the queue capacity
//! (~250 ms) are dropped, which doubles as the discard policy during turbo.
//!
//! Realtime discipline: the queue is a single-producer/single-consumer ring
//! (see [`SpscRing`]). The realtime callback (sole consumer) never takes a
//! lock and never allocates — it pops into a scratch buffer preallocated to
//! the device's max buffer size and converts samples in place — so it can't
//! be blocked or stalled by the emulation thread (sole producer). Stream
//! errors set a flag the emulation thread polls via [`AudioOutput::failed`].

use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};

/// Callback scratch size when the host won't report a max buffer size.
const DEFAULT_SCRATCH_FRAMES: usize = 4096;

// ---------------------------------------------------------------------------
// Lock-free SPSC ring

/// Pad a counter to its own cache line so the producer's `write` and the
/// consumer's `read` don't false-share.
#[repr(align(64))]
struct CacheLine<T>(T);

impl<T> Deref for CacheLine<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

#[inline]
fn pack(l: f32, r: f32) -> u64 {
    (u64::from(l.to_bits()) << 32) | u64::from(r.to_bits())
}

#[inline]
fn unpack(v: u64) -> (f32, f32) {
    #[allow(clippy::cast_possible_truncation)]
    (f32::from_bits((v >> 32) as u32), f32::from_bits(v as u32))
}

/// Lock-free single-producer / single-consumer ring of stereo frames shared
/// between the emulation thread (sole producer: [`SpscRing::push`]) and the
/// cpal realtime callback (sole consumer: [`SpscRing::pop`]).
///
/// Each slot packs one `(f32, f32)` frame into an `AtomicU64` (left in the
/// high 32 bits, right in the low 32), so neither `unsafe` nor a `Mutex` is
/// needed and the callback never blocks on the producer.
///
/// Memory ordering: `write`/`read` are monotonically increasing frame counters
/// (the slot index is `counter & mask`, hence the power-of-two capacity).
/// The producer writes the slots, then publishes them with a `Release` store
/// of `write`; the consumer reads `write` with `Acquire`, so it is guaranteed
/// to see those slot writes. The symmetric handshake on `read` lets the
/// producer see the consumer free up space. Slot loads/stores stay `Relaxed`
/// because the index handshake already orders them (and atomics never race).
struct SpscRing {
    slots: Box<[AtomicU64]>,
    /// `capacity - 1`; capacity is a power of two so `& mask` wraps the index.
    mask: usize,
    /// Producer-owned. Total frames ever pushed.
    write: CacheLine<AtomicUsize>,
    /// Consumer-owned. Total frames ever popped.
    read: CacheLine<AtomicUsize>,
}

impl SpscRing {
    /// Build a ring holding at least `min_frames` (rounded up to a power of
    /// two for cheap index masking).
    fn with_min_capacity(min_frames: usize) -> Self {
        let cap = min_frames.max(1).next_power_of_two();
        let slots = (0..cap)
            .map(|_| AtomicU64::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            slots,
            mask: cap - 1,
            write: CacheLine(AtomicUsize::new(0)),
            read: CacheLine(AtomicUsize::new(0)),
        }
    }

    fn capacity(&self) -> usize {
        self.mask + 1
    }

    /// Approximate number of queued frames, for monitoring/pacing only. Both
    /// counters are loaded `Relaxed`, so under concurrent push/pop the result
    /// is a slightly stale estimate; it is never used to bound push/pop.
    fn len(&self) -> usize {
        let w = self.write.load(Ordering::Relaxed);
        let r = self.read.load(Ordering::Relaxed);
        w.wrapping_sub(r)
    }

    /// Producer only. Push as many frames as fit; returns the count pushed.
    /// Anything beyond the free space is dropped (turbo/overflow discard).
    fn push(&self, frames: &[(f32, f32)]) -> usize {
        // Producer owns `write`; only the consumer's progress needs syncing.
        let write = self.write.load(Ordering::Relaxed);
        let read = self.read.load(Ordering::Acquire);
        let free = self.capacity() - write.wrapping_sub(read);
        let n = frames.len().min(free);
        for (i, &(l, r)) in frames[..n].iter().enumerate() {
            self.slots[write.wrapping_add(i) & self.mask].store(pack(l, r), Ordering::Relaxed);
        }
        // Release publishes the slot writes above to the consumer's Acquire.
        self.write.store(write.wrapping_add(n), Ordering::Release);
        n
    }

    /// Consumer only. Pop up to `max` frames, appending them to `out` (not
    /// cleared). Returns the count popped (0 on empty; never blocks).
    fn pop(&self, out: &mut Vec<(f32, f32)>, max: usize) -> usize {
        // Consumer owns `read`; Acquire on `write` to see the producer's data.
        let read = self.read.load(Ordering::Relaxed);
        let write = self.write.load(Ordering::Acquire);
        let n = write.wrapping_sub(read).min(max);
        for i in 0..n {
            let v = self.slots[read.wrapping_add(i) & self.mask].load(Ordering::Relaxed);
            out.push(unpack(v));
        }
        // Release publishes the freed space to the producer's Acquire.
        self.read.store(read.wrapping_add(n), Ordering::Release);
        n
    }
}

// ---------------------------------------------------------------------------
// Output stream

pub struct AudioOutput {
    /// Held to keep the stream playing; dropped on shutdown.
    _stream: cpal::Stream,
    ring: Arc<SpscRing>,
    sample_rate: u32,
    /// Set by the stream error callback; see [`Self::failed`].
    failed: Arc<AtomicBool>,
}

impl AudioOutput {
    /// Open the default output device at its default stream config.
    pub fn new() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no audio output device")?;
        let supported = device
            .default_output_config()
            .map_err(|e| format!("no default stream config: {e}"))?;
        let sample_rate = supported.sample_rate().0;
        let channels = usize::from(supported.channels());
        if sample_rate == 0 || channels == 0 {
            return Err("output device reports an unusable default config".into());
        }
        // ~250 ms cap (rounded up to a power of two) so a stalled callback
        // can't grow the queue forever.
        let capacity = sample_rate as usize / 4;
        let ring = Arc::new(SpscRing::with_min_capacity(capacity));
        // Size the callback scratch from the device's max buffer so the
        // realtime callback never allocates; cap it at the ring capacity and
        // fall back when the host won't report a size.
        let scratch_frames = match supported.buffer_size() {
            cpal::SupportedBufferSize::Range { max, .. } => {
                (*max as usize).clamp(1, ring.capacity())
            }
            cpal::SupportedBufferSize::Unknown => DEFAULT_SCRATCH_FRAMES,
        };
        let failed = Arc::new(AtomicBool::new(false));
        let config = supported.config();
        let stream = build_for_format(
            supported.sample_format(),
            &device,
            &config,
            channels,
            Arc::clone(&ring),
            &failed,
            scratch_frames,
        )
        .map_err(|e| format!("cannot build output stream: {e}"))?;
        stream
            .play()
            .map_err(|e| format!("cannot start output stream: {e}"))?;
        Ok(Self {
            _stream: stream,
            ring,
            sample_rate,
            failed,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Stereo frames currently queued for playback (approximate; see
    /// [`SpscRing::len`]). Fine for pacing and the stall watchdog.
    pub fn queued(&self) -> usize {
        self.ring.len()
    }

    /// Queue stereo frames; anything beyond the capacity is dropped.
    pub fn push(&self, samples: &[(f32, f32)]) {
        self.ring.push(samples);
    }

    /// True once the stream has reported an error (device unplugged, backend
    /// died). The stream is dead; the owner should stop pacing on it.
    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }
}

/// Dispatch on the device's sample format, building the stream with the
/// matching sample type. Every integer and float format cpal exposes is
/// handled via `FromSample<f32>`, so devices that negotiate e.g. I32 or U8
/// still get audio instead of falling back to timer pacing.
#[allow(clippy::too_many_arguments)]
fn build_for_format(
    format: cpal::SampleFormat,
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    ring: Arc<SpscRing>,
    failed: &Arc<AtomicBool>,
    scratch_frames: usize,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    use cpal::SampleFormat as F;
    macro_rules! build {
        ($t:ty) => {
            build_stream::<$t>(device, config, channels, ring, failed, scratch_frames)
        };
    }
    match format {
        F::I8 => build!(i8),
        F::I16 => build!(i16),
        F::I24 => build!(cpal::I24),
        F::I32 => build!(i32),
        F::I64 => build!(i64),
        F::U8 => build!(u8),
        F::U16 => build!(u16),
        F::U32 => build!(u32),
        F::U64 => build!(u64),
        F::F32 => build!(f32),
        F::F64 => build!(f64),
        // `SampleFormat` is `#[non_exhaustive]`: a future variant we can't map.
        other => Err(cpal::BuildStreamError::StreamConfigNotSupported)
            .inspect_err(|_| eprintln!("slopgb: unsupported sample format {other}")),
    }
}

/// Fill one device buffer from the ring. Pops at most `data.len() / channels`
/// frames (never more than `scratch` can hold) into `scratch`, then fans them
/// out across the device channels; any shortfall is silence. Runs entirely in
/// the realtime callback: no lock, no allocation.
fn fill_from_ring<T>(
    data: &mut [T],
    channels: usize,
    ring: &SpscRing,
    scratch: &mut Vec<(f32, f32)>,
) where
    T: SizedSample + FromSample<f32>,
{
    let frames = data.len() / channels;
    scratch.clear();
    // Never grow `scratch`: cap the drain at its preallocated capacity. Frames
    // the device wants beyond that become silence this callback — which does
    // not happen once `scratch` is sized to the device's max buffer.
    let want = frames.min(scratch.capacity());
    ring.pop(scratch, want);
    for (i, frame) in data.chunks_mut(channels).enumerate() {
        let (l, r) = scratch.get(i).copied().unwrap_or((0.0, 0.0));
        match frame {
            [mono] => *mono = T::from_sample(0.5 * (l + r)),
            [fl, fr, rest @ ..] => {
                *fl = T::from_sample(l);
                *fr = T::from_sample(r);
                for s in rest {
                    *s = T::from_sample(0.0f32);
                }
            }
            [] => {}
        }
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    ring: Arc<SpscRing>,
    failed: &Arc<AtomicBool>,
    scratch_frames: usize,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample + FromSample<f32>,
{
    // Sized to the device's max buffer so the callback never reallocates.
    let mut scratch: Vec<(f32, f32)> = Vec::with_capacity(scratch_frames);
    let failed = Arc::clone(failed);
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            fill_from_ring(data, channels, &ring, &mut scratch);
        },
        move |err| {
            eprintln!("slopgb: audio stream error: {err}");
            failed.store(true, Ordering::Relaxed);
        },
        None,
    )
}

// ---------------------------------------------------------------------------
// Resampler

/// Streaming linear resampler for stereo frames. Pass-through when the rates
/// match (the expected case once the core's sample rate can be set to the
/// device rate).
///
/// Tradeoff: linear interpolation with no low-pass on downsample, so high
/// frequencies alias when `dst_rate < src_rate` — acceptable for an emulator.
pub struct Resampler {
    src_rate: u32,
    dst_rate: u32,
    /// Position of the next output sample, in source samples, relative to
    /// `prev` (so `prev` is at 0.0 and the next input sample is at 1.0).
    pos: f64,
    /// Previous source frame the next output interpolates from. Seeded from
    /// the first real input frame (see `run`) so the first output equals it
    /// instead of lerping up from silence (a click).
    prev: (f32, f32),
    /// False until `prev` has been seeded from the first input frame.
    primed: bool,
}

impl Resampler {
    pub fn new(src_rate: u32, dst_rate: u32) -> Self {
        Self {
            src_rate,
            dst_rate,
            pos: 0.0,
            prev: (0.0, 0.0),
            primed: false,
        }
    }

    /// Convert `input` (source rate) and append the result to `out`
    /// (destination rate). State carries across calls.
    pub fn run(&mut self, input: &[(f32, f32)], out: &mut Vec<(f32, f32)>) {
        if self.src_rate == self.dst_rate {
            out.extend_from_slice(input);
            return;
        }
        // Seed `prev` from the very first frame ever fed, so the first output
        // sample equals the first input sample rather than starting at 0.
        if !self.primed {
            if let Some(&first) = input.first() {
                self.prev = first;
                self.primed = true;
            }
        }
        let step = f64::from(self.src_rate) / f64::from(self.dst_rate);
        for &cur in input {
            while self.pos < 1.0 {
                #[allow(clippy::cast_possible_truncation)]
                let t = self.pos as f32;
                out.push((
                    self.prev.0 + (cur.0 - self.prev.0) * t,
                    self.prev.1 + (cur.1 - self.prev.1) * t,
                ));
                self.pos += step;
            }
            self.pos -= 1.0;
            self.prev = cur;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring_with(frames: &[(f32, f32)]) -> Arc<SpscRing> {
        let ring = Arc::new(SpscRing::with_min_capacity(frames.len().max(1)));
        ring.push(frames);
        ring
    }

    fn pop_all(ring: &SpscRing) -> Vec<(f32, f32)> {
        let mut out = Vec::new();
        ring.pop(&mut out, ring.len());
        out
    }

    // --- SPSC ring ---------------------------------------------------------

    #[test]
    fn ring_push_pop_preserves_order_and_values() {
        let ring = SpscRing::with_min_capacity(16);
        let input: Vec<(f32, f32)> = (0..10).map(|i| (i as f32, -(i as f32))).collect();
        assert_eq!(ring.push(&input), input.len());
        let mut out = Vec::new();
        assert_eq!(ring.pop(&mut out, input.len()), input.len());
        assert_eq!(out, input);
    }

    #[test]
    fn ring_wraps_around_capacity_boundary() {
        // Capacity 4, but push/pop 3 per round so the masked index repeatedly
        // straddles the wrap point.
        let ring = SpscRing::with_min_capacity(4);
        assert_eq!(ring.capacity(), 4);
        let mut expected = 0u32;
        for round in 0..10u32 {
            let batch: Vec<(f32, f32)> = (0..3)
                .map(|k| {
                    let v = (round * 3 + k) as f32;
                    (v, -v)
                })
                .collect();
            assert_eq!(ring.push(&batch), 3);
            let mut out = Vec::new();
            assert_eq!(ring.pop(&mut out, 3), 3);
            for (l, r) in out {
                let v = expected as f32;
                assert_eq!((l, r), (v, -v));
                expected += 1;
            }
        }
    }

    #[test]
    fn ring_push_drops_overflow_when_full() {
        let ring = SpscRing::with_min_capacity(4); // capacity 4
        let input: Vec<(f32, f32)> = (0..10).map(|i| (i as f32, 0.0)).collect();
        // Only the first 4 fit; the rest are dropped.
        assert_eq!(ring.push(&input), 4);
        assert_eq!(ring.push(&[(99.0, 0.0)]), 0);
        let out = pop_all(&ring);
        assert_eq!(out, &input[..4]);
    }

    #[test]
    fn ring_pop_on_empty_returns_zero_without_blocking() {
        let ring = SpscRing::with_min_capacity(8);
        let mut out = Vec::new();
        assert_eq!(ring.pop(&mut out, 4), 0);
        assert!(out.is_empty());
    }

    #[test]
    fn ring_concurrent_push_pop_loses_nothing() {
        use std::thread;
        const N: u32 = 200_000;
        let ring = Arc::new(SpscRing::with_min_capacity(64));
        let producer = {
            let ring = Arc::clone(&ring);
            thread::spawn(move || {
                let mut i = 0u32;
                while i < N {
                    // Retry on a full ring so nothing is dropped for this test.
                    if ring.push(&[(i as f32, -(i as f32))]) == 1 {
                        i += 1;
                    } else {
                        std::hint::spin_loop();
                    }
                }
            })
        };
        let mut got = Vec::with_capacity(1);
        let mut next = 0u32;
        while next < N {
            got.clear();
            if ring.pop(&mut got, 1) == 1 {
                // Exact sequence: proves no loss, duplication, or reorder.
                assert_eq!(got[0], (next as f32, -(next as f32)));
                next += 1;
            } else {
                std::hint::spin_loop();
            }
        }
        producer.join().unwrap();
    }

    // --- callback fill -----------------------------------------------------

    #[test]
    fn fill_drains_stereo_frames_in_order() {
        let ring = ring_with(&[(0.1, 0.2), (0.3, 0.4)]);
        let mut data = [9.0f32; 4];
        let mut scratch = Vec::with_capacity(8);
        fill_from_ring(&mut data, 2, &ring, &mut scratch);
        assert_eq!(data, [0.1, 0.2, 0.3, 0.4]);
        assert_eq!(ring.len(), 0);
    }

    #[test]
    fn fill_pads_underrun_with_silence() {
        let ring = ring_with(&[(0.5, -0.5)]);
        let mut data = [9.0f32; 6];
        let mut scratch = Vec::with_capacity(8);
        fill_from_ring(&mut data, 2, &ring, &mut scratch);
        assert_eq!(data, [0.5, -0.5, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn fill_leaves_excess_frames_queued() {
        let ring = ring_with(&[(0.1, 0.1), (0.2, 0.2), (0.3, 0.3)]);
        let mut data = [9.0f32; 4]; // room for two frames only
        let mut scratch = Vec::with_capacity(8);
        fill_from_ring(&mut data, 2, &ring, &mut scratch);
        assert_eq!(data, [0.1, 0.1, 0.2, 0.2]);
        assert_eq!(ring.len(), 1);
        assert_eq!(pop_all(&ring), [(0.3, 0.3)]);
    }

    #[test]
    fn fill_downmixes_mono_and_zeroes_extra_channels() {
        let ring = ring_with(&[(0.2, 0.4)]);
        let mut mono = [9.0f32; 1];
        let mut scratch = Vec::with_capacity(8);
        fill_from_ring(&mut mono, 1, &ring, &mut scratch);
        assert!((mono[0] - 0.3).abs() < 1e-6);

        let ring = ring_with(&[(0.2, 0.4)]);
        let mut quad = [9.0f32; 4];
        fill_from_ring(&mut quad, 4, &ring, &mut scratch);
        assert_eq!(quad, [0.2, 0.4, 0.0, 0.0]);
    }

    #[test]
    fn fill_never_grows_scratch_beyond_capacity() {
        // Device demands 8 frames but scratch only holds 2: the drain is
        // capped, the extra is silence, and scratch never reallocates.
        let ring = ring_with(&[(0.1, 0.1), (0.2, 0.2), (0.3, 0.3), (0.4, 0.4)]);
        let mut scratch = Vec::with_capacity(2);
        let mut data = [9.0f32; 16]; // 8 stereo frames
        fill_from_ring(&mut data, 2, &ring, &mut scratch);
        assert_eq!(scratch.capacity(), 2);
        // First two frames are real; the remaining six are silence.
        assert_eq!(data[..4], [0.1, 0.1, 0.2, 0.2]);
        assert_eq!(data[4..], [0.0; 12]);
    }

    // --- sample-format conversion (via the fill path) ----------------------

    #[test]
    fn fill_converts_u8_full_scale() {
        // U8 origin is 128; -1.0 -> 0, 0.0 -> 128, +1.0 -> 255.
        let ring = ring_with(&[(-1.0, 0.0), (1.0, 0.0)]);
        let mut data = [200u8; 4];
        let mut scratch = Vec::with_capacity(8);
        fill_from_ring(&mut data, 2, &ring, &mut scratch);
        assert_eq!(data, [0, 128, 255, 128]);
    }

    #[test]
    fn fill_converts_i32_full_scale() {
        let ring = ring_with(&[(-1.0, 0.0), (1.0, 0.0)]);
        let mut data = [7i32; 4];
        let mut scratch = Vec::with_capacity(8);
        fill_from_ring(&mut data, 2, &ring, &mut scratch);
        assert_eq!(data, [i32::MIN, 0, i32::MAX, 0]);
    }

    #[test]
    fn fill_converts_i16_full_scale() {
        let ring = ring_with(&[(-1.0, 0.0), (1.0, 0.0)]);
        let mut data = [7i16; 4];
        let mut scratch = Vec::with_capacity(8);
        fill_from_ring(&mut data, 2, &ring, &mut scratch);
        assert_eq!(data, [i16::MIN, 0, i16::MAX, 0]);
    }

    #[test]
    fn fill_converts_u16_full_scale() {
        // U16 origin is 32768.
        let ring = ring_with(&[(-1.0, 0.0), (1.0, 0.0)]);
        let mut data = [7u16; 4];
        let mut scratch = Vec::with_capacity(8);
        fill_from_ring(&mut data, 2, &ring, &mut scratch);
        assert_eq!(data, [0, 32_768, u16::MAX, 32_768]);
    }

    // --- resampler ---------------------------------------------------------

    #[test]
    fn resampler_first_output_equals_first_input() {
        // `prev` must seed from the first real frame, not silence, so the
        // first output sample is the first input sample (no startup click).
        let mut r = Resampler::new(44_100, 48_000);
        let input = [(0.7, -0.3), (0.1, 0.2)];
        let mut out = Vec::new();
        r.run(&input, &mut out);
        assert_eq!(out[0], (0.7, -0.3));
    }

    #[test]
    fn resampler_passthrough_when_rates_match() {
        let mut r = Resampler::new(48_000, 48_000);
        let input = [(0.1, -0.1), (0.2, -0.2)];
        let mut out = Vec::new();
        r.run(&input, &mut out);
        assert_eq!(out, input);
    }

    #[test]
    fn resampler_doubles_sample_count_at_2x() {
        let mut r = Resampler::new(24_000, 48_000);
        let input: Vec<(f32, f32)> = (0..100).map(|i| (i as f32, -(i as f32))).collect();
        let mut out = Vec::new();
        r.run(&input, &mut out);
        assert_eq!(out.len(), 200);
        // out[3] interpolates halfway between input[0]=0.0 and input[1]=1.0.
        assert!((out[3].0 - 0.5).abs() < 1e-4);
    }

    #[test]
    fn resampler_halves_sample_count_at_half_rate() {
        let mut r = Resampler::new(48_000, 24_000);
        let input = vec![(0.0f32, 0.0f32); 100];
        let mut out = Vec::new();
        r.run(&input, &mut out);
        assert_eq!(out.len(), 50);
    }

    #[test]
    fn resampler_streams_across_calls() {
        // Feeding 1000 samples in chunks of 7 produces the same count as one
        // big call.
        let input: Vec<(f32, f32)> = (0..1000).map(|i| (i as f32, 0.0)).collect();
        let mut whole = Vec::new();
        Resampler::new(48_000, 44_100).run(&input, &mut whole);
        let mut chunked = Vec::new();
        let mut r = Resampler::new(48_000, 44_100);
        for chunk in input.chunks(7) {
            r.run(chunk, &mut chunked);
        }
        assert_eq!(whole, chunked);
    }
}
