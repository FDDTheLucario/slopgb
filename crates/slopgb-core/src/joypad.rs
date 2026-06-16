//! Joypad matrix (FF00 P1) and the SGB command-packet/multiplayer port.
//! Timer/serial/joypad work package.
//!
//! P1 exposes a 2x4 key matrix: bit 4 selects the d-pad column, bit 5 the
//! button column (both active low). The low nibble is the AND of all
//! selected columns (pressed = 0). The joypad interrupt fires on any
//! high-to-low transition of the P10-P13 input lines — whether caused by a
//! button press or by a select-line write that exposes an already-held
//! button (Pan Docs "Joypad Input" / "INT $60").
//!
//! On SGB/SGB2 the ICD2 also listens to P1 writes: command packets arrive
//! as pulse-coded bit streams on P14/P15 (Pan Docs "SGB Command Packet"),
//! and after a MLT_REQ command the reads with both select lines high
//! return the current joypad ID instead of key lines (Pan Docs "SGB
//! Command $11 — MLT_REQ"). See [`Sgb`].

/// A Game Boy button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    Right,
    Left,
    Up,
    Down,
    A,
    B,
    Select,
    Start,
}

impl Button {
    /// (is_dpad_column, active-low line mask within the low nibble).
    fn line(self) -> (bool, u8) {
        match self {
            Button::Right => (true, 0x01),
            Button::Left => (true, 0x02),
            Button::Up => (true, 0x04),
            Button::Down => (true, 0x08),
            Button::A => (false, 0x01),
            Button::B => (false, 0x02),
            Button::Select => (false, 0x04),
            Button::Start => (false, 0x08),
        }
    }
}

/// SGB command packets are 16 bytes; commands span 1-7 packets (the
/// 3-bit length field of the first header byte).
const SGB_PACKET_BYTES: usize = 16;
const SGB_COMMAND_MAX: usize = SGB_PACKET_BYTES * 7;

/// ICD2-side state of the SGB: the command-packet receiver and the
/// MLT_REQ multiplayer joypad multiplexer. Faithful port of SameBoy
/// Core/sgb.c (`GB_sgb_write` / the `MLT_REQ` case of `command_ready`);
/// only MLT_REQ is executed — every other SGB command affects the
/// SNES-side presentation (palettes, borders, sound) and has no effect
/// observable from the Game Boy bus.
#[derive(Clone)]
struct Sgb {
    /// Accumulated command bits (LSB-first within each byte).
    command: [u8; SGB_COMMAND_MAX],
    /// Bits received into `command` so far.
    write_index: usize,
    /// A $30 write (both lines high) arms the next pulse.
    ready_for_pulse: bool,
    /// A $00 reset pulse opened a packet.
    ready_for_write: bool,
    /// 128 bits of the current packet received: the next pulse must be the
    /// "0" stop bit ("1" is a corrupt packet).
    ready_for_stop: bool,
    /// Active controller count: 1, 2 or 4 — or the glitched 3 of the
    /// unsupported MLT_REQ mode 2.
    player_count: u8,
    /// Currently selected controller, 0-based.
    current_player: u8,
}

impl Sgb {
    fn new() -> Self {
        Self {
            command: [0; SGB_COMMAND_MAX],
            write_index: 0,
            // Idle line; the post-boot P1 = $30 hwio replay arms
            // `ready_for_pulse` like the boot ROM's header transfer left it.
            ready_for_pulse: false,
            ready_for_write: false,
            ready_for_stop: false,
            player_count: 1,
            current_player: 0,
        }
    }

    /// True when JOYP reads with both select lines high must return the
    /// joypad ID instead of idle key lines.
    fn multiplayer(&self) -> bool {
        self.player_count > 1
    }

    /// Observe a P1 write: `old`/`new` are the select bits (masked 0x30).
    fn joyp_write(&mut self, old: u8, new: u8) {
        // Pan Docs MLT_REQ: "The next joypad is automatically selected
        // when P15 goes from LOW (0) to HIGH (1)" — JOYP bit 5 rising.
        // SameBoy gates the increment on an even player count (single
        // player and the glitched 3-player mode never advance) and masks
        // with count-1.
        if new & 0x20 != 0 && old & 0x20 == 0 && self.player_count & 1 == 0 {
            self.current_player = (self.current_player + 1) & (self.player_count - 1);
        }
        // Pulse decoding (Pan Docs "SGB Command Packet"; SameBoy
        // GB_sgb_write): $00 = reset/start, $20 (P14 low) = "0" bit,
        // $10 (P15 low) = "1" bit, $30 = idle between pulses.
        match new >> 4 {
            0x3 => self.ready_for_pulse = true,
            0x2 => self.zero_pulse(),
            0x1 => self.one_pulse(),
            _ => self.reset_pulse(),
        }
    }

    /// Total bit length of the command being received, from the first
    /// header byte's 3-bit length field (0 acts as 1). The SGB boot ROM's
    /// $F1-family header commands are always a single packet despite their
    /// length bits (SameBoy GB_sgb_write).
    fn command_size_bits(&self) -> usize {
        if self.command[0] & 0xF1 == 0xF1 {
            return SGB_PACKET_BYTES * 8;
        }
        let packets = match self.command[0] & 7 {
            0 => 1,
            n => usize::from(n),
        };
        packets * SGB_PACKET_BYTES * 8
    }

    fn discard_command(&mut self) {
        self.write_index = 0;
        self.command = [0; SGB_COMMAND_MAX];
    }

    /// "0" pulse: a data bit, or the stop bit closing a packet.
    fn zero_pulse(&mut self) {
        if !self.ready_for_pulse || !self.ready_for_write {
            return;
        }
        if self.ready_for_stop {
            if self.write_index == self.command_size_bits() {
                self.command_ready();
                self.discard_command();
            }
            self.ready_for_pulse = false;
            self.ready_for_write = false;
            self.ready_for_stop = false;
        } else if self.write_index < SGB_COMMAND_MAX * 8 {
            self.write_index += 1;
            self.ready_for_pulse = false;
            if self.write_index % (SGB_PACKET_BYTES * 8) == 0 {
                self.ready_for_stop = true;
            }
        }
    }

    /// "1" pulse: a data bit; in stop position it corrupts the packet.
    fn one_pulse(&mut self) {
        if !self.ready_for_pulse || !self.ready_for_write {
            return;
        }
        if self.ready_for_stop {
            // Corrupt packet: dropped wholesale (SameBoy logs and resets).
            self.ready_for_pulse = false;
            self.ready_for_write = false;
            self.discard_command();
        } else if self.write_index < SGB_COMMAND_MAX * 8 {
            self.command[self.write_index / 8] |= 1 << (self.write_index & 7);
            self.write_index += 1;
            self.ready_for_pulse = false;
            if self.write_index % (SGB_PACKET_BYTES * 8) == 0 {
                self.ready_for_stop = true;
            }
        }
    }

    /// "$00" pulse: opens a packet; off a packet boundary it restarts the
    /// whole command.
    fn reset_pulse(&mut self) {
        if !self.ready_for_pulse {
            return;
        }
        self.ready_for_write = true;
        self.ready_for_pulse = false;
        if self.write_index % (SGB_PACKET_BYTES * 8) != 0
            || self.write_index == 0
            || self.ready_for_stop
        {
            self.discard_command();
            self.ready_for_stop = false;
        }
    }

    /// A complete command arrived; execute the ones with Game-Boy-visible
    /// effects (header byte 0: command number * 8 + packet length).
    fn command_ready(&mut self) {
        if self.command[0] >> 3 != 0x11 {
            return;
        }
        // MLT_REQ (Pan Docs "SGB Command $11 — MLT_REQ"): data bit 0
        // enables multiplayer, bit 1 selects four players; changing modes
        // masks the current player with the new count - 1. Mode 2 is
        // unsupported by the SGB system software and glitches: player
        // count 3 (odd, so joypad-ID increments stop) with the current
        // player mapped (p + 1) & 2 — pinned empirically by the
        // hardware-verified expectations of SameSuite sgb/command_mlt_req
        // ("glitched player 3" rows: at-command players 1,2,3,0 read back
        // as IDs 2,2,0,0).
        match self.command[1] & 3 {
            0 => {
                self.player_count = 1;
                self.current_player = 0;
            }
            1 => {
                self.player_count = 2;
                self.current_player &= 1;
            }
            3 => {
                self.player_count = 4;
                self.current_player &= 3;
            }
            _ => {
                self.player_count = 3;
                self.current_player = (self.current_player + 1) & 2;
            }
        }
    }
}

#[derive(Clone)]
pub struct Joypad {
    /// P1 bits 4-5 as last written (active low; 1 = column not selected).
    select: u8,
    /// D-pad column, active low (bit 0 Right, 1 Left, 2 Up, 3 Down).
    dpad: u8,
    /// Button column, active low (bit 0 A, 1 B, 2 Select, 3 Start).
    buttons: u8,
    /// Latched IF bits not yet collected by `take_irq`.
    irq: u8,
    /// SGB packet/multiplayer port, present on SGB/SGB2 when the cartridge
    /// header unlocks SGB functions (`Cartridge::supports_sgb`).
    sgb: Option<Sgb>,
}

impl Joypad {
    /// `sgb` enables the ICD2 command-packet/multiplayer port (SGB/SGB2
    /// with an SGB-flagged cartridge only).
    pub fn new(sgb: bool) -> Self {
        Self {
            // Both columns selected: P1 reads 0xCF with nothing pressed,
            // the DMG/CGB post-boot value.
            select: 0x00,
            dpad: 0x0F,
            buttons: 0x0F,
            irq: 0,
            sgb: sgb.then(Sgb::new),
        }
    }

    /// The P10-P13 input lines: AND of every selected column, 1 when idle.
    fn input_lines(&self) -> u8 {
        let mut lines = 0x0F;
        if self.select & 0x10 == 0 {
            lines &= self.dpad;
        }
        if self.select & 0x20 == 0 {
            lines &= self.buttons;
        }
        lines
    }

    /// Latch the joypad interrupt on any 1 -> 0 input line transition.
    /// `before` is a prior `input_lines()` value, so both operands are
    /// already confined to the low nibble.
    fn latch_edges(&mut self, before: u8) {
        if before & !self.input_lines() != 0 {
            self.irq |= 0x10;
        }
    }

    pub fn press(&mut self, b: Button) {
        let before = self.input_lines();
        let (dpad, mask) = b.line();
        if dpad {
            self.dpad &= !mask;
        } else {
            self.buttons &= !mask;
        }
        self.latch_edges(before);
    }

    pub fn release(&mut self, b: Button) {
        let (dpad, mask) = b.line();
        if dpad {
            self.dpad |= mask;
        } else {
            self.buttons |= mask;
        }
        // Releases only produce rising edges; no interrupt.
    }

    /// IF bits requested since the last call (bit 4 = joypad), then clears.
    pub fn take_irq(&mut self) -> u8 {
        let irq = self.irq;
        self.irq = 0;
        irq
    }

    /// Read FF00. Unselected/unused bits read 1. In SGB multiplayer mode a
    /// read with both select lines high returns the joypad ID in the low
    /// nibble — $F for player 1 down to $C for player 4 (Pan Docs
    /// "Reading Multiple Controllers").
    pub fn read(&self) -> u8 {
        if self.select == 0x30 {
            if let Some(sgb) = &self.sgb {
                if sgb.multiplayer() {
                    return 0xF0 | (0x0F - sgb.current_player);
                }
            }
        }
        0xC0 | self.select | self.input_lines()
    }

    /// Write FF00 (select lines only; on SGB the ICD2 snoops the write).
    pub fn write(&mut self, value: u8) {
        let before = self.input_lines();
        if let Some(sgb) = &mut self.sgb {
            sgb.joyp_write(self.select, value & 0x30);
        }
        self.select = value & 0x30;
        // Newly exposing a held button drops a P1 line: interrupt.
        self.latch_edges(before);
    }
}

impl Default for Joypad {
    fn default() -> Self {
        Self::new(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_boot_read_is_cf() {
        // Both columns selected, nothing pressed (DMG/CGB post-boot P1).
        assert_eq!(Joypad::new(false).read(), 0xCF);
    }

    #[test]
    fn deselected_columns_read_all_ones() {
        let mut j = Joypad::new(false);
        j.write(0x30);
        j.press(Button::A);
        j.press(Button::Down);
        assert_eq!(j.read(), 0xFF);
    }

    #[test]
    fn only_select_bits_are_writable() {
        let mut j = Joypad::new(false);
        j.write(0xCF); // bits 0-3 and 6-7 ignored
        assert_eq!(j.read(), 0xCF);
        j.write(0xFF);
        assert_eq!(j.read(), 0xFF);
    }

    #[test]
    fn dpad_press_reads_active_low_and_raises_irq() {
        let mut j = Joypad::new(false);
        j.write(0x20); // select d-pad column only
        j.press(Button::Right);
        assert_eq!(j.read(), 0xEE); // bit 0 low
        assert_eq!(j.take_irq(), 0x10);
        assert_eq!(j.take_irq(), 0, "take_irq clears the latch");
    }

    #[test]
    fn button_press_reads_active_low_and_raises_irq() {
        let mut j = Joypad::new(false);
        j.write(0x10); // select button column only
        j.press(Button::Start);
        assert_eq!(j.read(), 0xD7); // bit 3 low
        assert_eq!(j.take_irq(), 0x10);
    }

    #[test]
    fn unselected_press_no_irq_until_column_selected() {
        let mut j = Joypad::new(false);
        j.write(0x30); // nothing selected
        j.press(Button::A);
        assert_eq!(j.read(), 0xFF);
        assert_eq!(j.take_irq(), 0);
        // Selecting the button column exposes the held A: line falls -> IRQ.
        j.write(0x10);
        assert_eq!(j.read(), 0xDE);
        assert_eq!(j.take_irq(), 0x10);
    }

    #[test]
    fn release_restores_line_without_irq() {
        let mut j = Joypad::new(false);
        j.write(0x20);
        j.press(Button::Up);
        j.take_irq();
        j.release(Button::Up);
        assert_eq!(j.read(), 0xEF);
        assert_eq!(j.take_irq(), 0);
    }

    #[test]
    fn both_columns_selected_are_anded() {
        let mut j = Joypad::new(false);
        j.write(0x00);
        j.press(Button::Right); // d-pad bit 0
        j.press(Button::B); // button bit 1
        assert_eq!(j.read(), 0xCC); // 0b1110 & 0b1101 = 0b1100
    }

    #[test]
    fn repeated_press_does_not_relatch_irq() {
        let mut j = Joypad::new(false);
        j.write(0x10);
        j.press(Button::A);
        assert_eq!(j.take_irq(), 0x10);
        j.press(Button::A); // line already low: no new edge
        assert_eq!(j.take_irq(), 0);
    }

    #[test]
    fn deselecting_produces_no_irq() {
        let mut j = Joypad::new(false);
        j.write(0x10);
        j.press(Button::A);
        j.take_irq();
        j.write(0x30); // line rises: no IRQ
        assert_eq!(j.take_irq(), 0);
    }

    #[test]
    fn impossible_dpad_combo_passes_through() {
        // Hardware cannot reject Left+Right; the frontend may send it and
        // the matrix reports it honestly.
        let mut j = Joypad::new(false);
        j.write(0x20);
        j.press(Button::Left);
        j.press(Button::Right);
        assert_eq!(j.read(), 0xEC);
    }

    // ---- SGB command packets / MLT_REQ ----

    /// SGB-enabled joypad as the post-boot hwio replay leaves it: the boot
    /// ROM's header transfer ended with the line idle, P1 = $30
    /// (model::HWIO_SGB), which arms the packet receiver.
    fn sgb_joypad() -> Joypad {
        let mut j = Joypad::new(true);
        j.write(0x30);
        j
    }

    /// Send one 16-byte command packet exactly like SameSuite's
    /// `SendSgbPacket` (sgb/command_mlt_req.asm): reset pulse, 128 data
    /// bits LSB-first ("1" = P15 low = $10, "0" = P14 low = $20, each
    /// followed by $30), then a "0" stop bit.
    fn send_packet(j: &mut Joypad, data: &[u8; 16]) {
        j.write(0x00);
        j.write(0x30);
        for &byte in data {
            for bit in 0..8 {
                j.write(if byte >> bit & 1 != 0 { 0x10 } else { 0x20 });
                j.write(0x30);
            }
        }
        j.write(0x20);
        j.write(0x30);
    }

    /// MLT_REQ packet: command $11, length 1, one data byte.
    fn mlt_req(mode: u8) -> [u8; 16] {
        let mut p = [0u8; 16];
        p[0] = 0x89;
        p[1] = mode;
        p
    }

    /// One joypad-ID increment: P15 low then high (SameSuite `Increment`).
    fn sgb_increment(j: &mut Joypad) {
        j.write(0x10);
        j.write(0x30);
    }

    /// Full trace of SameSuite sgb/command_mlt_req.asm: every `ldff a,(rP1)`
    /// of the ROM in order, against its hardware-verified CorrectResults
    /// table. Covers mode switches, ID increments, the per-packet
    /// increments ("before it gets ANDed"), and the glitched mode 2.
    #[test]
    fn sgb_command_mlt_req_trace() {
        let mut j = sgb_joypad();
        let mut results = Vec::new();

        send_packet(&mut j, &mlt_req(1));
        results.push(j.read());
        sgb_increment(&mut j);
        results.push(j.read());

        send_packet(&mut j, &mlt_req(0));
        send_packet(&mut j, &mlt_req(1));
        results.push(j.read());

        send_packet(&mut j, &mlt_req(0));
        send_packet(&mut j, &mlt_req(2));
        results.push(j.read());
        sgb_increment(&mut j);
        results.push(j.read());

        send_packet(&mut j, &mlt_req(0));
        send_packet(&mut j, &mlt_req(3));
        results.push(j.read());
        for _ in 0..3 {
            sgb_increment(&mut j);
            results.push(j.read());
        }

        // Switching 4 -> 2 players; the MLT_REQ_1 packet itself increments
        // the player 5 times (reset edge + four "1" bits) before the new
        // count masks it.
        for increments in 0..4 {
            send_packet(&mut j, &mlt_req(0));
            send_packet(&mut j, &mlt_req(3));
            for _ in 0..increments {
                sgb_increment(&mut j);
            }
            send_packet(&mut j, &mlt_req(1));
            results.push(j.read());
        }

        // How many times sending a packet increments: MLT_REQ_3 carries
        // six edges (reset + five "1" bits).
        send_packet(&mut j, &mlt_req(0));
        send_packet(&mut j, &mlt_req(3));
        results.push(j.read());
        send_packet(&mut j, &mlt_req(3));
        results.push(j.read());

        // Glitched mode 2 entered from 4-player mode with players 0-3.
        for increments in 0..4 {
            send_packet(&mut j, &mlt_req(0));
            send_packet(&mut j, &mlt_req(3));
            for _ in 0..increments {
                sgb_increment(&mut j);
            }
            send_packet(&mut j, &mlt_req(2));
            results.push(j.read());
        }

        // Incrementing within the glitched mode (no effect: odd count).
        send_packet(&mut j, &mlt_req(0));
        send_packet(&mut j, &mlt_req(3));
        send_packet(&mut j, &mlt_req(2));
        sgb_increment(&mut j);
        results.push(j.read());
        sgb_increment(&mut j);
        results.push(j.read());

        send_packet(&mut j, &mlt_req(0));
        send_packet(&mut j, &mlt_req(3));
        sgb_increment(&mut j);
        send_packet(&mut j, &mlt_req(2));
        sgb_increment(&mut j);
        results.push(j.read());
        sgb_increment(&mut j);
        results.push(j.read());

        send_packet(&mut j, &mlt_req(0));
        send_packet(&mut j, &mlt_req(3));
        sgb_increment(&mut j);
        sgb_increment(&mut j);
        send_packet(&mut j, &mlt_req(2));
        sgb_increment(&mut j);
        results.push(j.read());

        // CorrectResults of sgb/command_mlt_req.asm (hardware-verified).
        assert_eq!(
            results,
            [
                0xFF, 0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE, 0xFD, //
                0xFC, 0xFE, 0xFF, 0xFE, 0xFF, 0xFF, 0xFD, 0xFD, //
                0xFD, 0xFF, 0xFF, 0xFD, 0xFD, 0xFD, 0xFD, 0xFF,
            ]
        );
    }

    /// Full trace of SameSuite sgb/command_mlt_req_1_incrementing.asm: the
    /// joypad ID advances exactly on writes taking P15 from low to high,
    /// whatever P14 does.
    #[test]
    fn sgb_mlt_req_increment_is_p15_rising_edge() {
        let mut j = sgb_joypad();
        send_packet(&mut j, &mlt_req(1));
        let mut results = Vec::new();
        for seq in [
            &[0x10u8, 0x30][..],       // increments
            &[0x20, 0x30],             // does not increment
            &[0x10, 0x20, 0x30],       // increments (once)
            &[0x10, 0x20, 0x10, 0x30], // two edges: wraps back
            &[0x10, 0x10, 0x30],       // increments
            &[0x00, 0x10, 0x30],       // increments
            &[0x10, 0x00, 0x30],       // increments
            &[0x00, 0x30],             // increments
        ] {
            for &v in seq {
                j.write(v);
            }
            results.push(j.read());
        }
        // CorrectResults of sgb/command_mlt_req_1_incrementing.asm.
        assert_eq!(results, [0xFE, 0xFE, 0xFF, 0xFF, 0xFE, 0xFF, 0xFE, 0xFF]);
    }

    /// Single-player mode: reads with both lines high stay plain key
    /// reads, and P15 edges never advance anything.
    #[test]
    fn sgb_single_player_reads_are_plain() {
        let mut j = sgb_joypad();
        sgb_increment(&mut j);
        assert_eq!(j.read(), 0xFF);
        send_packet(&mut j, &mlt_req(1));
        send_packet(&mut j, &mlt_req(0));
        sgb_increment(&mut j);
        assert_eq!(j.read(), 0xFF);
    }

    /// In multiplayer mode, selecting a key column still reads the matrix
    /// (the host joypad is player 1); only the both-lines-high read shows
    /// the ID.
    #[test]
    fn sgb_multiplayer_key_reads_keep_working() {
        let mut j = sgb_joypad();
        send_packet(&mut j, &mlt_req(1));
        j.press(Button::Start);
        j.write(0x10); // select button column
        assert_eq!(j.read(), 0xD7);
        j.write(0x30);
        assert_eq!(j.read() & 0x0F, 0x0E, "ID read: P15 edge advanced to 2");
    }

    /// A "1" pulse in stop-bit position corrupts the packet: the command
    /// must not execute (SameBoy GB_sgb_write case 1).
    #[test]
    fn sgb_corrupt_stop_bit_discards_packet() {
        let mut j = sgb_joypad();
        let p = mlt_req(1);
        j.write(0x00);
        j.write(0x30);
        for &byte in &p {
            for bit in 0..8 {
                j.write(if byte >> bit & 1 != 0 { 0x10 } else { 0x20 });
                j.write(0x30);
            }
        }
        j.write(0x10); // corrupt: "1" where the stop bit belongs
        j.write(0x30);
        assert_eq!(j.read(), 0xFF, "command dropped: still single player");
        // The receiver recovers: a fresh packet works.
        send_packet(&mut j, &mlt_req(1));
        sgb_increment(&mut j);
        assert_eq!(j.read(), 0xFE);
    }

    /// A reset pulse mid-packet restarts the command from scratch.
    #[test]
    fn sgb_reset_pulse_mid_packet_restarts() {
        let mut j = sgb_joypad();
        // Half a packet of "1" bits, then a reset and a full MLT_REQ_1.
        j.write(0x00);
        j.write(0x30);
        for _ in 0..64 {
            j.write(0x10);
            j.write(0x30);
        }
        send_packet(&mut j, &mlt_req(1));
        sgb_increment(&mut j);
        assert_eq!(j.read(), 0xFE, "MLT_REQ executed cleanly after restart");
    }

    /// Non-SGB joypads ignore the packet protocol entirely.
    #[test]
    fn non_sgb_ignores_packets() {
        let mut j = Joypad::new(false);
        j.write(0x30);
        send_packet(&mut j, &mlt_req(1));
        sgb_increment(&mut j);
        assert_eq!(j.read(), 0xFF);
    }
}
