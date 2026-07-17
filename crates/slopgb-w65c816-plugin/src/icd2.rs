//! The ICD2 register block — the SGB cartridge's SNES↔GB interface chip as
//! the SNES CPU sees it at `$6000-$7FFF` (nocash fullsnes, "SGB I/O Map
//! (ICD2-R)"). The read side effects (`$7000` clears the packet flag,
//! `$7800` auto-increments the character-buffer index) must run synchronously
//! with the hosted CPU's bus reads, so the block lives here in the plugin;
//! the host pumps its GB-side halves (packet deposit, pad readback, LCD-row
//! shadow) through the out-of-band host window (`HOST_WIN` in `lib.rs`).
//!
//! See `docs/hardware-state/sgb-icd2.md` for the extracted register spec.

/// One GB LCD character row as the ICD2 serves it at `$7800`: 160×8 pixels
/// re-arranged to 2bpp tiles = 320 bytes (fullsnes "SGB Port 7800h").
pub(crate) const CHAR_ROW_LEN: usize = 320;
/// The four character-row buffers the chip rotates through.
pub(crate) const CHAR_ROWS: usize = 4;
/// Serialized [`Icd2::save_state`] length.
pub(crate) const ICD2_STATE_LEN: usize =
    16 + 1 + 4 + 1 + 1 + 1 + 1 + 1 + CHAR_ROWS * CHAR_ROW_LEN + 2;

/// ICD2-R register state. CPU-side accesses come from the hosted 65C816's
/// bus (`cpu_read`/`cpu_write`); `host_*` methods are the GB-side halves the
/// orchestrating host drives between `run_until` slices.
pub(crate) struct Icd2 {
    /// The 16-byte command packet readable at `$7000-$700F`.
    packet: [u8; 16],
    /// `$6002` bit 0: a new packet is available (cleared by a `$7000` read).
    available: bool,
    /// `$6004-$6007` controller latches, players 1-4 (active low).
    pads: [u8; 4],
    /// Sticky: the program wrote at least one pad latch since reset — the
    /// host's signal that the SNES side has taken over the joypad.
    pads_written: bool,
    /// `$6001` bits 1-0: the character-buffer row selected for `$7800` reads.
    read_row: u8,
    /// `$6000` bits 1-0: the row currently being written (host shadow).
    write_row: u8,
    /// `$6000` bits 7-3: the current GB LCD character row (host shadow).
    lcd_row: u8,
    /// `$6003` as last written (reset/multiplayer/speed — capture only; the
    /// GB-reset bit is host-visible, never wired to the GB core).
    control: u8,
    /// The four 320-byte character-row buffers `$7800` serves.
    char_buf: [[u8; CHAR_ROW_LEN]; CHAR_ROWS],
    /// `$7800` read index within the selected row, 0-511 (reset by `$6001`).
    buf_idx: u16,
}

impl Icd2 {
    pub(crate) fn new() -> Self {
        Icd2 {
            packet: [0; 16],
            available: false,
            // Idle pads (nothing pressed, active low) until the program writes.
            pads: [0xFF; 4],
            pads_written: false,
            read_row: 0,
            write_row: 0,
            lcd_row: 0,
            // Power-on control: GB not held in reset, default speed 1 = 4MHz
            // (fullsnes "SGB Port 6003h": "default 1=4MHz").
            control: 0x01,
            char_buf: [[0; CHAR_ROW_LEN]; CHAR_ROWS],
            buf_idx: 0,
        }
    }

    /// A CPU read anywhere in `$6000-$7FFF`. The chip decodes only A0-A3 and
    /// A11-A15 (fullsnes SGB I/O map), so `addr & 0xF800` picks the block and
    /// `addr & 0xF` the register, mirroring each block every 16 bytes.
    pub(crate) fn cpu_read(&mut self, addr: u16) -> u8 {
        let reg = (addr & 0xF) as usize;
        match addr & 0xF800 {
            0x6000 => match reg {
                // $6000 (LCD char row + write buffer); $6001/$6004/$6005 read
                // as its mirror on [$600F]=21h chips (fullsnes garbage table).
                0x0 | 0x1 | 0x4 | 0x5 => (self.lcd_row << 3) | self.write_row,
                // $6002 (packet available); $6003/$6006/$6007 mirror it.
                0x2 | 0x3 | 0x6 | 0x7 => u8::from(self.available),
                // $600F chip version 21h; $6008-$600E mirror it.
                _ => 0x21,
            },
            // $6800-$680F unused (open bus) — modeled as 0.
            0x6800 => 0,
            0x7000 => {
                // "Reading from 7000h (but not from 7001h-700Fh) does reset
                // the flag in 6002h" (fullsnes "SGB Port 7000h-700Fh").
                if reg == 0 {
                    self.available = false;
                }
                self.packet[reg]
            }
            // $7800 character-buffer data; $7801-$780F are mirrors of the
            // same port (not open bus), sharing the auto-increment.
            _ => {
                let idx = usize::from(self.buf_idx);
                self.buf_idx = (self.buf_idx + 1) & 511;
                if idx < CHAR_ROW_LEN {
                    self.char_buf[usize::from(self.read_row)][idx]
                } else {
                    // Indices 320-511 read $FF (black pixels).
                    0xFF
                }
            }
        }
    }

    /// A CPU write anywhere in `$6000-$7FFF` (only the `$6000` block has
    /// writable registers; the mailbox and character buffer are read-only).
    pub(crate) fn cpu_write(&mut self, addr: u16, v: u8) {
        if addr & 0xF800 != 0x6000 {
            return;
        }
        match (addr & 0xF) as usize {
            // "The buffer index is reset to 0 upon writing to Port 6001h."
            0x1 => {
                self.read_row = v & 3;
                self.buf_idx = 0;
            }
            0x3 => self.control = v,
            r @ 0x4..=0x7 => {
                self.pads[r - 4] = v;
                self.pads_written = true;
            }
            _ => {}
        }
    }

    // -- Host (GB-side) halves, pumped between run_until slices -------------

    /// Latch a fresh 16-byte packet into the mailbox and raise `$6002`.
    /// Depositing over an unread packet overwrites it (as the wire would);
    /// the host's pump avoids that by checking [`Self::packet_pending`]
    /// before each deposit.
    pub(crate) fn host_deposit_packet(&mut self, bytes: &[u8; 16]) {
        self.packet = *bytes;
        self.available = true;
    }

    /// Whether the mailbox still holds an unread packet (`$6002` bit 0).
    pub(crate) fn packet_pending(&self) -> bool {
        self.available
    }

    /// The pad latches + the sticky program-wrote-them flag.
    pub(crate) fn host_pads(&self) -> ([u8; 4], bool) {
        (self.pads, self.pads_written)
    }

    /// Refresh the `$6000` shadows (current LCD character row + write buffer).
    pub(crate) fn host_set_lcd_row(&mut self, lcd_row: u8, write_row: u8) {
        self.lcd_row = lcd_row & 0x1F;
        self.write_row = write_row & 3;
    }

    /// The last `$6003` control write (reset/multiplayer/speed bits).
    pub(crate) fn host_control(&self) -> u8 {
        self.control
    }

    /// Load one 320-byte character row (the GB screen path the SNES DMAs
    /// from `$7800`). Short data leaves the row's tail unchanged.
    pub(crate) fn host_load_char_row(&mut self, row: usize, data: &[u8]) {
        let row = &mut self.char_buf[row & (CHAR_ROWS - 1)];
        let n = data.len().min(CHAR_ROW_LEN);
        row[..n].copy_from_slice(&data[..n]);
    }

    // -- Save state ----------------------------------------------------------

    pub(crate) fn save_state(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.packet);
        buf.push(u8::from(self.available));
        buf.extend_from_slice(&self.pads);
        buf.push(u8::from(self.pads_written));
        buf.push(self.read_row);
        buf.push(self.write_row);
        buf.push(self.lcd_row);
        buf.push(self.control);
        for row in &self.char_buf {
            buf.extend_from_slice(row);
        }
        buf.extend_from_slice(&self.buf_idx.to_le_bytes());
    }

    /// Restore from exactly [`ICD2_STATE_LEN`] bytes. A wrong-length slice is
    /// ignored (the chip keeps its state) rather than panic — the plugin's
    /// `load_state` length-gates the whole blob, this guards any future
    /// caller.
    pub(crate) fn load_state(&mut self, b: &[u8]) {
        if b.len() != ICD2_STATE_LEN {
            return;
        }
        self.packet.copy_from_slice(&b[..16]);
        self.available = b[16] != 0;
        self.pads.copy_from_slice(&b[17..21]);
        self.pads_written = b[21] != 0;
        self.read_row = b[22] & 3;
        self.write_row = b[23] & 3;
        self.lcd_row = b[24] & 0x1F;
        self.control = b[25];
        for (r, row) in self.char_buf.iter_mut().enumerate() {
            row.copy_from_slice(&b[26 + r * CHAR_ROW_LEN..26 + (r + 1) * CHAR_ROW_LEN]);
        }
        let off = 26 + CHAR_ROWS * CHAR_ROW_LEN;
        self.buf_idx = u16::from_le_bytes([b[off], b[off + 1]]) & 511;
    }
}

#[cfg(test)]
#[path = "icd2_tests.rs"]
mod tests;
