//! Wave channel (channel 3): 32 4-bit samples from FF30-FF3F.
//!
//! DMG quirks implemented here (gbdev wiki "Game Boy Sound Operation",
//! Blargg dmg_sound 09-wave read while on / 10-wave trigger while on / 12-wave):
//! - While the channel plays, CPU wave-RAM accesses only work within ~2
//!   T-cycles of the channel's own sample fetch, and they hit the byte the
//!   channel just read; otherwise reads return 0xFF and writes are lost.
//!   On CGB the access always works and hits the current byte.
//! - Retriggering on DMG while the channel is about to fetch corrupts the
//!   first four bytes of wave RAM.

use super::length::LengthCounter;

#[derive(Clone)]
pub(super) struct Wave {
    cgb: bool,
    pub(super) enabled: bool,
    /// NR30 bit 7.
    pub(super) dac: bool,
    /// NR32 bits 6-5.
    pub(super) volume_code: u8,
    /// 11-bit frequency from NR33/NR34.
    pub(super) freq: u16,
    pub(super) length: LengthCounter,
    /// T-cycles until the next sample fetch (always >= 1 while enabled).
    pub(super) timer: u32,
    /// Sample index 0-31.
    pub(super) position: u8,
    /// Last byte fetched from wave RAM ("sample buffer").
    pub(super) sample_byte: u8,
    pub(super) ram: [u8; 16],
    /// T-cycles since the last sample fetch (saturating).
    pub(super) t_since_fetch: u32,
}

impl Wave {
    pub(super) fn new(cgb: bool) -> Self {
        Self {
            cgb,
            enabled: false,
            dac: false,
            volume_code: 0,
            freq: 0,
            length: LengthCounter::new(256),
            // period() at frequency 0: (2048 - 0) * 2.
            timer: 4096,
            position: 0,
            sample_byte: 0,
            ram: [0; 16],
            t_since_fetch: u32::MAX,
        }
    }

    /// Sample fetch period in T-cycles.
    pub(super) fn period(&self) -> u32 {
        (2048 - u32::from(self.freq)) * 2
    }

    /// Advance one T-cycle.
    pub(super) fn step(&mut self) {
        self.t_since_fetch = self.t_since_fetch.saturating_add(1);
        if !self.enabled {
            return;
        }
        debug_assert!(
            self.timer > 0,
            "wave frequency timer invariant violated: must stay >= 1 while enabled"
        );
        self.timer -= 1;
        if self.timer == 0 {
            self.timer = self.period();
            // Position advances *before* the read: after a trigger the first
            // sample fetched is index 1, the low nibble of byte 0 — sample 0
            // is skipped (Pan Docs "Sound Channel 3").
            self.position = (self.position + 1) & 31;
            self.sample_byte = self.ram[usize::from(self.position >> 1)];
            self.t_since_fetch = 0;
        }
    }

    /// Current digital output, 0-15. NR32 volume codes: 0 = mute, 1 = 100%,
    /// 2 = 50%, 3 = 25% (right shifts 4/0/1/2).
    pub(super) fn digital(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        let nibble = if self.position & 1 == 0 {
            self.sample_byte >> 4
        } else {
            self.sample_byte & 0x0F
        };
        nibble >> [4u8, 0, 1, 2][usize::from(self.volume_code)]
    }

    /// CPU access to wave RAM lands on the byte the channel is playing, and
    /// on DMG only within 2 T-cycles of the channel's own fetch.
    fn cpu_ram_slot(&self, index: usize) -> Option<usize> {
        if !self.enabled {
            Some(index)
        } else if self.cgb || self.t_since_fetch < 2 {
            Some(usize::from(self.position >> 1))
        } else {
            None
        }
    }

    pub(super) fn read_ram(&self, index: usize) -> u8 {
        match self.cpu_ram_slot(index) {
            Some(i) => self.ram[i],
            None => 0xFF,
        }
    }

    pub(super) fn write_ram(&mut self, index: usize, value: u8) {
        if let Some(i) = self.cpu_ram_slot(index) {
            self.ram[i] = value;
        }
    }

    pub(super) fn trigger(&mut self) {
        // DMG wave-RAM corruption: retriggering while the channel is within
        // 2 T-cycles of a sample fetch rewrites the first 4 bytes. If the
        // byte about to be read is one of the first four, only byte 0 is
        // rewritten with it; otherwise bytes 0-3 are rewritten with the
        // aligned 4-byte block containing it (gbdev wiki, Obscure Behavior).
        if !self.cgb && self.enabled && self.timer <= 2 {
            let offset = usize::from((self.position + 1) & 31) >> 1;
            if offset < 4 {
                self.ram[0] = self.ram[offset];
            } else {
                let base = offset & !3;
                let block = [
                    self.ram[base],
                    self.ram[base + 1],
                    self.ram[base + 2],
                    self.ram[base + 3],
                ];
                self.ram[..4].copy_from_slice(&block);
            }
        }
        self.enabled = self.dac;
        self.position = 0;
        // The first fetch after a trigger is delayed by 6 extra T-cycles;
        // until then the stale sample buffer keeps playing (gbdev wiki).
        self.timer = self.period() + 6;
    }

    pub(super) fn power_off(&mut self, clear_length_counter: bool) {
        self.enabled = false;
        self.dac = false;
        self.volume_code = 0;
        self.freq = 0;
        self.length.power_off(clear_length_counter);
        // Wave RAM survives power off.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing_wave(cgb: bool, freq: u16) -> Wave {
        let mut w = Wave::new(cgb);
        for (i, b) in w.ram.iter_mut().enumerate() {
            *b = i as u8;
        }
        w.dac = true;
        w.volume_code = 1;
        w.freq = freq;
        w.trigger();
        w
    }

    #[test]
    fn volume_codes_shift_output() {
        let mut w = Wave::new(false);
        w.enabled = true;
        w.sample_byte = 0xC8; // high nibble 12, low nibble 8
        w.position = 0; // even: high nibble
        for (code, want) in [(0u8, 0u8), (1, 12), (2, 6), (3, 3)] {
            w.volume_code = code;
            assert_eq!(w.digital(), want, "code {code}");
        }
        w.position = 1; // odd: low nibble
        w.volume_code = 1;
        assert_eq!(w.digital(), 8);
    }

    #[test]
    fn first_sample_after_trigger_is_index_1() {
        let mut w = playing_wave(false, 2047); // period 2, first fetch at T 8
        w.ram[0] = 0xAB;
        for _ in 0..7 {
            w.step();
        }
        assert_eq!(w.position, 0, "still replaying the stale buffer");
        w.step(); // T 8: first fetch
        assert_eq!(w.position, 1);
        assert_eq!(w.sample_byte, 0xAB);
        assert_eq!(w.digital(), 0x0B, "low nibble of byte 0 plays first");
    }

    #[test]
    fn trigger_keeps_stale_sample_buffer() {
        let mut w = playing_wave(false, 2047);
        for _ in 0..32 {
            w.step();
        }
        let stale = w.sample_byte;
        w.trigger();
        assert_eq!(w.sample_byte, stale);
    }

    #[test]
    fn ram_fully_accessible_while_channel_off() {
        let mut w = Wave::new(false);
        for i in 0..16 {
            w.write_ram(i, 0xF0 | i as u8);
        }
        for i in 0..16 {
            assert_eq!(w.read_ram(i), 0xF0 | i as u8);
        }
    }

    #[test]
    fn dmg_ram_access_only_inside_fetch_window() {
        let mut w = playing_wave(false, 2047);
        // Before the first fetch there is no window: reads see 0xFF.
        for _ in 0..4 {
            w.step();
        }
        assert_eq!(w.read_ram(0), 0xFF);
        w.write_ram(3, 0x99);
        assert_eq!(w.ram[3], 3, "write outside the window is lost");
        // At max frequency a fetch happens every 2 T-cycles, so the window
        // is effectively always open; every address maps to the current byte.
        for _ in 0..8 {
            w.step();
        }
        let current = usize::from(w.position >> 1);
        for i in 0..16 {
            assert_eq!(w.read_ram(i), w.ram[current]);
        }
        w.write_ram(9, 0x5A);
        assert_eq!(w.ram[current], 0x5A);
    }

    #[test]
    fn dmg_ram_access_blocked_at_low_frequency() {
        let mut w = playing_wave(false, 0); // period 4096
        for _ in 0..64 {
            w.step();
        }
        // Last fetch is ages away.
        assert_eq!(w.read_ram(0), 0xFF);
        let before = w.ram;
        w.write_ram(0, 0xEE);
        assert_eq!(w.ram, before);
    }

    #[test]
    fn cgb_ram_access_always_hits_current_byte() {
        let mut w = playing_wave(true, 0); // slow: window never open on DMG
        for _ in 0..64 {
            w.step();
        }
        let current = usize::from(w.position >> 1);
        assert_eq!(w.read_ram(5), w.ram[current]);
        w.write_ram(5, 0x77);
        assert_eq!(w.ram[current], 0x77);
    }

    #[test]
    fn dmg_retrigger_corruption_first_four_bytes() {
        // Position p, next fetch reads byte (p+1)/2. Walk to a spot where
        // that byte is < 4: ram[0] takes its value.
        let mut w = playing_wave(false, 2047);
        // First fetch at T8 (pos 1), then every 2T: after 8+2k steps pos=1+k.
        for _ in 0..12 {
            w.step(); // pos = 3, fetch just happened, timer = 2
        }
        assert_eq!(w.position, 3);
        assert!(w.timer <= 2);
        w.trigger();
        // Next fetch would read byte (3+1)>>1 = 2.
        assert_eq!(w.ram[0], 2);
        assert_eq!(w.ram[1], 1, "later bytes untouched");
    }

    #[test]
    fn dmg_retrigger_corruption_aligned_block() {
        let mut w = playing_wave(false, 2047);
        // pos = 1 + k after 8 + 2k steps; 40 steps -> pos 17, next byte 9,
        // aligned block 8..12.
        for _ in 0..40 {
            w.step();
        }
        assert_eq!(w.position, 17);
        w.trigger();
        assert_eq!(&w.ram[..4], &[8, 9, 10, 11]);
        assert_eq!(w.ram[4], 4, "bytes past 3 untouched");
    }

    #[test]
    fn no_corruption_when_no_fetch_imminent() {
        let mut w = playing_wave(false, 0); // period 4096
        for _ in 0..16 {
            w.step();
        }
        assert!(w.timer > 2);
        let before = w.ram;
        w.trigger();
        assert_eq!(w.ram, before);
    }

    #[test]
    fn no_corruption_on_cgb() {
        let mut w = playing_wave(true, 2047);
        for _ in 0..12 {
            w.step();
        }
        let before = w.ram;
        w.trigger();
        assert_eq!(w.ram, before);
    }

    #[test]
    fn no_corruption_when_channel_disabled() {
        let mut w = playing_wave(false, 2047);
        for _ in 0..12 {
            w.step();
        }
        w.enabled = false;
        let before = w.ram;
        w.trigger();
        assert_eq!(w.ram, before);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "wave frequency timer")]
    fn step_with_zero_timer_panics_in_debug() {
        let mut w = Wave::new(false);
        w.enabled = true;
        w.timer = 0; // violates the "timer always >= 1 while enabled" invariant
        w.step();
    }

    #[test]
    fn disabled_channel_outputs_zero_and_does_not_fetch() {
        let mut w = playing_wave(false, 2047);
        w.enabled = false;
        let pos = w.position;
        for _ in 0..64 {
            w.step();
        }
        assert_eq!(w.position, pos);
        assert_eq!(w.digital(), 0);
    }
}
