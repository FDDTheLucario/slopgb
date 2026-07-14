//! Manual little-endian binary (de)serialization for on-disk save states
//! (bgb's File → Save state / Load state). Std-only and `forbid(unsafe_code)`,
//! so there is no serde and no memory-dump: every struct hand-writes its
//! volatile state through [`Writer`] / [`Reader`].
//!
//! This is a live-debugger/UI feature — `GameBoy::save_state` is `&self`
//! (read-only) and `GameBoy::load_state` is never reached on a golden/test
//! path, so adding the serializers leaves the gbtr fingerprint byte-identical
//! (golden-safe).

/// A growable little-endian byte sink. Width-typed pushes keep the read side
/// (the symmetric [`Reader`]) in lock-step.
#[derive(Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn bool(&mut self, v: bool) {
        self.buf.push(u8::from(v));
    }
    /// Raw bytes, length-implicit (the reader knows the fixed length).
    pub fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }
    /// A fixed-length `u32` array, little-endian (the PPU frame buffers).
    pub fn u32_slice(&mut self, s: &[u32]) {
        for &v in s {
            self.u32(v);
        }
    }
    /// Write an `Option<T>` as a presence byte, followed (if `Some`) by the
    /// payload written by `write_payload` — the one canonical shape for every
    /// optional staged-event/scratch field across the save-state format
    /// (replaces the several near-identical `write_opt*` helpers that used to
    /// be hand-duplicated per payload shape in each module).
    pub fn write_opt<T>(&mut self, o: &Option<T>, write_payload: impl FnOnce(&mut Self, &T)) {
        match o {
            Some(v) => {
                self.bool(true);
                write_payload(self, v);
            }
            None => self.bool(false),
        }
    }
    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }
}

/// Cursor over a save-state byte buffer. Every `take` is bounds-checked, so a
/// truncated/corrupt file is a [`StateError::Truncated`], never a panic.
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&[u8], StateError> {
        let end = self.pos.checked_add(n).ok_or(StateError::Truncated)?;
        let slice = self.buf.get(self.pos..end).ok_or(StateError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }
    pub fn u8(&mut self) -> Result<u8, StateError> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16, StateError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    pub fn u32(&mut self) -> Result<u32, StateError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn u64(&mut self) -> Result<u64, StateError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
    pub fn bool(&mut self) -> Result<bool, StateError> {
        Ok(self.u8()? != 0)
    }
    /// Fill `dst` from the next `dst.len()` bytes.
    pub fn bytes_into(&mut self, dst: &mut [u8]) -> Result<(), StateError> {
        let b = self.take(dst.len())?;
        dst.copy_from_slice(b);
        Ok(())
    }
    /// Read `n` bytes into a fresh `Vec`. Bounds-checked *before* allocating, so
    /// a corrupt length can't trigger a huge speculative allocation (it errors
    /// at [`Self::take`] instead).
    pub fn bytes_vec(&mut self, n: usize) -> Result<Vec<u8>, StateError> {
        Ok(self.take(n)?.to_vec())
    }
    /// Fill `dst` from the next `dst.len()` little-endian `u32`s.
    pub fn u32_slice_into(&mut self, dst: &mut [u32]) -> Result<(), StateError> {
        for d in dst {
            *d = self.u32()?;
        }
        Ok(())
    }
    /// Read an `Option<T>` written by [`Writer::write_opt`]: a presence byte,
    /// then (if set) the payload read by `read_payload`.
    pub fn read_opt<T>(
        &mut self,
        read_payload: impl FnOnce(&mut Self) -> Result<T, StateError>,
    ) -> Result<Option<T>, StateError> {
        Ok(if self.bool()? {
            Some(read_payload(self)?)
        } else {
            None
        })
    }
}

/// Why a `GameBoy::load_state` was rejected. Never a panic — a corrupt or
/// foreign save state is an error the UI reports while leaving the machine
/// intact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateError {
    /// The buffer ended before a field could be read (truncated/corrupt).
    Truncated,
    /// The leading magic bytes don't identify a slopgb save state.
    BadMagic,
    /// The format version isn't one this build can load.
    BadVersion,
    /// The state was saved from a different ROM than the one loaded.
    RomMismatch,
    /// The state was saved from a different *system* than the one loaded: its
    /// SGB-audio-tail flag disagrees with this machine's model (an SGB state
    /// loaded into DMG/CGB, or a DMG/CGB state loaded into SGB).
    ModelMismatch,
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            StateError::Truncated => "save state is truncated or corrupt",
            StateError::BadMagic => "not a slopgb save state",
            StateError::BadVersion => "unsupported save-state version",
            StateError::RomMismatch => "save state is for a different ROM",
            StateError::ModelMismatch => {
                "save state is for a different system (SGB audio present/absent mismatch)"
            }
        };
        f.write_str(s)
    }
}

impl std::error::Error for StateError {}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
