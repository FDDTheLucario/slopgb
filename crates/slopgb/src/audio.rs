//! Audio output: a cpal stream at the device's default config fed from a
//! shared queue of stereo frames, plus a streaming linear resampler to bridge
//! the core's APU sample rate to the device rate.
//!
//! The audio callback only pops from the queue; on underrun it outputs
//! silence. The emulation side pushes; pushes beyond the queue capacity
//! (~250 ms) are dropped, which doubles as the discard policy during turbo.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};

/// Stereo sample frames queued for the audio callback.
type Ring = Arc<Mutex<VecDeque<(f32, f32)>>>;

fn lock(ring: &Ring) -> MutexGuard<'_, VecDeque<(f32, f32)>> {
    // A poisoned queue just means a thread died mid-push; the data is sound.
    ring.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct AudioOutput {
    /// Held to keep the stream playing; dropped on shutdown.
    _stream: cpal::Stream,
    ring: Ring,
    sample_rate: u32,
    capacity: usize,
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
        let ring: Ring = Arc::new(Mutex::new(VecDeque::new()));
        let config = supported.config();
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => {
                build_stream::<f32>(&device, &config, channels, Arc::clone(&ring))
            }
            cpal::SampleFormat::I16 => {
                build_stream::<i16>(&device, &config, channels, Arc::clone(&ring))
            }
            cpal::SampleFormat::U16 => {
                build_stream::<u16>(&device, &config, channels, Arc::clone(&ring))
            }
            other => return Err(format!("unsupported sample format {other}")),
        }
        .map_err(|e| format!("cannot build output stream: {e}"))?;
        stream
            .play()
            .map_err(|e| format!("cannot start output stream: {e}"))?;
        Ok(Self {
            _stream: stream,
            ring,
            sample_rate,
            // ~250 ms cap so a stalled callback can't grow the queue forever.
            capacity: sample_rate as usize / 4,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Stereo frames currently queued for playback.
    pub fn queued(&self) -> usize {
        lock(&self.ring).len()
    }

    /// Queue stereo frames; anything beyond the capacity is dropped.
    pub fn push(&self, samples: &[(f32, f32)]) {
        let mut q = lock(&self.ring);
        let free = self.capacity.saturating_sub(q.len());
        q.extend(samples.iter().take(free).copied());
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    ring: Ring,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample + FromSample<f32>,
{
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let mut q = lock(&ring);
            for frame in data.chunks_mut(channels) {
                let (l, r) = q.pop_front().unwrap_or((0.0, 0.0)); // underrun: silence
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
        },
        |err| eprintln!("slopgb: audio stream error: {err}"),
        None,
    )
}

/// Streaming linear resampler for stereo frames. Pass-through when the rates
/// match (the expected case once the core's sample rate can be set to the
/// device rate).
pub struct Resampler {
    src_rate: u32,
    dst_rate: u32,
    /// Position of the next output sample, in source samples, relative to
    /// `prev` (so `prev` is at 0.0 and the next input sample is at 1.0).
    pos: f64,
    prev: (f32, f32),
}

impl Resampler {
    pub fn new(src_rate: u32, dst_rate: u32) -> Self {
        Self {
            src_rate,
            dst_rate,
            pos: 0.0,
            prev: (0.0, 0.0),
        }
    }

    /// Convert `input` (source rate) and append the result to `out`
    /// (destination rate). State carries across calls.
    pub fn run(&mut self, input: &[(f32, f32)], out: &mut Vec<(f32, f32)>) {
        if self.src_rate == self.dst_rate {
            out.extend_from_slice(input);
            return;
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
