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
const SGB_PACKET_BYTES: usize = crate::sgb::SGB_PACKET_LEN;
const SGB_COMMAND_MAX: usize = SGB_PACKET_BYTES * 7;
/// Max raw packets queued for the SNES-side coprocessor before the oldest is
/// dropped: with no coprocessor attached nothing drains the tee, so it must
/// stay bounded. Real titles space packets ~4 frames apart (Pan Docs "SGB
/// Command Packet"), and the coprocessor pump drains far faster than that.
const SGB_PACKET_QUEUE_CAP: usize = 16;

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
    /// A completed non-MLT_REQ command awaiting forward to the PPU/SGB
    /// presentation layer (drained by [`Joypad::take_sgb_command`]).
    /// Transient — set in `command_ready` and drained within the same P1
    /// write, so it is never serialized.
    pending_cmd: Option<Vec<u8>>,
    /// Raw 16-byte packets teed for the SNES-side coprocessor's ICD2 mailbox
    /// (every accepted packet, MLT_REQ and mid-command packets included — the
    /// real ICD2 latches each packet at `$7000-$700F`, fullsnes "SGB Port
    /// 7000h"). Bounded; oldest dropped. Drained by
    /// [`Joypad::take_sgb_packet`]; serialized (packets may await delivery).
    packets: std::collections::VecDeque<[u8; SGB_PACKET_BYTES]>,
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
            pending_cmd: None,
            packets: std::collections::VecDeque::new(),
        }
    }

    /// Tee the just-accepted 16-byte packet (its stop bit passed) for the
    /// SNES-side coprocessor. `write_index` sits on the packet's end boundary.
    fn tee_packet(&mut self) {
        let end = self.write_index / 8;
        if end < SGB_PACKET_BYTES {
            return;
        }
        let mut p = [0u8; SGB_PACKET_BYTES];
        p.copy_from_slice(&self.command[end - SGB_PACKET_BYTES..end]);
        while self.packets.len() >= SGB_PACKET_QUEUE_CAP {
            self.packets.pop_front();
        }
        self.packets.push_back(p);
    }

    /// True when JOYP reads with both select lines high must return the
    /// joypad ID instead of idle key lines.
    fn multiplayer(&self) -> bool {
        self.player_count > 1
    }

    /// Observe a P1 write: `old`/`new` are the select bits (masked 0x30).
    /// Returns whether this write *started* a new command transfer (the first
    /// reset pulse of a command) — the debugger's "SGB transfer start" edge.
    fn joyp_write(&mut self, old: u8, new: u8) -> bool {
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
            0x3 => {
                self.ready_for_pulse = true;
                false
            }
            0x2 => {
                self.zero_pulse();
                false
            }
            0x1 => {
                self.one_pulse();
                false
            }
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
            // The stop bit validated a full 16-byte packet: tee it for the
            // coprocessor before `discard_command` can zero the accumulator.
            self.tee_packet();
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
    fn reset_pulse(&mut self) -> bool {
        if !self.ready_for_pulse {
            return false;
        }
        // A reset pulse that discards the accumulator opens a fresh command
        // transfer (write_index 0 / mid-packet garbage / after a stop) — the
        // "SGB transfer start" edge; an inter-packet reset mid-command does not.
        let started = self.write_index % (SGB_PACKET_BYTES * 8) != 0
            || self.write_index == 0
            || self.ready_for_stop;
        self.ready_for_write = true;
        self.ready_for_pulse = false;
        if started {
            self.discard_command();
            self.ready_for_stop = false;
        }
        started
    }

    /// A complete command arrived; execute the ones with Game-Boy-visible
    /// effects (header byte 0: command number * 8 + packet length).
    fn command_ready(&mut self) {
        if self.command[0] >> 3 != 0x11 {
            // Every non-MLT_REQ command drives the SNES-side presentation
            // (palettes, attributes, mask, borders, sound). Stash the
            // completed bytes for the interconnect to forward to the PPU/SGB
            // layer; MLT_REQ (the only Game-Boy-bus-visible command) is
            // executed below. `discard_command` zeroes the buffer after this
            // returns, so capture the exact command length now.
            let len = self.command_size_bits() / 8;
            self.pending_cmd = Some(self.command[..len].to_vec());
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

    /// Whether `b` is currently held, for the debugger's joypad view.
    pub fn pressed(&self, b: Button) -> bool {
        let (dpad, mask) = b.line();
        let col = if dpad { self.dpad } else { self.buttons };
        col & mask == 0
    }

    /// IF bits requested since the last call (bit 4 = joypad), then clears.
    pub fn take_irq(&mut self) -> u8 {
        let irq = self.irq;
        self.irq = 0;
        irq
    }

    /// Drain a completed non-MLT_REQ SGB command for the interconnect to
    /// forward to the PPU/SGB presentation layer. `None` on a non-SGB joypad
    /// or when no command has completed since the last drain.
    pub(crate) fn take_sgb_command(&mut self) -> Option<Vec<u8>> {
        self.sgb.as_mut().and_then(|s| s.pending_cmd.take())
    }

    /// Drain one raw teed 16-byte packet for the SNES-side coprocessor's ICD2
    /// mailbox, oldest first. `None` on a non-SGB joypad or an empty queue.
    pub(crate) fn take_sgb_packet(&mut self) -> Option<[u8; SGB_PACKET_BYTES]> {
        self.sgb.as_mut().and_then(|s| s.packets.pop_front())
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
    /// Returns whether an SGB command transfer just started (always `false` off
    /// SGB) — the "SGB transfer start" exception edge.
    pub fn write(&mut self, value: u8) -> bool {
        let before = self.input_lines();
        let sgb_started = match &mut self.sgb {
            Some(sgb) => sgb.joyp_write(self.select, value & 0x30),
            None => false,
        };
        self.select = value & 0x30;
        // Newly exposing a held button drops a P1 line: interrupt.
        self.latch_edges(before);
        sgb_started
    }
}

impl Default for Joypad {
    fn default() -> Self {
        Self::new(false)
    }
}

// --- Save state (manual serialization; see `crate::state`) ---
impl Joypad {
    pub(crate) fn write_state(&self, w: &mut crate::state::Writer) {
        w.u8(self.select);
        w.u8(self.dpad);
        w.u8(self.buttons);
        w.u8(self.irq);
        match &self.sgb {
            Some(s) => {
                w.bool(true);
                w.bytes(&s.command);
                w.u32(s.write_index as u32);
                w.bool(s.ready_for_pulse);
                w.bool(s.ready_for_write);
                w.bool(s.ready_for_stop);
                w.u8(s.player_count);
                w.u8(s.current_player);
                w.u8(s.packets.len() as u8);
                for p in &s.packets {
                    w.bytes(p);
                }
            }
            None => w.bool(false),
        }
    }
    pub(crate) fn read_state(
        &mut self,
        r: &mut crate::state::Reader<'_>,
    ) -> Result<(), crate::state::StateError> {
        self.select = r.u8()?;
        self.dpad = r.u8()?;
        self.buttons = r.u8()?;
        self.irq = r.u8()?;
        if r.bool()? {
            let mut command = [0u8; SGB_COMMAND_MAX];
            r.bytes_into(&mut command)?;
            let write_index = r.u32()? as usize;
            let ready_for_pulse = r.bool()?;
            let ready_for_write = r.bool()?;
            let ready_for_stop = r.bool()?;
            let player_count = r.u8()?;
            let current_player = r.u8()?;
            // A corrupt save with player_count 0 would underflow `player_count
            // - 1` in `joyp_write` (the SGB player-cycle mask) → a debug-build
            // panic *after* a successful load. Reject out-of-range values so a
            // bad state can never arm a later panic (state contract: never a
            // panic on a corrupt save). Real SGB carts only ever store 1/2/4.
            if player_count == 0 || player_count > 4 || current_player >= player_count {
                return Err(crate::state::StateError::Truncated);
            }
            let n = usize::from(r.u8()?);
            // A count past the cap cannot come from `write_state`; reject it
            // rather than let a corrupt save allocate/loop on garbage.
            if n > SGB_PACKET_QUEUE_CAP {
                return Err(crate::state::StateError::Truncated);
            }
            let mut packets = std::collections::VecDeque::with_capacity(n);
            for _ in 0..n {
                let mut p = [0u8; SGB_PACKET_BYTES];
                r.bytes_into(&mut p)?;
                packets.push_back(p);
            }
            self.sgb = Some(Sgb {
                command,
                write_index,
                ready_for_pulse,
                ready_for_write,
                ready_for_stop,
                player_count,
                current_player,
                pending_cmd: None,
                packets,
            });
        } else {
            self.sgb = None;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "joypad_tests.rs"]
mod tests;
