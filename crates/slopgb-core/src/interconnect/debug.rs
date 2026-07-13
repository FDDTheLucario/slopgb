//! Debugger-only inherent methods on [`Interconnect`]: memory watchpoints (RM8),
//! the execution profiler (MB5), and the exception-break checks (Options →
//! Exceptions). Every one is a live-debugger control that defaults inert and is
//! never exercised on a golden/test path, so the fingerprint stays
//! byte-identical. Interconnect work package.

use super::*;
use crate::{EXC_ECHO_RAM, EXC_INVALID_OPCODE, EXC_LCD_OFF_VBLANK, EXC_LD_B_B};

impl Interconnect {
    /// Per-access debugger check on a CPU bus access: memory watchpoints (RM8)
    /// and the echo-RAM exception break. Both halves early-out when their
    /// feature is unarmed (empty watch list / `exc_mask == 0`), so this is a
    /// no-op on every golden path (golden-safe). Replaces the former
    /// `check_watch`, called from the ticked `Bus` read/read_inc/write.
    pub(super) fn check_access(&mut self, addr: u16, is_write: bool) {
        // CDL: record a CPU read/write of this byte (R=1, W=2). `None` when the
        // log is off → no-op, so the golden path is byte-identical.
        self.cdl_mark(addr, if is_write { 2 } else { 1 });
        if !self.watchpoints.is_empty()
            && self
                .watchpoints
                .iter()
                .any(|w| w.addr == addr && if is_write { w.write } else { w.read })
        {
            self.watch_hit = Some(addr);
        }
        // Echo RAM is C000-DDFF mirrored at E000-FDFF; any CPU access there is
        // bgb's "break on ram echo (E000-FDFF) access".
        if self.exc_mask & EXC_ECHO_RAM != 0 && (0xE000..=0xFDFF).contains(&addr) {
            self.exc_hit = Some(addr);
        }
    }

    /// Exception break on a write: disabling the LCD (`FF40` bit 7 → 0) while it
    /// is on and the PPU is outside vblank (mode ≠ 1). The caller passes the
    /// *new* value before committing it, so `lcd_enabled()` still reads the old
    /// LCDC. Inert when the bit is unarmed.
    pub(super) fn check_exc_lcd(&mut self, addr: u16, value: u8) {
        if self.exc_mask & EXC_LCD_OFF_VBLANK != 0
            && addr == 0xFF40
            && value & 0x80 == 0
            && self.ppu.lcd_enabled()
            && self.ppu.mode_bits() != 1
        {
            self.exc_hit = Some(addr);
        }
    }

    /// Exception break on the opcode about to execute at `pc`: `LD B,B` (`40h`)
    /// or an undefined opcode. The undefined set is exactly the 11 opcodes the
    /// CPU hard-locks on (`cpu::execute`). Inert when no opcode exception is
    /// armed (`exc_mask == 0`).
    pub(super) fn exec_exception(&mut self, pc: u16, opcode: u8) {
        if self.exc_mask & (EXC_LD_B_B | EXC_INVALID_OPCODE) == 0 {
            return;
        }
        let hit = (self.exc_mask & EXC_LD_B_B != 0 && opcode == 0x40)
            || (self.exc_mask & EXC_INVALID_OPCODE != 0
                && matches!(
                    opcode,
                    0xD3 | 0xDB | 0xDD | 0xE3 | 0xE4 | 0xEB | 0xEC | 0xED | 0xF4 | 0xFC | 0xFD
                ));
        if hit {
            self.exc_hit = Some(pc);
        }
    }

    /// Set the debugger exception-break mask (the `EXC_*` bits). `0` disarms
    /// every check (golden-safe). Clears any pending hit (like
    /// [`Self::set_watchpoints`]) so re-arming can't replay a stale one.
    /// Live-debugger-only.
    pub fn set_exceptions(&mut self, mask: u16) {
        self.exc_mask = mask;
        self.exc_hit = None;
    }

    /// The current exception-break mask (`0` when nothing is armed).
    pub fn exceptions(&self) -> u16 {
        self.exc_mask
    }

    /// Take the pending exception-break hit address (cleared by the read).
    pub fn take_exc_hit(&mut self) -> Option<u16> {
        self.exc_hit.take()
    }

    /// Replace the debugger memory watchpoints (RM8). Empty disables the
    /// access-path check entirely (golden-safe).
    pub fn set_watchpoints(&mut self, wps: &[crate::Watchpoint]) {
        self.watchpoints = wps.to_vec();
        self.watch_hit = None;
    }

    /// Take the pending watchpoint hit address (cleared by the read).
    pub fn take_watch_hit(&mut self) -> Option<u16> {
        self.watch_hit.take()
    }

    /// Enable/disable the execution profiler (MB5). Enabling allocates the tally
    /// (preserving an existing one); disabling drops it and any break-mode state.
    /// Live-debugger-only.
    pub fn set_profiling(&mut self, on: bool) {
        match (on, self.prof.is_some()) {
            (true, false) => self.prof = Some(std::collections::BTreeMap::new()),
            (false, true) => {
                self.prof = None;
                self.prof_break = false;
                self.prof_break_hit = None;
            }
            _ => {}
        }
    }

    /// Arm/disarm profiler break mode (halt the free run on each address's first
    /// execution). Only meaningful while profiling is on.
    pub fn set_profile_break(&mut self, on: bool) {
        self.prof_break = on;
        if !on {
            self.prof_break_hit = None;
        }
    }

    /// Whether profiler break mode is armed.
    pub fn profile_break(&self) -> bool {
        self.prof_break
    }

    /// Take the pending break-mode hit address (cleared by the read).
    pub fn take_prof_break_hit(&mut self) -> Option<u16> {
        self.prof_break_hit.take()
    }

    /// Zero the profiler tally without disabling logging (bgb's "clear buffer").
    pub fn clear_profile(&mut self) {
        if let Some(m) = &mut self.prof {
            m.clear();
        }
    }

    /// Cumulative base offsets of the bank-aware CDL buffer's physical regions
    /// (ROM | VRAM | SRAM | WRAM | tail `0xFE00-0xFFFF`) and its total size.
    /// Fixed for a machine's lifetime, so the buffer sized to `total` at
    /// enable/load never needs re-indexing.
    fn cdl_layout(&self) -> CdlLayout {
        let vram = self.cart.rom_len();
        let sram = vram + CDL_VRAM_LEN;
        let wram = sram + self.cart.ram_len();
        let tail = wram + self.wram.len();
        CdlLayout {
            rom: 0,
            vram,
            sram,
            wram,
            total: tail + CDL_TAIL_LEN,
        }
    }

    /// Translate a CPU address to its index in the physical CDL buffer, or
    /// `None` when the access lands on no physical byte (disabled/absent SRAM,
    /// or an RTC register). Shared by the mark hook and `cdl_flag` so the record
    /// and the display can't disagree (the `rom_bank_for` pattern). `&self`.
    fn cdl_index(&self, addr: u16) -> Option<usize> {
        let l = self.cdl_layout();
        Some(match addr {
            0x0000..=0x7FFF => l.rom + self.cart.rom_offset(addr),
            0x8000..=0x9FFF => l.vram + self.ppu.vram_bank() * 0x2000 + usize::from(addr & 0x1FFF),
            0xA000..=0xBFFF => l.sram + self.cart.ram_offset(addr)?,
            0xC000..=0xFDFF => l.wram + self.wram_index(addr),
            // 0xFE00-0xFFFF tail (OAM/IO/HRAM/IE), unbanked.
            _ => l.wram + self.wram.len() + usize::from(addr - 0xFE00),
        })
    }

    /// Mark a CPU access to `addr` with `flag` (R=1/W=2/X=4) in the bank-aware
    /// CDL. The `is_none()` early-out keeps this the same no-op it was when the
    /// log is off, so golden paths stay byte-identical; the index is resolved
    /// via `&self` before the `&mut self.cdl` borrow.
    pub(super) fn cdl_mark(&mut self, addr: u16, flag: u8) {
        if self.cdl.is_none() {
            return;
        }
        if let Some(i) = self.cdl_index(addr) {
            if let Some(b) = &mut self.cdl {
                b[i] |= flag;
            }
        }
    }

    /// Enable/disable the code/data log (CDL). Enabling allocates the physical
    /// flag buffer sized to the machine (preserving an existing one); disabling
    /// drops it. Live-debugger-only, golden-safe (a `None` log is a no-op).
    pub fn set_cdl(&mut self, on: bool) {
        match (on, self.cdl.is_some()) {
            (true, false) => self.cdl = Some(vec![0u8; self.cdl_layout().total].into()),
            (false, true) => self.cdl = None,
            _ => {}
        }
    }

    /// The CDL access flags for the byte `addr` currently maps to (R=1, W=2,
    /// X=4), or 0 when the log is off / the byte is unvisited / no physical byte
    /// is mapped. Follows live banking, so it tints the currently-mapped bank.
    #[must_use]
    pub fn cdl_flag(&self, addr: u16) -> u8 {
        match (&self.cdl, self.cdl_index(addr)) {
            (Some(b), Some(i)) => b[i],
            _ => 0,
        }
    }

    /// Like [`Self::cdl_flag`] but for an **explicit** bank of the three banked
    /// regions (ROMX / VRAM / WRAMX), so the MCP/debug `cdl` tool can inspect a
    /// bank other than the live one. Outside those regions `bank` is meaningless
    /// and this is exactly [`Self::cdl_flag`]. The physical index mirrors
    /// [`Self::cdl_index`]; banks wrap within the region (ROM size is a power of
    /// two, so the modulo matches the mapper's address-line mask). 0 when the log
    /// is off. Side-effect-free (`&self`).
    #[must_use]
    pub fn cdl_flag_banked(&self, bank: u16, addr: u16) -> u8 {
        let Some(b) = &self.cdl else { return 0 };
        let l = self.cdl_layout();
        let idx = match addr {
            0x4000..=0x7FFF => {
                let rom_len = self.cart.rom_len().max(0x4000);
                l.rom + (usize::from(bank) * 0x4000) % rom_len + usize::from(addr & 0x3FFF)
            }
            0x8000..=0x9FFF => l.vram + usize::from(bank & 1) * 0x2000 + usize::from(addr & 0x1FFF),
            0xA000..=0xBFFF => match self.cart.ram_offset_banked(bank, addr) {
                Some(off) => l.sram + off,
                None => return 0,
            },
            0xD000..=0xDFFF => {
                let nbanks = (self.wram.len() / 0x1000).max(1);
                let bk = usize::from(bank).max(1) % nbanks;
                l.wram + bk * 0x1000 + usize::from(addr & 0x0FFF)
            }
            _ => return self.cdl_flag(addr),
        };
        b.get(idx).copied().unwrap_or(0)
    }

    /// The whole physical flag buffer (for a save), or `None` when the log is
    /// off. Its length is `cdl_layout().total` (ROM+VRAM+SRAM+WRAM+tail).
    #[must_use]
    pub fn cdl_flags(&self) -> Option<&[u8]> {
        self.cdl.as_deref()
    }

    /// Every maximal **continuous** span of logged (non-`.`) CPU addresses, one
    /// [`CdlRange`] per span. The inverse of [`Self::cdl_flag_banked`]: it walks
    /// the physical buffer region by region and bank by bank (within one bank
    /// CPU addresses are contiguous, so a run of set bytes is one range),
    /// canonicalising each physical byte to its `(bank, CPU address)`. A range
    /// never crosses a region/bank boundary — exactly where the address form /
    /// bank prefix changes. Empty when the log is off. Read-only, golden-safe.
    #[must_use]
    pub fn cdl_logged_ranges(&self) -> Vec<CdlRange> {
        let Some(buf) = &self.cdl else {
            return Vec::new();
        };
        let l = self.cdl_layout();
        let mut out = Vec::new();
        // ROM: bank 0 → 0x0000-0x3FFF (bare), banks 1.. → 0x4000-0x7FFF.
        for (bank, base) in (0..).zip((l.rom..l.vram).step_by(ROM_BANK)) {
            let cpu_start = if bank == 0 { 0x0000 } else { 0x4000 };
            push_runs(buf, base, ROM_BANK, cpu_start, bank, &mut out);
        }
        // VRAM: both CGB banks → 0x8000-0x9FFF.
        for (bank, base) in (0..).zip((l.vram..l.sram).step_by(VRAM_BANK)) {
            push_runs(buf, base, VRAM_BANK, 0x8000, bank, &mut out);
        }
        // SRAM: banks → 0xA000-0xBFFF.
        for (bank, base) in (0..).zip((l.sram..l.wram).step_by(SRAM_BANK)) {
            push_runs(buf, base, SRAM_BANK, 0xA000, bank, &mut out);
        }
        // WRAM: bank 0 → 0xC000-0xCFFF (bare), banks 1.. → 0xD000-0xDFFF.
        let wram_end = l.wram + self.wram.len();
        for (bank, base) in (0..).zip((l.wram..wram_end).step_by(WRAM_BANK)) {
            let cpu_start = if bank == 0 { 0xC000 } else { 0xD000 };
            push_runs(buf, base, WRAM_BANK, cpu_start, bank, &mut out);
        }
        // Tail: 0xFE00-0xFFFF (OAM/IO/HRAM/IE), unbanked.
        push_runs(buf, wram_end, CDL_TAIL_LEN, 0xFE00, 0, &mut out);
        out
    }

    /// Zero the CDL flags without disabling logging (bgb's "clear buffer").
    pub fn cdl_clear(&mut self) {
        if let Some(b) = &mut self.cdl {
            b.fill(0);
        }
    }

    /// Load a physical CDL flag buffer (a decoded `.cdl` file), enabling the
    /// log. Rejects (returns false) a buffer whose length doesn't match this
    /// machine's layout — i.e. a `.cdl` from a different ROM/RAM configuration.
    #[must_use]
    pub fn load_cdl(&mut self, flags: &[u8]) -> bool {
        if flags.len() != self.cdl_layout().total {
            return false;
        }
        self.cdl = Some(flags.into());
        true
    }

    /// The live WRAM bank mapped at `0xD000-0xDFFF` (CGB SVBK, always 1 on DMG;
    /// `0xC000-0xCFFF` is always bank 0), for the memory-viewer bank indicator.
    #[must_use]
    pub fn wram_bank(&self) -> usize {
        if self.model.is_cgb() {
            usize::from(self.svbk & 7).max(1)
        } else {
            1
        }
    }

    /// The live VRAM bank (CGB VBK; always 0 on DMG), for the viewer indicator.
    #[must_use]
    pub fn vram_bank(&self) -> usize {
        self.ppu.vram_bank()
    }

    /// Whether the profiler is currently logging.
    pub fn profiling(&self) -> bool {
        self.prof.is_some()
    }

    /// Times the instruction at `pc` has executed since the last clear (0 if
    /// unseen or profiling is off).
    pub fn profile_count(&self, pc: u16) -> u64 {
        self.prof
            .as_ref()
            .and_then(|m| m.get(&pc))
            .copied()
            .unwrap_or(0)
    }

    /// Distinct instruction addresses seen since the last clear.
    pub fn profile_seen(&self) -> usize {
        self.prof
            .as_ref()
            .map_or(0, std::collections::BTreeMap::len)
    }
}

/// VRAM slice of the bank-aware CDL buffer: both CGB banks (2×0x2000), so DMG
/// and CGB share one layout.
const CDL_VRAM_LEN: usize = 0x4000;
/// Tail slice covering `0xFE00-0xFFFF` (OAM/unusable/IO/HRAM/IE), unbanked —
/// keeps HRAM-executed code (OAM-DMA wait loops) tinted.
const CDL_TAIL_LEN: usize = 0x200;
/// Physical per-bank slice sizes, one CPU window each (the units the CDL layout
/// and [`Interconnect::cdl_logged_ranges`] step by).
const ROM_BANK: usize = 0x4000;
const VRAM_BANK: usize = 0x2000;
const SRAM_BANK: usize = 0x2000;
const WRAM_BANK: usize = 0x1000;

/// One continuous span of logged CPU addresses, inclusive `[start, end]`, all in
/// bank `bank` (meaningful only for the banked regions; 0 elsewhere). Produced by
/// [`Interconnect::cdl_logged_ranges`] for the MCP/debug `cdl-ranges` tool.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CdlRange {
    pub bank: u16,
    pub start: u16,
    pub end: u16,
}

/// Append every maximal run of non-zero bytes in `buf[base..base+span]` as a
/// [`CdlRange`], mapping physical offset `o` to CPU address `cpu_start + o`
/// (contiguous within one bank). A `.` (zero) gap splits a run.
fn push_runs(
    buf: &[u8],
    base: usize,
    span: usize,
    cpu_start: u16,
    bank: u16,
    out: &mut Vec<CdlRange>,
) {
    let mut run_start: Option<u16> = None;
    for o in 0..span {
        let cpu = cpu_start + o as u16;
        let set = buf.get(base + o).is_some_and(|&f| f != 0);
        match (set, run_start) {
            (true, None) => run_start = Some(cpu),
            (false, Some(start)) => {
                out.push(CdlRange {
                    bank,
                    start,
                    end: cpu - 1,
                });
                run_start = None;
            }
            _ => {}
        }
    }
    if let Some(start) = run_start {
        out.push(CdlRange {
            bank,
            start,
            end: cpu_start + (span as u16 - 1),
        });
    }
}

/// Cumulative base offsets of the physical CDL regions plus the total buffer
/// size (see [`Interconnect::cdl_layout`]).
struct CdlLayout {
    rom: usize,
    vram: usize,
    sram: usize,
    wram: usize,
    total: usize,
}
