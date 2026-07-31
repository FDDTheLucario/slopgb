//! MSU-1 streaming-audio coprocessor, as a slopgb tier-3 wasm plugin.
//!
//! MSU-1 is an open homebrew add-on chip (near/byuu): eight memory-mapped
//! registers stream a CD-quality `.pcm` audio track and expose a large `.msu`
//! data ROM by seek/read. This plugin implements that register interface over
//! the coprocessor comm ports and streams the (user-supplied) files through the
//! v4 bulk-file channel — nothing copyrighted is reproduced; the audio + data
//! packs are the user's own files, served host-side.
//!
//! # Two modes (both ride the same coprocessor)
//!
//! 1. **Register interface.** Comm ports `0..=7` map 1:1 to the SNES MSU-1
//!    registers `$2000..=$2007` (see [`Msu1::port_write`] / [`Msu1::port_read`]).
//!    The game writes a track number + a control byte; the plugin streams that
//!    track's `.pcm`, and reads the `.msu` data ROM by a 32-bit seek pointer.
//! 2. **Resident handler + polled mailbox.** Every `run_until`, the plugin polls
//!    the host mailbox ([`slopgb_plugin_api::recv_mailbox`]); a game that writes a
//!    `[cmd, track_lo, track_hi, flags]` play-request there starts playback with
//!    no register writes — the general homebrew custom-music pattern of which the
//!    fixed registers are a special case.
//!
//! # References (open MSU-1 documentation)
//!
//! - zumi MSU-1 notes: <https://zumi.neocities.org/stuff/msu1_notes/>
//! - Sunlitspace542/MSU-1-Docs: <https://github.com/Sunlitspace542/MSU-1-Docs>
//!
//! The register map, status/control bit layout, and `.pcm` format below are a
//! port of those specs.

use slopgb_plugin_api::{Coprocessor, read_file, recv_mailbox, slopgb_coprocessor_plugin};

/// The host-file key (see [`read_file`]) the plugin reads the `.msu` data ROM
/// from. Audio tracks use their own number as the key; a 16-bit track can never
/// collide with this reserved 32-bit value, so the two file kinds stay distinct.
pub const DATA_FILE_KEY: u32 = 0xFFFF_FFFF;

/// A `.pcm` track begins with an 8-byte header: the 4-byte magic then a 32-bit
/// little-endian loop point (in samples); the interleaved 16-bit stereo samples
/// follow. [`PCM_MAGIC`] identifies a valid track.
pub const PCM_HEADER_LEN: u32 = 8;
/// `"MSU1"` — the `.pcm` magic (zumi notes / MSU-1-Docs).
pub const PCM_MAGIC: [u8; 4] = *b"MSU1";
/// One stereo sample is two little-endian `i16`s (left, right) = 4 bytes.
const BYTES_PER_SAMPLE: u32 = 4;
/// The chip revision reported in the low status bits (revision 1).
const REVISION: u8 = 1;
/// The six ID bytes read back from ports `2..=7` — `"S-MSU1"` spells out the
/// chip so a game can detect it (zumi notes).
const ID: [u8; 6] = *b"S-MSU1";

/// MSU_STATUS (`$2000` read) bit positions.
const ST_TRACK_MISSING: u8 = 1 << 3;
const ST_AUDIO_PLAYING: u8 = 1 << 4;
const ST_AUDIO_REPEAT: u8 = 1 << 5;

/// MSU_CONTROL (`$2007` write) bit positions.
const CTL_PLAY: u8 = 1 << 0;
const CTL_REPEAT: u8 = 1 << 1;
const CTL_RESUME: u8 = 1 << 2;

/// The MSU-1 streaming-audio coprocessor state.
struct Msu1 {
    /// The 32-bit data-ROM seek pointer being assembled from writes to ports
    /// `0..=3`; the write to port 3 commits it into `data_pos`.
    seek_buf: [u8; 4],
    /// Current `.msu` data-ROM read pointer (auto-increments on a port-1 read).
    data_pos: u32,
    /// The 16-bit track number being assembled from writes to ports 4..=5.
    track_buf: [u8; 2],
    /// The selected audio track (host-file key for [`read_file`]).
    track: u16,
    /// Track volume (0 = mute, 0xFF ≈ unity); defaults to full so a track is
    /// audible without an explicit volume write.
    volume: u8,
    /// Playback flags derived from the control register / mailbox.
    playing: bool,
    repeat: bool,
    /// The requested track's `.pcm` was absent or had a bad header.
    track_missing: bool,
    /// Byte offset of the next sample within the track's sample data (the file
    /// offset is `PCM_HEADER_LEN + audio_pos`).
    audio_pos: u32,
    /// The track's loop point in *samples* (from the header); a repeat seeks to
    /// `loop_point * BYTES_PER_SAMPLE`.
    loop_point: u32,
    /// Stereo PCM synthesized since the last drain (oldest first).
    pending: Vec<(i16, i16)>,
    /// The chip's own cycle domain — one cycle == one output (44.1 kHz) sample.
    cycle: u64,
    /// The last mailbox contents, for edge-detecting a new play-request.
    last_mailbox: Vec<u8>,
}

impl Msu1 {
    /// The MSU_STATUS byte (`$2000` read): revision in the low bits, then the
    /// track-missing / playing / repeat flags (zumi notes bit layout).
    fn status(&self) -> u8 {
        let mut s = REVISION & 0b0000_0111;
        if self.track_missing {
            s |= ST_TRACK_MISSING;
        }
        if self.playing {
            s |= ST_AUDIO_PLAYING;
        }
        if self.repeat {
            s |= ST_AUDIO_REPEAT;
        }
        s
    }

    /// Load the selected track's header: validate the magic and latch the loop
    /// point, or mark the track missing. Resets the play position.
    fn select_track(&mut self) {
        let mut header = [0u8; PCM_HEADER_LEN as usize];
        let n = read_file(u32::from(self.track), 0, &mut header);
        self.audio_pos = 0;
        match parse_pcm_header(&header[..n]) {
            Some(loop_point) => {
                self.track_missing = false;
                self.loop_point = loop_point;
            }
            None => {
                self.track_missing = true;
                self.loop_point = 0;
                self.playing = false;
            }
        }
    }

    /// Apply a control-register (`$2007`) write: repeat/play/resume (zumi notes).
    /// Play restarts from the beginning; resume keeps the current position.
    fn write_control(&mut self, val: u8) {
        self.repeat = val & CTL_REPEAT != 0;
        if self.track_missing {
            self.playing = false;
        } else if val & CTL_RESUME != 0 {
            self.playing = true;
        } else if val & CTL_PLAY != 0 {
            self.audio_pos = 0;
            self.playing = true;
        } else {
            self.playing = false;
        }
    }

    /// Poll the host mailbox once; a changed non-empty `[cmd, track_lo, track_hi,
    /// flags]` with `cmd == 1` selects that track and starts playback (mode 2 —
    /// the resident-handler / polled-mailbox path). `flags` bit 0 = repeat.
    fn poll_mailbox(&mut self) {
        let mb = recv_mailbox();
        if mb == self.last_mailbox {
            return;
        }
        self.last_mailbox = mb.clone();
        if let [1, lo, hi, rest @ ..] = mb.as_slice() {
            self.track = u16::from_le_bytes([*lo, *hi]);
            self.repeat = rest.first().is_some_and(|f| f & 1 != 0);
            self.select_track();
            if !self.track_missing {
                self.audio_pos = 0;
                self.playing = true;
            }
        }
    }

    /// Produce one stereo sample from the streaming track, advancing the play
    /// position. Returns `None` (and stops playback) at a non-repeating end.
    fn next_sample(&mut self) -> Option<(i16, i16)> {
        let mut buf = [0u8; BYTES_PER_SAMPLE as usize];
        // At end-of-data, seek to the loop point once and retry; a still-short
        // read (empty / past-end loop point) stops playback rather than spinning.
        for _ in 0..2 {
            let file_off = PCM_HEADER_LEN + self.audio_pos;
            if read_file(u32::from(self.track), file_off, &mut buf) == buf.len() {
                self.audio_pos += BYTES_PER_SAMPLE;
                let l = scale_sample(i16::from_le_bytes([buf[0], buf[1]]), self.volume);
                let r = scale_sample(i16::from_le_bytes([buf[2], buf[3]]), self.volume);
                return Some((l, r));
            }
            if self.repeat {
                self.audio_pos = self.loop_point.saturating_mul(BYTES_PER_SAMPLE);
            } else {
                break;
            }
        }
        self.playing = false;
        None
    }
}

impl Coprocessor for Msu1 {
    const MANIFEST: &'static str = concat!(
        "id\tmsu1\n",
        "name\tMSU-1 Streaming Audio\n",
        "provides\tstreaming-audio\n",
        "flag\tmsu1\tdir\tLoad an MSU-1 streaming-audio pack from DIR\t$rom_dir",
    );

    fn new() -> Self {
        Msu1 {
            seek_buf: [0; 4],
            data_pos: 0,
            track_buf: [0; 2],
            track: 0,
            volume: 0xFF,
            playing: false,
            repeat: false,
            track_missing: false,
            audio_pos: 0,
            loop_point: 0,
            pending: Vec::new(),
            cycle: 0,
            last_mailbox: Vec::new(),
        }
    }

    fn reset(&mut self) {
        *self = Msu1::new();
    }

    /// Advance to `target_cycle` (== the 44.1 kHz output-sample index): poll the
    /// mailbox, then synthesize one sample per elapsed cycle while playing. The
    /// host drains the samples with [`Coprocessor::drain_pcm`].
    // ponytail: one host `read_file` crossing per sample (a track streams a few
    // hundred samples/frame). Batch into a chunk read if the crossing cost ever
    // shows up in a profile.
    fn run_until(&mut self, target_cycle: u64) -> u64 {
        self.poll_mailbox();
        let n = target_cycle.saturating_sub(self.cycle);
        for _ in 0..n {
            if !self.playing {
                break;
            }
            match self.next_sample() {
                Some(pair) => self.pending.push(pair),
                None => break,
            }
        }
        self.cycle = self.cycle.max(target_cycle);
        self.cycle
    }

    fn port_write(&mut self, port: u8, val: u8) {
        match port & 7 {
            0 => self.seek_buf[0] = val,
            1 => self.seek_buf[1] = val,
            2 => self.seek_buf[2] = val,
            3 => {
                self.seek_buf[3] = val;
                self.data_pos = u32::from_le_bytes(self.seek_buf);
            }
            4 => self.track_buf[0] = val,
            5 => {
                self.track_buf[1] = val;
                self.track = u16::from_le_bytes(self.track_buf);
                self.select_track();
            }
            6 => self.volume = val,
            _ => self.write_control(val),
        }
    }

    fn port_read(&mut self, port: u8) -> u8 {
        match port & 7 {
            0 => self.status(),
            1 => {
                let mut b = [0u8; 1];
                let v = if read_file(DATA_FILE_KEY, self.data_pos, &mut b) == 1 {
                    b[0]
                } else {
                    0
                };
                self.data_pos = self.data_pos.wrapping_add(1);
                v
            }
            p => ID[usize::from(p - 2)],
        }
    }

    fn drain_pcm(&mut self) -> Vec<(i16, i16)> {
        std::mem::take(&mut self.pending)
    }
}

/// Validate a `.pcm` header and return its loop point (in samples), or `None`
/// if the magic is wrong or the header is truncated.
fn parse_pcm_header(header: &[u8]) -> Option<u32> {
    let header: &[u8; 8] = header.get(..8)?.try_into().ok()?;
    if header[..4] != PCM_MAGIC {
        return None;
    }
    Some(u32::from_le_bytes([
        header[4], header[5], header[6], header[7],
    ]))
}

/// Scale a sample by the MSU-1 volume byte (0 = mute, 0xFF ≈ unity gain).
fn scale_sample(sample: i16, volume: u8) -> i16 {
    ((i32::from(sample) * i32::from(volume)) >> 8) as i16
}

slopgb_coprocessor_plugin!(Msu1);

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
