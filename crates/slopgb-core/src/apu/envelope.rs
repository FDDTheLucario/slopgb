//! Volume envelope (NRx2) shared by the pulse and noise channels.
//!
//! Clocked at 64 Hz by frame-sequencer step 7. A period of 0 disables the
//! envelope, but the internal timer still treats 0 as 8 when reloaded on
//! trigger (gbdev wiki "Game Boy Sound Operation", Obscure Behavior).

pub(super) struct Envelope {
    /// NRx2 bits 7-4.
    pub(super) initial_volume: u8,
    /// NRx2 bit 3: 1 = increase.
    pub(super) add_mode: bool,
    /// NRx2 bits 2-0.
    pub(super) period: u8,
    /// Current output volume, 0-15.
    pub(super) volume: u8,
    pub(super) timer: u8,
}

impl Envelope {
    pub(super) fn new() -> Self {
        Self {
            initial_volume: 0,
            add_mode: false,
            period: 0,
            volume: 0,
            timer: 0,
        }
    }

    pub(super) fn write(&mut self, value: u8) {
        self.initial_volume = value >> 4;
        self.add_mode = value & 0x08 != 0;
        self.period = value & 0x07;
    }

    pub(super) fn read(&self) -> u8 {
        (self.initial_volume << 4) | (u8::from(self.add_mode) << 3) | self.period
    }

    /// NRx2 high 5 bits non-zero <=> the channel's DAC is powered.
    pub(super) fn dac_enabled(&self) -> bool {
        self.initial_volume != 0 || self.add_mode
    }

    pub(super) fn trigger(&mut self) {
        // Period 0 is treated as 8 by the timer.
        self.timer = if self.period == 0 { 8 } else { self.period };
        self.volume = self.initial_volume;
    }

    /// 64 Hz frame-sequencer clock.
    pub(super) fn clock(&mut self) {
        if self.period == 0 {
            return;
        }
        if self.timer > 0 {
            self.timer -= 1;
        }
        if self.timer == 0 {
            self.timer = self.period;
            if self.add_mode && self.volume < 15 {
                self.volume += 1;
            } else if !self.add_mode && self.volume > 0 {
                self.volume -= 1;
            }
        }
    }

    pub(super) fn power_off(&mut self) {
        *self = Self::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readback_round_trips() {
        let mut e = Envelope::new();
        e.write(0xA5);
        assert_eq!(e.read(), 0xA5);
        assert_eq!(e.initial_volume, 0xA);
        assert!(!e.add_mode);
        assert_eq!(e.period, 5);
    }

    #[test]
    fn dac_enabled_iff_high_5_bits() {
        let mut e = Envelope::new();
        e.write(0x00);
        assert!(!e.dac_enabled());
        e.write(0x08); // volume 0 but add mode: DAC on
        assert!(e.dac_enabled());
        e.write(0x10);
        assert!(e.dac_enabled());
        e.write(0x07); // only period bits: DAC off
        assert!(!e.dac_enabled());
    }

    #[test]
    fn increases_every_period_clocks_and_clamps_at_15() {
        let mut e = Envelope::new();
        e.write(0xDA); // volume 13, increase, period 2
        e.trigger();
        assert_eq!(e.volume, 13);
        e.clock();
        assert_eq!(e.volume, 13); // period 2: first clock only arms
        e.clock();
        assert_eq!(e.volume, 14);
        e.clock();
        assert_eq!(e.volume, 14);
        e.clock();
        assert_eq!(e.volume, 15);
        for _ in 0..8 {
            e.clock();
        }
        assert_eq!(e.volume, 15); // clamped, envelope stops
    }

    #[test]
    fn envelope_decrease_clamps_at_zero() {
        let mut e = Envelope::new();
        e.write(0x21); // volume 2, decrease, period 1
        e.trigger();
        assert_eq!(e.volume, 2);
        e.clock();
        assert_eq!(e.volume, 1);
        e.clock();
        assert_eq!(e.volume, 0);
        e.clock();
        assert_eq!(e.volume, 0); // stays
    }

    #[test]
    fn period_zero_never_changes_volume() {
        let mut e = Envelope::new();
        e.write(0x50); // volume 5, decrease, period 0
        e.trigger();
        assert_eq!(e.timer, 8); // 0 treated as 8
        for _ in 0..32 {
            e.clock();
        }
        assert_eq!(e.volume, 5);
    }
}
