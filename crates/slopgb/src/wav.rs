//! Std-only WAV encoder for the audio recorder (Joypad → "Audio"). Buffers the
//! recorded stereo frames in memory while recording and emits a 16-bit PCM WAV
//! on stop (`encode_wav`); the frontend picks the path and writes the bytes. No
//! dep (the frontend stays winit/softbuffer/cpal-only). Kept pure so it tests
//! headless.

/// Encode stereo `frames` (f32 −1..1 per channel, clamped) as a 16-bit PCM WAV
/// at `rate` Hz. Standard 44-byte RIFF/WAVE header (`fmt ` + `data`) then
/// interleaved little-endian i16 L,R samples.
#[must_use]
pub fn encode_wav(frames: &[(f32, f32)], rate: u32) -> Vec<u8> {
    const CHANNELS: u16 = 2;
    const BITS: u16 = 16;
    let block_align = CHANNELS * BITS / 8; // 4 bytes / frame
    let byte_rate = rate * u32::from(block_align);
    let data_len = (frames.len() * usize::from(block_align)) as u32;

    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes()); // RIFF chunk size
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // audio format = PCM
    out.extend_from_slice(&CHANNELS.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&BITS.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    let to_i16 = |s: f32| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
    for &(l, r) in frames {
        out.extend_from_slice(&to_i16(l).to_le_bytes());
        out.extend_from_slice(&to_i16(r).to_le_bytes());
    }
    out
}

#[cfg(test)]
#[path = "wav_tests.rs"]
mod tests;
