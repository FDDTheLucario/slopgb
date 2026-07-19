//! Std-only uncompressed-AVI video writer for the recorder (Joypad → "Video").
//! Streams each frame straight to disk (a whole recording is far too big to
//! buffer), then patches the RIFF/movi sizes + frame count and appends the
//! `idx1` index on finalize. No dep (the frontend stays winit/softbuffer/cpal-
//! only). Video only — audio is the separate WAV recorder.
//!
//! Layout: `RIFF … AVI ` / `LIST hdrl` (`avih` + `LIST strl` = `strh`+`strf`) /
//! `LIST movi` (`00db` chunks, one per frame, uncompressed bottom-up BGR) /
//! `idx1` (one entry per frame).

use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};
use std::path::Path;

/// A streaming uncompressed-AVI writer. Each `write_frame` appends one frame;
/// `finish` (or drop) patches the headers and writes the index.
pub struct AviWriter {
    file: File,
    w: u32,
    h: u32,
    /// Padded row stride (BMP rule: rows are 4-byte aligned).
    stride: usize,
    frames: u32,
    /// Per-frame `(offset-from-movi-fourcc, byte-length)` for `idx1`.
    index: Vec<(u32, u32)>,
    finished: bool,
}

impl AviWriter {
    /// Open `path` and write the header for a `w`×`h` stream at `fps`.
    pub fn create(path: &Path, w: u32, h: u32, fps: f64) -> io::Result<Self> {
        let stride = (w as usize * 3 + 3) & !3;
        let us_per_frame = (1_000_000.0 / fps).round() as u32;
        let mut file = File::create(path)?;
        write_header(&mut file, w, h, stride, us_per_frame, 0)?;
        Ok(Self {
            file,
            w,
            h,
            stride,
            frames: 0,
            index: Vec::new(),
            finished: false,
        })
    }

    /// Append one XRGB8888 `frame` (`w`×`h`, top-down). Written as a `00db`
    /// chunk of bottom-up BGR rows (AVI's DIB convention), 4-byte-aligned.
    pub fn write_frame(&mut self, frame: &[u32]) -> io::Result<()> {
        let (w, h) = (self.w as usize, self.h as usize);
        let data_len = self.stride * h;
        let movi_off = self.file.stream_position()?;
        self.file.write_all(b"00db")?;
        self.file.write_all(&(data_len as u32).to_le_bytes())?;
        let mut row = vec![0u8; self.stride];
        for y in (0..h).rev() {
            for x in 0..w {
                let px = frame.get(y * w + x).copied().unwrap_or(0);
                row[x * 3] = px as u8; // B
                row[x * 3 + 1] = (px >> 8) as u8; // G
                row[x * 3 + 2] = (px >> 16) as u8; // R
            }
            self.file.write_all(&row)?;
        }
        // idx1 `dwChunkOffset` is relative to the `movi` FourCC (the canonical
        // VfW convention: the first frame's `00db` header lands at offset 4).
        self.index
            .push(((movi_off - (MOVI_DATA_POS - 4)) as u32, data_len as u32));
        self.frames += 1;
        Ok(())
    }

    /// Patch the sizes/frame-count and append the `idx1` index. Idempotent.
    pub fn finish(&mut self) -> io::Result<()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        let movi_end = self.file.stream_position()?;
        let movi_size = movi_end - (MOVI_DATA_POS - 4); // includes the `movi` fourcc

        // idx1 chunk: one 16-byte entry per frame.
        self.file.write_all(b"idx1")?;
        self.file
            .write_all(&((self.index.len() * 16) as u32).to_le_bytes())?;
        for &(off, len) in &self.index {
            self.file.write_all(b"00db")?;
            self.file.write_all(&0x10u32.to_le_bytes())?; // AVIIF_KEYFRAME
            self.file.write_all(&off.to_le_bytes())?;
            self.file.write_all(&len.to_le_bytes())?;
        }

        let file_end = self.file.stream_position()?;
        // Patch RIFF size (file_end - 8).
        self.file.seek(SeekFrom::Start(4))?;
        self.file
            .write_all(&((file_end - 8) as u32).to_le_bytes())?;
        // Patch avih total-frames + the movi LIST size.
        self.file.seek(SeekFrom::Start(AVIH_FRAMES_POS))?;
        self.file.write_all(&self.frames.to_le_bytes())?;
        self.file.seek(SeekFrom::Start(STRH_LENGTH_POS))?;
        self.file.write_all(&self.frames.to_le_bytes())?;
        self.file.seek(SeekFrom::Start(MOVI_SIZE_POS))?;
        self.file.write_all(&(movi_size as u32).to_le_bytes())?;
        self.file.seek(SeekFrom::End(0))?;
        Ok(())
    }
}

impl Drop for AviWriter {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

// Fixed file offsets in the header we wrote, used to patch fields on finalize.
// The header is a fixed size (no variable-length fields), so these are constants.
/// `avih` total-frames field.
const AVIH_FRAMES_POS: u64 = 0x30;
/// `strh` stream-length (frames) field.
const STRH_LENGTH_POS: u64 = 0x8C;
/// The `movi` LIST size field.
const MOVI_SIZE_POS: u64 = 0xD8;
/// First byte of the movi chunk data (right after the `movi` fourcc).
const MOVI_DATA_POS: u64 = 0xE0;

/// Write the fixed-size AVI header for a `w`×`h`, `stride`-row, `us_per_frame`
/// stream with `frames` frames (patched to the real count on finish). Every
/// offset in the `*_POS` constants above is derived from this exact layout.
fn write_header(
    f: &mut File,
    w: u32,
    h: u32,
    stride: usize,
    us_per_frame: u32,
    frames: u32,
) -> io::Result<()> {
    let img = (stride * h as usize) as u32;
    // RIFF header (size patched on finish).
    f.write_all(b"RIFF")?;
    f.write_all(&0u32.to_le_bytes())?;
    f.write_all(b"AVI ")?;
    // LIST hdrl.
    f.write_all(b"LIST")?;
    f.write_all(&(4u32 + 8 + 56 + 8 + 4 + 8 + 56 + 8 + 40).to_le_bytes())?; // hdrl size
    f.write_all(b"hdrl")?;
    // avih (56 bytes).
    f.write_all(b"avih")?;
    f.write_all(&56u32.to_le_bytes())?;
    f.write_all(&us_per_frame.to_le_bytes())?; // dwMicroSecPerFrame
    let bytes_per_sec = img.saturating_mul(1_000_000 / us_per_frame.max(1));
    f.write_all(&bytes_per_sec.to_le_bytes())?; // dwMaxBytesPerSec (approx)
    f.write_all(&0u32.to_le_bytes())?; // dwPaddingGranularity
    f.write_all(&0x10u32.to_le_bytes())?; // dwFlags = AVIF_HASINDEX
    f.write_all(&frames.to_le_bytes())?; // dwTotalFrames  (AVIH_FRAMES_POS)
    f.write_all(&0u32.to_le_bytes())?; // dwInitialFrames
    f.write_all(&1u32.to_le_bytes())?; // dwStreams
    f.write_all(&img.to_le_bytes())?; // dwSuggestedBufferSize
    f.write_all(&w.to_le_bytes())?; // dwWidth
    f.write_all(&h.to_le_bytes())?; // dwHeight
    f.write_all(&[0u8; 16])?; // dwReserved[4]
    // LIST strl.
    f.write_all(b"LIST")?;
    f.write_all(&(4u32 + 8 + 56 + 8 + 40).to_le_bytes())?; // strl size
    f.write_all(b"strl")?;
    // strh (56 bytes).
    f.write_all(b"strh")?;
    f.write_all(&56u32.to_le_bytes())?;
    f.write_all(b"vids")?; // fccType
    f.write_all(b"DIB ")?; // fccHandler (uncompressed)
    f.write_all(&0u32.to_le_bytes())?; // dwFlags
    f.write_all(&0u16.to_le_bytes())?; // wPriority
    f.write_all(&0u16.to_le_bytes())?; // wLanguage
    f.write_all(&0u32.to_le_bytes())?; // dwInitialFrames
    f.write_all(&us_per_frame.to_le_bytes())?; // dwScale (µs)
    f.write_all(&1_000_000u32.to_le_bytes())?; // dwRate → rate/scale = fps
    f.write_all(&0u32.to_le_bytes())?; // dwStart
    f.write_all(&frames.to_le_bytes())?; // dwLength (frames)  (STRH_LENGTH_POS)
    f.write_all(&img.to_le_bytes())?; // dwSuggestedBufferSize
    f.write_all(&0xFFFF_FFFFu32.to_le_bytes())?; // dwQuality
    f.write_all(&img.to_le_bytes())?; // dwSampleSize
    f.write_all(&[0u8; 8])?; // rcFrame (left,top,right,bottom as i16) → 0s
    // strf = BITMAPINFOHEADER (40 bytes).
    f.write_all(b"strf")?;
    f.write_all(&40u32.to_le_bytes())?;
    f.write_all(&40u32.to_le_bytes())?; // biSize
    f.write_all(&w.to_le_bytes())?; // biWidth
    f.write_all(&h.to_le_bytes())?; // biHeight (positive → bottom-up)
    f.write_all(&1u16.to_le_bytes())?; // biPlanes
    f.write_all(&24u16.to_le_bytes())?; // biBitCount
    f.write_all(&0u32.to_le_bytes())?; // biCompression = BI_RGB
    f.write_all(&img.to_le_bytes())?; // biSizeImage
    f.write_all(&0u32.to_le_bytes())?; // biXPelsPerMeter
    f.write_all(&0u32.to_le_bytes())?; // biYPelsPerMeter
    f.write_all(&0u32.to_le_bytes())?; // biClrUsed
    f.write_all(&0u32.to_le_bytes())?; // biClrImportant
    // LIST movi (size patched on finish).
    f.write_all(b"LIST")?; // at 0xD4
    f.write_all(&0u32.to_le_bytes())?; // movi size          (MOVI_SIZE_POS = 0xD8)
    f.write_all(b"movi")?; // 0xDC..0xE0 → data starts at MOVI_DATA_POS
    Ok(())
}

#[cfg(test)]
#[path = "avi_tests.rs"]
mod tests;
