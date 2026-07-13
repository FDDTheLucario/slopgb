//! SPC700 — the SNES APU (audio) CPU core.
//!
//! Self-contained, cycle-accurate SPC700 (Sony SPC700 / S-SMP) processor. The
//! Super Game Boy runs its SNES-side sound driver on this CPU; slopgb needs it
//! for SGB audio. This module is **standalone**: it does not touch the Game Boy
//! CPU, PPU, or any GB state. It is wired to the S-DSP (see [`Dsp`]) and to the
//! SNES↔APU comm ports (see [`Spc700::snes_write_port`]) by the SGB APU seam.
//!
//! # Model
//!
//! - 8-bit registers `A`, `X`, `Y`, `SP`; 16-bit `PC`; `PSW` = N V P B H I Z C.
//! - 64 KB RAM with a 64-byte IPL boot ROM overlaid at `$FFC0-$FFFF`, toggled by
//!   `$F1` bit 7 ([`ram`]).
//! - Four bidirectional comm ports `$F4-$F7` (separate SNES-side / APU-side
//!   latches), control/test `$F0/$F1`, DSP address/data `$F2/$F3`, three timers
//!   `$FA-$FF` ([`ports`], [`timers`]).
//! - `A`/`Y` form the 16-bit pair `YA` (`Y` = high byte).
//!
//! # Cycle timing
//!
//! The SPC700 runs at 1.024 MHz. [`Spc700::step`] executes one instruction and
//! returns the cycles it consumed, matching the documented per-opcode table
//! (fullsnes / anomie's SPC700 doc / bsnes `spc700`). The timers are advanced
//! from those cycles.
//!
//! # Sources
//!
//! Behaviour and cycle counts are from nocash **fullsnes** ("SNES APU / SPC700"),
//! anomie's **SPC700 doc**, and **bsnes** (`processor/spc700`) for the awkward
//! corners (`DIV` overflow quirk, `DAA`/`DAS`, `ADDW`/`SUBW` half-carry). Each is
//! cited at the implementation site. Every opcode is validated against the
//! `SingleStepTests/spc700` suite (see `spc700_tests/`).

mod ops_alu;
mod ops_bit;
mod ops_branch;
mod ops_misc;
mod ops_mov;
// SGB-audio seam completion: APU-RAM access, `Clone`, save-state serialization.
// Additive over the verified opcode/RAM/ports modules, which stay untouched; see
// `phase3.rs`.
mod phase3;
mod ports;
mod ram;
mod timers;

pub use ports::Dsp;
use timers::Timer;

/// Program status word (processor flags). Stored unpacked for clarity; packed
/// to/from a byte only for `PUSH/POP PSW`, `BRK`, `RETI`.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Psw {
    /// Negative (bit 7 of last result).
    pub n: bool,
    /// Overflow.
    pub v: bool,
    /// Direct page select: `false` → `$00xx`, `true` → `$01xx` (PSW bit 5).
    pub p: bool,
    /// Break.
    pub b: bool,
    /// Half-carry (carry out of bit 3 / bit 11 for word ops).
    pub h: bool,
    /// Interrupt enable.
    pub i: bool,
    /// Zero.
    pub z: bool,
    /// Carry.
    pub c: bool,
}

impl Psw {
    /// Pack to the `$FF`-order byte `N V P B H I Z C`.
    pub fn to_byte(self) -> u8 {
        (self.n as u8) << 7
            | (self.v as u8) << 6
            | (self.p as u8) << 5
            | (self.b as u8) << 4
            | (self.h as u8) << 3
            | (self.i as u8) << 2
            | (self.z as u8) << 1
            | (self.c as u8)
    }

    /// Unpack from a byte.
    pub fn from_byte(b: u8) -> Self {
        Psw {
            n: b & 0x80 != 0,
            v: b & 0x40 != 0,
            p: b & 0x20 != 0,
            b: b & 0x10 != 0,
            h: b & 0x08 != 0,
            i: b & 0x04 != 0,
            z: b & 0x02 != 0,
            c: b & 0x01 != 0,
        }
    }
}

/// The SPC700 CPU + its APU RAM, I/O ports, and timers.
pub struct Spc700 {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub psw: Psw,

    // Internal state below is private: it is reached only by this module and
    // its descendants (the `ops_*`, `ram`, `ports`, `timers`, and test
    // submodules), which see private items of an ancestor. Phase 3 uses the
    // public methods (`attach_dsp`, `snes_*`), never these fields.
    /// 64 KB APU RAM (RAM underlies the IPL ROM at `$FFC0-$FFFF`).
    ram: Box<[u8; 0x1_0000]>,

    /// APU-side input latches (`$F4-$F7` read): what the SNES wrote.
    port_in: [u8; 4],
    /// APU-side output latches (`$F4-$F7` write): what the SNES reads.
    port_out: [u8; 4],
    /// `$F0` test register.
    test: u8,
    /// `$F1` control register (timer enables + IPL-ROM enable; strobes not kept).
    control: u8,
    /// `$F2` DSP register-select.
    dsp_addr: u8,
    /// `$F8`/`$F9` general-purpose registers (AUXIO4/5).
    aux: [u8; 2],
    /// DSP register shadow used when no [`Dsp`] is attached (standalone mode).
    dsp_shadow: [u8; 128],
    timer: [Timer; 3],
    /// Prescaler accumulator for the 8 kHz T0/T1 (÷128 of 1.024 MHz).
    presc_8k: u32,
    /// Prescaler accumulator for the 64 kHz T2 (÷16).
    presc_64k: u32,

    /// Optional S-DSP plugged in by Phase 3. `None` → the [`Spc700::dsp_shadow`]
    /// answers.
    dsp: Option<Box<dyn Dsp>>,

    /// Flat-RAM mode: bypass all I/O + IPL decoding, treat every address as RAM.
    /// Used only by the `SingleStepTests` harness (real hardware never sets it).
    flat_mem: bool,
    /// Set by `SLEEP`/`STOP`; the CPU idles until reset.
    pub stopped: bool,
    /// Total cycles executed since reset (wraps; informational).
    pub cycles: u64,
}

impl Default for Spc700 {
    fn default() -> Self {
        Self::new()
    }
}

impl Spc700 {
    /// Build a reset SPC700 with the IPL ROM enabled (production/APU mode).
    pub fn new() -> Self {
        let mut s = Spc700 {
            a: 0,
            x: 0,
            y: 0,
            sp: 0,
            pc: 0,
            psw: Psw::default(),
            ram: Box::new([0u8; 0x1_0000]),
            port_in: [0; 4],
            port_out: [0; 4],
            test: 0,
            control: 0,
            dsp_addr: 0,
            aux: [0; 2],
            dsp_shadow: [0; 128],
            timer: [Timer::default(); 3],
            presc_8k: 0,
            presc_64k: 0,
            dsp: None,
            flat_mem: false,
            stopped: false,
            cycles: 0,
        };
        s.reset();
        s
    }

    /// Reset: IPL ROM enabled, `PC` from the reset vector at `$FFFE/$FFFF`
    /// (which is `$FFC0` while the IPL is mapped), registers cleared. bsnes
    /// `SMP::power`. The IPL itself sets `SP`/`X` immediately, so their reset
    /// values are irrelevant.
    pub fn reset(&mut self) {
        self.control = 0x80; // IPL ROM enabled (bit 7); timers disabled.
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.sp = 0;
        self.psw = Psw::default();
        self.stopped = false;
        self.presc_8k = 0;
        self.presc_64k = 0;
        for t in &mut self.timer {
            *t = Timer::default();
        }
        self.pc = self.read16(0xFFFE);
    }

    /// Execute one instruction; returns the cycles consumed. Drives the timers.
    #[must_use = "the returned cycle count drives the DSP/timer clock in Phase 3"]
    pub fn step(&mut self) -> u32 {
        if self.stopped {
            // Oscillator halted by SLEEP/STOP. Report the minimal tick so callers
            // still advance the timers + DSP (which keep running on real hardware).
            self.tick_timers(2);
            self.tick_dsp(2);
            return 2;
        }
        let op = self.fetch();
        let cyc = self.execute(op);
        self.cycles = self.cycles.wrapping_add(cyc as u64);
        self.tick_timers(cyc);
        self.tick_dsp(cyc);
        cyc
    }

    // -- core memory / operand helpers -------------------------------------

    /// Fetch the byte at `PC` and advance `PC`.
    pub(super) fn fetch(&mut self) -> u8 {
        let v = self.read8(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    /// Fetch a little-endian word (two operand bytes) at `PC`.
    pub(super) fn fetch16(&mut self) -> u16 {
        let lo = self.fetch() as u16;
        let hi = self.fetch() as u16;
        hi << 8 | lo
    }

    /// Read a little-endian word at an absolute address (high byte wraps `+1`).
    pub(super) fn read16(&mut self, addr: u16) -> u16 {
        let lo = self.read8(addr) as u16;
        let hi = self.read8(addr.wrapping_add(1)) as u16;
        hi << 8 | lo
    }

    /// Direct-page effective address for an offset, honoring the `P` flag.
    pub(super) fn dp(&self, off: u8) -> u16 {
        (self.psw.p as u16) << 8 | off as u16
    }

    /// Read a word from the direct page; the high byte wraps within the page
    /// (`off+1` mod 256), per SPC700 direct-page pointer semantics.
    pub(super) fn read_word_dp(&mut self, off: u8) -> u16 {
        let lo = self.read8(self.dp(off)) as u16;
        let hi = self.read8(self.dp(off.wrapping_add(1))) as u16;
        hi << 8 | lo
    }

    /// Write a word to the direct page (high byte wraps within the page).
    pub(super) fn write_word_dp(&mut self, off: u8, val: u16) {
        let a = self.dp(off);
        self.write8(a, val as u8);
        let a2 = self.dp(off.wrapping_add(1));
        self.write8(a2, (val >> 8) as u8);
    }

    /// Push a byte to the stack (page 1, `$0100-$01FF`).
    pub(super) fn push(&mut self, v: u8) {
        let a = 0x0100 | self.sp as u16;
        self.write8(a, v);
        self.sp = self.sp.wrapping_sub(1);
    }

    /// Pull a byte from the stack.
    pub(super) fn pull(&mut self) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.read8(0x0100 | self.sp as u16)
    }

    /// Push a word (high byte first, so a pull yields low then high).
    pub(super) fn push16(&mut self, v: u16) {
        self.push((v >> 8) as u8);
        self.push(v as u8);
    }

    /// Pull a word (low then high).
    pub(super) fn pull16(&mut self) -> u16 {
        let lo = self.pull() as u16;
        let hi = self.pull() as u16;
        hi << 8 | lo
    }

    /// Set N and Z from an 8-bit result.
    pub(super) fn set_nz(&mut self, v: u8) {
        self.psw.n = v & 0x80 != 0;
        self.psw.z = v == 0;
    }

    /// Dispatch one opcode. Returns the cycle count (branch-taken adds +2 inside
    /// the relevant handlers). Cycle table: fullsnes "SPC700 Opcodes".
    #[rustfmt::skip]
    fn execute(&mut self, op: u8) -> u32 {
        let base = CYCLES[op as usize] as u32;
        // Handlers that can add branch-taken cycles `return` the total themselves;
        // for those arms the returned value replaces `base`.
        match op {
            // ---- ALU: OR / AND / EOR / CMP / ADC / SBC (A, <mode>) ----
            0x04 => { let v = self.am_dp();   self.a = self.op_or(self.a, v); }
            0x05 => { let v = self.am_abs();  self.a = self.op_or(self.a, v); }
            0x06 => { let v = self.am_xind(); self.a = self.op_or(self.a, v); }
            0x07 => { let v = self.am_idx();  self.a = self.op_or(self.a, v); }
            0x08 => { let v = self.am_imm();  self.a = self.op_or(self.a, v); }
            0x14 => { let v = self.am_dpx();  self.a = self.op_or(self.a, v); }
            0x15 => { let v = self.am_absx(); self.a = self.op_or(self.a, v); }
            0x16 => { let v = self.am_absy(); self.a = self.op_or(self.a, v); }
            0x17 => { let v = self.am_idy();  self.a = self.op_or(self.a, v); }
            0x09 => self.alu_dp_dp(AluOp::Or),
            0x18 => self.alu_dp_imm(AluOp::Or),
            0x19 => self.alu_xy(AluOp::Or),

            0x24 => { let v = self.am_dp();   self.a = self.op_and(self.a, v); }
            0x25 => { let v = self.am_abs();  self.a = self.op_and(self.a, v); }
            0x26 => { let v = self.am_xind(); self.a = self.op_and(self.a, v); }
            0x27 => { let v = self.am_idx();  self.a = self.op_and(self.a, v); }
            0x28 => { let v = self.am_imm();  self.a = self.op_and(self.a, v); }
            0x34 => { let v = self.am_dpx();  self.a = self.op_and(self.a, v); }
            0x35 => { let v = self.am_absx(); self.a = self.op_and(self.a, v); }
            0x36 => { let v = self.am_absy(); self.a = self.op_and(self.a, v); }
            0x37 => { let v = self.am_idy();  self.a = self.op_and(self.a, v); }
            0x29 => self.alu_dp_dp(AluOp::And),
            0x38 => self.alu_dp_imm(AluOp::And),
            0x39 => self.alu_xy(AluOp::And),

            0x44 => { let v = self.am_dp();   self.a = self.op_eor(self.a, v); }
            0x45 => { let v = self.am_abs();  self.a = self.op_eor(self.a, v); }
            0x46 => { let v = self.am_xind(); self.a = self.op_eor(self.a, v); }
            0x47 => { let v = self.am_idx();  self.a = self.op_eor(self.a, v); }
            0x48 => { let v = self.am_imm();  self.a = self.op_eor(self.a, v); }
            0x54 => { let v = self.am_dpx();  self.a = self.op_eor(self.a, v); }
            0x55 => { let v = self.am_absx(); self.a = self.op_eor(self.a, v); }
            0x56 => { let v = self.am_absy(); self.a = self.op_eor(self.a, v); }
            0x57 => { let v = self.am_idy();  self.a = self.op_eor(self.a, v); }
            0x49 => self.alu_dp_dp(AluOp::Eor),
            0x58 => self.alu_dp_imm(AluOp::Eor),
            0x59 => self.alu_xy(AluOp::Eor),

            0x64 => { let v = self.am_dp();   let a = self.a; self.cmp8(a, v); }
            0x65 => { let v = self.am_abs();  let a = self.a; self.cmp8(a, v); }
            0x66 => { let v = self.am_xind(); let a = self.a; self.cmp8(a, v); }
            0x67 => { let v = self.am_idx();  let a = self.a; self.cmp8(a, v); }
            0x68 => { let v = self.am_imm();  let a = self.a; self.cmp8(a, v); }
            0x74 => { let v = self.am_dpx();  let a = self.a; self.cmp8(a, v); }
            0x75 => { let v = self.am_absx(); let a = self.a; self.cmp8(a, v); }
            0x76 => { let v = self.am_absy(); let a = self.a; self.cmp8(a, v); }
            0x77 => { let v = self.am_idy();  let a = self.a; self.cmp8(a, v); }
            0x69 => self.alu_dp_dp(AluOp::Cmp),
            0x78 => self.alu_dp_imm(AluOp::Cmp),
            0x79 => self.alu_xy(AluOp::Cmp),

            0x84 => { let v = self.am_dp();   self.a = self.adc8(self.a, v); }
            0x85 => { let v = self.am_abs();  self.a = self.adc8(self.a, v); }
            0x86 => { let v = self.am_xind(); self.a = self.adc8(self.a, v); }
            0x87 => { let v = self.am_idx();  self.a = self.adc8(self.a, v); }
            0x88 => { let v = self.am_imm();  self.a = self.adc8(self.a, v); }
            0x94 => { let v = self.am_dpx();  self.a = self.adc8(self.a, v); }
            0x95 => { let v = self.am_absx(); self.a = self.adc8(self.a, v); }
            0x96 => { let v = self.am_absy(); self.a = self.adc8(self.a, v); }
            0x97 => { let v = self.am_idy();  self.a = self.adc8(self.a, v); }
            0x89 => self.alu_dp_dp(AluOp::Adc),
            0x98 => self.alu_dp_imm(AluOp::Adc),
            0x99 => self.alu_xy(AluOp::Adc),

            0xA4 => { let v = self.am_dp();   self.a = self.sbc8(self.a, v); }
            0xA5 => { let v = self.am_abs();  self.a = self.sbc8(self.a, v); }
            0xA6 => { let v = self.am_xind(); self.a = self.sbc8(self.a, v); }
            0xA7 => { let v = self.am_idx();  self.a = self.sbc8(self.a, v); }
            0xA8 => { let v = self.am_imm();  self.a = self.sbc8(self.a, v); }
            0xB4 => { let v = self.am_dpx();  self.a = self.sbc8(self.a, v); }
            0xB5 => { let v = self.am_absx(); self.a = self.sbc8(self.a, v); }
            0xB6 => { let v = self.am_absy(); self.a = self.sbc8(self.a, v); }
            0xB7 => { let v = self.am_idy();  self.a = self.sbc8(self.a, v); }
            0xA9 => self.alu_dp_dp(AluOp::Sbc),
            0xB8 => self.alu_dp_imm(AluOp::Sbc),
            0xB9 => self.alu_xy(AluOp::Sbc),

            // ---- CMP X / CMP Y ----
            0x1E => { let v = self.am_abs(); let x = self.x; self.cmp8(x, v); }
            0x3E => { let v = self.am_dp();  let x = self.x; self.cmp8(x, v); }
            0xC8 => { let v = self.am_imm(); let x = self.x; self.cmp8(x, v); }
            0x5E => { let v = self.am_abs(); let y = self.y; self.cmp8(y, v); }
            0x7E => { let v = self.am_dp();  let y = self.y; self.cmp8(y, v); }
            0xAD => { let v = self.am_imm(); let y = self.y; self.cmp8(y, v); }

            // ---- shifts / rotates / inc / dec ----
            0x1C => { let v = self.a; self.a = self.op_asl(v); }
            0x3C => { let v = self.a; self.a = self.op_rol(v); }
            0x5C => { let v = self.a; self.a = self.op_lsr(v); }
            0x7C => { let v = self.a; self.a = self.op_ror(v); }
            0xBC => { let v = self.a; self.a = self.op_inc(v); }
            0x9C => { let v = self.a; self.a = self.op_dec(v); }
            0x1D => { let v = self.x; self.x = self.op_dec(v); }
            0x3D => { let v = self.x; self.x = self.op_inc(v); }
            0xDC => { let v = self.y; self.y = self.op_dec(v); }
            0xFC => { let v = self.y; self.y = self.op_inc(v); }
            0x0B => self.rmw_dp(Rmw::Asl),
            0x0C => self.rmw_abs(Rmw::Asl),
            0x1B => self.rmw_dpx(Rmw::Asl),
            0x2B => self.rmw_dp(Rmw::Rol),
            0x2C => self.rmw_abs(Rmw::Rol),
            0x3B => self.rmw_dpx(Rmw::Rol),
            0x4B => self.rmw_dp(Rmw::Lsr),
            0x4C => self.rmw_abs(Rmw::Lsr),
            0x5B => self.rmw_dpx(Rmw::Lsr),
            0x6B => self.rmw_dp(Rmw::Ror),
            0x6C => self.rmw_abs(Rmw::Ror),
            0x7B => self.rmw_dpx(Rmw::Ror),
            0x8B => self.rmw_dp(Rmw::Dec),
            0x8C => self.rmw_abs(Rmw::Dec),
            0x9B => self.rmw_dpx(Rmw::Dec),
            0xAB => self.rmw_dp(Rmw::Inc),
            0xAC => self.rmw_abs(Rmw::Inc),
            0xBB => self.rmw_dpx(Rmw::Inc),

            // ---- 16-bit word ops ----
            0x7A => self.op_addw(),
            0x9A => self.op_subw(),
            0x5A => self.op_cmpw(),
            0x3A => self.op_incw(),
            0x1A => self.op_decw(),
            0xBA => self.op_movw_load(),
            0xDA => self.op_movw_store(),

            // ---- multiply / divide / decimal / nibble ----
            0xCF => self.op_mul(),
            0x9E => self.op_div(),
            0xDF => self.op_daa(),
            0xBE => self.op_das(),
            0x9F => self.op_xcn(),

            // ---- MOV loads (set N,Z) ----
            0xE8 => { let v = self.am_imm();  self.a = v; self.set_nz(v); }
            0xE6 => { let v = self.am_xind(); self.a = v; self.set_nz(v); }
            0xBF => { let v = self.am_xind_inc(); self.a = v; self.set_nz(v); }
            0xE4 => { let v = self.am_dp();   self.a = v; self.set_nz(v); }
            0xF4 => { let v = self.am_dpx();  self.a = v; self.set_nz(v); }
            0xE5 => { let v = self.am_abs();  self.a = v; self.set_nz(v); }
            0xF5 => { let v = self.am_absx(); self.a = v; self.set_nz(v); }
            0xF6 => { let v = self.am_absy(); self.a = v; self.set_nz(v); }
            0xE7 => { let v = self.am_idx();  self.a = v; self.set_nz(v); }
            0xF7 => { let v = self.am_idy();  self.a = v; self.set_nz(v); }
            0xCD => { let v = self.am_imm(); self.x = v; self.set_nz(v); }
            0xF8 => { let v = self.am_dp();  self.x = v; self.set_nz(v); }
            0xF9 => { let v = self.am_dpy(); self.x = v; self.set_nz(v); }
            0xE9 => { let v = self.am_abs(); self.x = v; self.set_nz(v); }
            0x8D => { let v = self.am_imm(); self.y = v; self.set_nz(v); }
            0xEB => { let v = self.am_dp();  self.y = v; self.set_nz(v); }
            0xFB => { let v = self.am_dpx(); self.y = v; self.set_nz(v); }
            0xEC => { let v = self.am_abs(); self.y = v; self.set_nz(v); }

            // ---- register transfers ----
            0x5D => { let v = self.a;  self.x = v;  self.set_nz(v); } // MOV X,A
            0x7D => { let v = self.x;  self.a = v;  self.set_nz(v); } // MOV A,X
            0xDD => { let v = self.y;  self.a = v;  self.set_nz(v); } // MOV A,Y
            0xFD => { let v = self.a;  self.y = v;  self.set_nz(v); } // MOV Y,A
            0x9D => { let v = self.sp; self.x = v;  self.set_nz(v); } // MOV X,SP
            0xBD => { self.sp = self.x; }                            // MOV SP,X (no flags)

            // ---- MOV stores (no flags) ----
            0xC6 => { let a = self.dp(self.x); let v = self.a; self.write8(a, v); }
            0xAF => self.mov_xinc_store(),
            0xC4 => { let a = self.ea_dp();   let v = self.a; self.write8(a, v); }
            0xD4 => { let a = self.ea_dpx();  let v = self.a; self.write8(a, v); }
            0xC5 => { let a = self.ea_abs();  let v = self.a; self.write8(a, v); }
            0xD5 => { let a = self.ea_absx(); let v = self.a; self.write8(a, v); }
            0xD6 => { let a = self.ea_absy(); let v = self.a; self.write8(a, v); }
            0xC7 => { let a = self.ea_idx();  let v = self.a; self.write8(a, v); }
            0xD7 => { let a = self.ea_idy();  let v = self.a; self.write8(a, v); }
            0xD8 => { let a = self.ea_dp();   let v = self.x; self.write8(a, v); }
            0xD9 => { let a = self.ea_dpy();  let v = self.x; self.write8(a, v); }
            0xC9 => { let a = self.ea_abs();  let v = self.x; self.write8(a, v); }
            0xCB => { let a = self.ea_dp();   let v = self.y; self.write8(a, v); }
            0xDB => { let a = self.ea_dpx();  let v = self.y; self.write8(a, v); }
            0xCC => { let a = self.ea_abs();  let v = self.y; self.write8(a, v); }
            0xFA => self.mov_dp_dp(),
            0x8F => self.mov_dp_imm(),

            // ---- push / pop ----
            0x2D => { let v = self.a; self.push(v); }
            0x4D => { let v = self.x; self.push(v); }
            0x6D => { let v = self.y; self.push(v); }
            0x0D => { let v = self.psw.to_byte(); self.push(v); }
            0xAE => { let v = self.pull(); self.a = v; }
            0xCE => { let v = self.pull(); self.x = v; }
            0xEE => { let v = self.pull(); self.y = v; }
            0x8E => { let v = self.pull(); self.psw = Psw::from_byte(v); }

            // ---- branches / jumps / calls (some add taken cycles → `return`) ----
            0x10 => return self.branch(base, !self.psw.n), // BPL
            0x30 => return self.branch(base, self.psw.n),  // BMI
            0x50 => return self.branch(base, !self.psw.v), // BVC
            0x70 => return self.branch(base, self.psw.v),  // BVS
            0x90 => return self.branch(base, !self.psw.c), // BCC
            0xB0 => return self.branch(base, self.psw.c),  // BCS
            0xD0 => return self.branch(base, !self.psw.z), // BNE
            0xF0 => return self.branch(base, self.psw.z),  // BEQ
            0x2F => self.bra(),
            0x5F => { let a = self.fetch16(); self.pc = a; }        // JMP !abs
            0x1F => self.jmp_absx(),                                // JMP [!abs+X]
            0x3F => { let a = self.fetch16(); let pc = self.pc; self.push16(pc); self.pc = a; } // CALL
            0x4F => self.pcall(),
            0x6F => { self.pc = self.pull16(); }                    // RET
            0x7F => self.reti(),
            0x0F => self.brk(),
            0x01 | 0x11 | 0x21 | 0x31 | 0x41 | 0x51 | 0x61 | 0x71
            | 0x81 | 0x91 | 0xA1 | 0xB1 | 0xC1 | 0xD1 | 0xE1 | 0xF1 => self.tcall(op),
            0x2E => return self.cbne_dp(base, false),
            0xDE => return self.cbne_dp(base, true),
            0x6E => return self.dbnz_dp(base),
            0xFE => return self.dbnz_y(base),

            // ---- bit ops on dp.bit ----
            0x02 | 0x22 | 0x42 | 0x62 | 0x82 | 0xA2 | 0xC2 | 0xE2 => self.set1(op),
            0x12 | 0x32 | 0x52 | 0x72 | 0x92 | 0xB2 | 0xD2 | 0xF2 => self.clr1(op),
            0x03 | 0x23 | 0x43 | 0x63 | 0x83 | 0xA3 | 0xC3 | 0xE3 => return self.bbs(base, op),
            0x13 | 0x33 | 0x53 | 0x73 | 0x93 | 0xB3 | 0xD3 | 0xF3 => return self.bbc(base, op),
            0x0E => self.tset1(),
            0x4E => self.tclr1(),

            // ---- carry-bit / membit ops (13-bit addr) ----
            0xAA => self.mov1_c_m(),
            0xCA => self.mov1_m_c(),
            0x4A => self.and1(false),
            0x6A => self.and1(true),
            0x0A => self.or1(false),
            0x2A => self.or1(true),
            0x8A => self.eor1(),
            0xEA => self.not1(),

            // ---- flag ops / control / NOP / SLEEP / STOP (see ops_misc) ----
            0x00 | 0x20 | 0x40 | 0x60 | 0x80 | 0xA0 | 0xC0 | 0xE0 | 0xED | 0xEF
            | 0xFF => self.op_misc(op),
        }
        base
    }
}

/// ALU op selector for the `dp,dp` / `dp,#imm` / `(X),(Y)` shared handlers.
#[derive(Clone, Copy)]
pub(super) enum AluOp {
    Or,
    And,
    Eor,
    Cmp,
    Adc,
    Sbc,
}

/// Read-modify-write op selector for the `dp` / `!abs` / `dp+X` shared handlers.
#[derive(Clone, Copy)]
pub(super) enum Rmw {
    Asl,
    Rol,
    Lsr,
    Ror,
    Inc,
    Dec,
}

/// Base cycle counts, indexed by opcode. Conditional ops list the *not-taken*
/// count; taken adds +2 inside the handler. Source: fullsnes "SPC700 Opcodes",
/// cross-checked exhaustively against the `SingleStepTests/spc700` cycle traces.
/// `EF`/`FF` (SLEEP/STOP) halt the CPU; the hardware trace snapshots a 7-cycle
/// fetch-then-spin, so they are 7 here (behaviourally the CPU sets `stopped`).
#[rustfmt::skip]
pub(super) const CYCLES: [u8; 256] = [
    // x0 x1 x2 x3 x4 x5 x6 x7 x8 x9 xA xB xC xD xE xF
    2, 8, 4, 5, 3, 4, 3, 6, 2, 6, 5, 4, 5, 4, 6, 8, // 0x
    2, 8, 4, 5, 4, 5, 5, 6, 5, 5, 6, 5, 2, 2, 4, 6, // 1x
    2, 8, 4, 5, 3, 4, 3, 6, 2, 6, 5, 4, 5, 4, 5, 4, // 2x
    2, 8, 4, 5, 4, 5, 5, 6, 5, 5, 6, 5, 2, 2, 3, 8, // 3x
    2, 8, 4, 5, 3, 4, 3, 6, 2, 6, 4, 4, 5, 4, 6, 6, // 4x
    2, 8, 4, 5, 4, 5, 5, 6, 5, 5, 4, 5, 2, 2, 4, 3, // 5x
    2, 8, 4, 5, 3, 4, 3, 6, 2, 6, 4, 4, 5, 4, 5, 5, // 6x
    2, 8, 4, 5, 4, 5, 5, 6, 5, 5, 5, 5, 2, 2, 3, 6, // 7x
    2, 8, 4, 5, 3, 4, 3, 6, 2, 6, 5, 4, 5, 2, 4, 5, // 8x
    2, 8, 4, 5, 4, 5, 5, 6, 5, 5, 5, 5, 2, 2, 12, 5, // 9x
    3, 8, 4, 5, 3, 4, 3, 6, 2, 6, 4, 4, 5, 2, 4, 4, // Ax
    2, 8, 4, 5, 4, 5, 5, 6, 5, 5, 5, 5, 2, 2, 3, 4, // Bx
    3, 8, 4, 5, 4, 5, 4, 7, 2, 5, 6, 4, 5, 2, 4, 9, // Cx
    2, 8, 4, 5, 5, 6, 6, 7, 4, 5, 5, 5, 2, 2, 6, 3, // Dx
    2, 8, 4, 5, 3, 4, 3, 6, 2, 4, 5, 3, 4, 3, 4, 7, // Ex  (EF SLEEP: halt, see note)
    2, 8, 4, 5, 4, 5, 5, 6, 3, 4, 5, 4, 2, 2, 4, 7, // Fx  (FF STOP: halt, see note)
];

#[cfg(test)]
#[path = "spc700_tests/mod.rs"]
mod tests;
