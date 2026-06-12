//! Unit tests for instruction decode/execute. Split out of `execute.rs`
//! purely for file size; compiled as `super::tests` via the `#[path]`
//! attribute on the module declaration there.

use super::super::{Bus, Cpu, Registers, flags};
use super::step;
use Ev::{Read, Tick, Write};

/// One bus event == one M-cycle. The index in [`TestBus::log`] is the
/// cycle index, so comparing whole logs asserts both the kind and the
/// exact cycle position of every access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ev {
    Read(u16, u8),
    Write(u16, u8),
    Tick,
}

/// 64 KiB flat RAM. IF lives at 0xFF0F and IE at 0xFFFF inside `mem`, so
/// stack pushes that land on IE behave exactly like on hardware.
struct TestBus {
    mem: Vec<u8>,
    log: Vec<Ev>,
    stop_result: bool,
    stop_calls: u32,
    /// M-cycles elapsed since construction (`take_log` does not reset
    /// this).
    cycles: usize,
    /// Simulated peripheral IRQ: during M-cycle `at` (0-based), OR
    /// `bits` into IF - models e.g. the PPU raising the vblank IF bit
    /// partway through a cycle, before that cycle's memory access.
    raise_if: Option<(usize, u8)>,
    /// `raise_if` models a timer IF committed on the *last* T-substep of
    /// its M-cycle: invisible to [`Bus::pending_halt_wake`] during the
    /// cycle it is raised (visible from the next cycle on), mirroring the
    /// real interconnect's substep-aware halt-exit sampling.
    late_if: bool,
    /// Current core-clock gate level as driven through
    /// [`Bus::set_halted`] (the calls are idempotent).
    halted_state: bool,
    /// Gate *transitions*, as (M-cycles elapsed when the call arrived,
    /// new level) — pins exactly when halt/stop engage and release the
    /// core clock gate.
    halt_calls: Vec<(usize, bool)>,
}

impl TestBus {
    fn new() -> Self {
        Self {
            mem: vec![0; 0x10000],
            log: Vec::new(),
            stop_result: false,
            stop_calls: 0,
            cycles: 0,
            raise_if: None,
            late_if: false,
            halted_state: false,
            halt_calls: Vec::new(),
        }
    }

    /// Start of every M-cycle: advance the simulated peripherals
    /// (tick-then-access, same ordering as the real interconnect).
    fn advance(&mut self) {
        if let Some((at, bits)) = self.raise_if {
            if at == self.cycles {
                self.mem[0xFF0F] |= bits;
            }
        }
        self.cycles += 1;
    }

    fn load(&mut self, addr: u16, bytes: &[u8]) {
        for (i, &b) in bytes.iter().enumerate() {
            self.mem[usize::from(addr) + i] = b;
        }
    }

    fn take_log(&mut self) -> Vec<Ev> {
        std::mem::take(&mut self.log)
    }
}

impl Bus for TestBus {
    fn read(&mut self, addr: u16) -> u8 {
        self.advance();
        let v = self.mem[usize::from(addr)];
        self.log.push(Read(addr, v));
        v
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.advance();
        self.log.push(Write(addr, value));
        self.mem[usize::from(addr)] = value;
    }

    fn tick(&mut self) {
        self.advance();
        self.log.push(Tick);
    }

    fn pending(&self) -> u8 {
        self.mem[0xFF0F] & self.mem[0xFFFF] & 0x1F
    }

    fn pending_halt_wake(&self) -> u8 {
        let mut p = self.pending();
        if self.late_if {
            if let Some((at, bits)) = self.raise_if {
                // `advance` has already incremented `cycles`, so the bits
                // raised during the current M-cycle (at == cycles - 1) are
                // the ones the halt-exit sampling missed.
                if at + 1 == self.cycles {
                    p &= !bits;
                }
            }
        }
        p
    }

    fn ack(&mut self, bit: u8) {
        self.mem[0xFF0F] &= !(1 << bit);
    }

    fn stop(&mut self) -> bool {
        self.stop_calls += 1;
        self.stop_result
    }

    fn set_halted(&mut self, halted: bool) {
        if self.halted_state != halted {
            self.halted_state = halted;
            self.halt_calls.push((self.cycles, halted));
        }
    }
}

const PC0: u16 = 0xC000;
const SP0: u16 = 0xD000;

/// Build an F-register value from individual flags.
fn fl(z: bool, n: bool, h: bool, c: bool) -> u8 {
    (u8::from(z) << 7) | (u8::from(n) << 6) | (u8::from(h) << 5) | (u8::from(c) << 4)
}

fn cpu() -> Cpu {
    let mut regs = Registers::default();
    regs.pc = PC0;
    regs.sp = SP0;
    Cpu {
        regs,
        ime: false,
        ime_pending: false,
        halted: false,
        stopped: false,
        halt_bug: false,
        debug_breakpoint: false,
        locked: false,
    }
}

fn bus(program: &[u8]) -> TestBus {
    let mut b = TestBus::new();
    b.load(PC0, program);
    b
}

// ----- loads -----

#[test]
fn nop_is_one_fetch_cycle() {
    let mut c = cpu();
    let mut b = bus(&[0x00]);
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0x00)]);
    assert_eq!(c.regs.pc, PC0 + 1);
    assert_eq!(c.regs.f(), 0);
}

#[test]
fn ld_r_r_moves_value_in_one_cycle() {
    let mut c = cpu();
    c.regs.c = 0x42;
    let mut b = bus(&[0x41]); // LD B,C
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0x41)]);
    assert_eq!(c.regs.b, 0x42);
}

#[test]
fn ld_b_b_sets_debug_breakpoint_and_still_loads() {
    let mut c = cpu();
    c.regs.b = 7;
    let mut b = bus(&[0x40]);
    step(&mut c, &mut b);
    assert!(c.debug_breakpoint);
    assert!(c.debug_breakpoint_hit());
    assert_eq!(c.regs.b, 7);
    assert_eq!(b.log, [Read(PC0, 0x40)]);
}

#[test]
fn ld_r_hl_and_ld_hl_r_traces() {
    let mut c = cpu();
    c.regs.set_hl(0xC800);
    let mut b = bus(&[0x56, 0x73]); // LD D,(HL); LD (HL),E
    b.mem[0xC800] = 0x99;
    c.regs.e = 0x5A;
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0, 0x56), Read(0xC800, 0x99)]);
    assert_eq!(c.regs.d, 0x99);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0 + 1, 0x73), Write(0xC800, 0x5A)]);
    assert_eq!(b.mem[0xC800], 0x5A);
}

#[test]
fn ld_r_imm_and_ld_hl_imm() {
    let mut c = cpu();
    let mut b = bus(&[0x3E, 0xAB, 0x36, 0x77]); // LD A,n; LD (HL),n
    c.regs.set_hl(0xC800);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0, 0x3E), Read(PC0 + 1, 0xAB)]);
    assert_eq!(c.regs.a, 0xAB);
    step(&mut c, &mut b);
    assert_eq!(
        b.take_log(),
        [
            Read(PC0 + 2, 0x36),
            Read(PC0 + 3, 0x77),
            Write(0xC800, 0x77)
        ]
    );
}

#[test]
fn ld_rp_imm_all_pairs() {
    for (op, check) in [(0x01u8, 0usize), (0x11, 1), (0x21, 2), (0x31, 3)] {
        let mut c = cpu();
        let mut b = bus(&[op, 0xCD, 0xAB]);
        step(&mut c, &mut b);
        assert_eq!(
            b.log,
            [Read(PC0, op), Read(PC0 + 1, 0xCD), Read(PC0 + 2, 0xAB)]
        );
        let got = match check {
            0 => c.regs.bc(),
            1 => c.regs.de(),
            2 => c.regs.hl(),
            _ => c.regs.sp,
        };
        assert_eq!(got, 0xABCD);
    }
}

#[test]
fn ld_a_indirect_loads_and_stores() {
    // stores
    let mut c = cpu();
    c.regs.a = 0x5C;
    c.regs.set_bc(0xC700);
    c.regs.set_de(0xC701);
    c.regs.set_hl(0xC702);
    let mut b = bus(&[0x02, 0x12, 0x22, 0x32]);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0, 0x02), Write(0xC700, 0x5C)]);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0 + 1, 0x12), Write(0xC701, 0x5C)]);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0 + 2, 0x22), Write(0xC702, 0x5C)]);
    assert_eq!(c.regs.hl(), 0xC703); // HL+
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0 + 3, 0x32), Write(0xC703, 0x5C)]);
    assert_eq!(c.regs.hl(), 0xC702); // HL-

    // loads
    let mut c = cpu();
    c.regs.set_bc(0xC700);
    c.regs.set_de(0xC701);
    c.regs.set_hl(0xC702);
    let mut b = bus(&[0x0A, 0x1A, 0x2A, 0x3A]);
    b.load(0xC700, &[0x11, 0x22, 0x33]);
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x11);
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x22);
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x33);
    assert_eq!(c.regs.hl(), 0xC703);
    b.take_log();
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0 + 3, 0x3A), Read(0xC703, 0x00)]);
    assert_eq!(c.regs.hl(), 0xC702);
}

#[test]
fn ld_nn_a_and_ld_a_nn() {
    let mut c = cpu();
    c.regs.a = 0x77;
    let mut b = bus(&[0xEA, 0x34, 0xC9, 0xFA, 0x34, 0xC9]);
    step(&mut c, &mut b);
    assert_eq!(
        b.take_log(),
        [
            Read(PC0, 0xEA),
            Read(PC0 + 1, 0x34),
            Read(PC0 + 2, 0xC9),
            Write(0xC934, 0x77)
        ]
    );
    c.regs.a = 0;
    step(&mut c, &mut b);
    assert_eq!(
        b.take_log(),
        [
            Read(PC0 + 3, 0xFA),
            Read(PC0 + 4, 0x34),
            Read(PC0 + 5, 0xC9),
            Read(0xC934, 0x77)
        ]
    );
    assert_eq!(c.regs.a, 0x77);
}

#[test]
fn ldh_imm_and_c_variants() {
    let mut c = cpu();
    c.regs.a = 0x42;
    c.regs.c = 0x81;
    let mut b = bus(&[0xE0, 0x80, 0xF0, 0x80, 0xE2, 0xF2]);
    step(&mut c, &mut b);
    assert_eq!(
        b.take_log(),
        [Read(PC0, 0xE0), Read(PC0 + 1, 0x80), Write(0xFF80, 0x42)]
    );
    c.regs.a = 0;
    step(&mut c, &mut b);
    assert_eq!(
        b.take_log(),
        [Read(PC0 + 2, 0xF0), Read(PC0 + 3, 0x80), Read(0xFF80, 0x42)]
    );
    assert_eq!(c.regs.a, 0x42);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0 + 4, 0xE2), Write(0xFF81, 0x42)]);
    b.mem[0xFF81] = 0x55;
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0 + 5, 0xF2), Read(0xFF81, 0x55)]);
    assert_eq!(c.regs.a, 0x55);
}

#[test]
fn ld_nn_sp_writes_lo_then_hi() {
    let mut c = cpu();
    c.regs.sp = 0xABCD;
    let mut b = bus(&[0x08, 0x34, 0xC1]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0, 0x08),
            Read(PC0 + 1, 0x34),
            Read(PC0 + 2, 0xC1),
            Write(0xC134, 0xCD),
            Write(0xC135, 0xAB)
        ]
    );
}

#[test]
fn ld_sp_hl_has_internal_cycle() {
    let mut c = cpu();
    c.regs.set_hl(0x1234);
    let mut b = bus(&[0xF9]);
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0xF9), Tick]);
    assert_eq!(c.regs.sp, 0x1234);
}

// ----- push / pop -----

#[test]
fn push_has_internal_cycle_before_writes() {
    let mut c = cpu();
    c.regs.set_bc(0x1234);
    let mut b = bus(&[0xC5]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0, 0xC5),
            Tick,
            Write(SP0 - 1, 0x12),
            Write(SP0 - 2, 0x34)
        ]
    );
    assert_eq!(c.regs.sp, SP0 - 2);
}

#[test]
fn pop_has_no_internal_cycle() {
    let mut c = cpu();
    let mut b = bus(&[0xD1]); // POP DE
    b.load(SP0, &[0x34, 0x12]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [Read(PC0, 0xD1), Read(SP0, 0x34), Read(SP0 + 1, 0x12)]
    );
    assert_eq!(c.regs.de(), 0x1234);
    assert_eq!(c.regs.sp, SP0 + 2);
}

#[test]
fn pop_af_masks_f_low_nibble() {
    let mut c = cpu();
    let mut b = bus(&[0xF1]);
    b.load(SP0, &[0xFF, 0x12]);
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x12);
    assert_eq!(c.regs.f(), 0xF0);
}

#[test]
fn push_af_writes_a_then_f() {
    let mut c = cpu();
    c.regs.a = 0x12;
    c.regs.set_f(0xF0);
    let mut b = bus(&[0xF5]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0, 0xF5),
            Tick,
            Write(SP0 - 1, 0x12),
            Write(SP0 - 2, 0xF0)
        ]
    );
}

// ----- 8-bit ALU -----

#[test]
fn add_and_adc_flags() {
    let mut c = cpu();
    c.regs.a = 0x0F;
    c.regs.b = 0x01;
    let mut b = bus(&[0x80]); // ADD A,B
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x10);
    assert_eq!(c.regs.f(), flags::H);

    // carry + zero: 0xFF + 0x00 + carry-in
    let mut c = cpu();
    c.regs.a = 0xFF;
    c.regs.b = 0x00;
    c.regs.set_f(flags::C);
    let mut b = bus(&[0x88]); // ADC A,B
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x00);
    assert_eq!(c.regs.f(), flags::Z | flags::H | flags::C);

    // ADC carry contributes to both halves
    let mut c = cpu();
    c.regs.a = 0x80;
    c.regs.b = 0x80;
    let mut b = bus(&[0x80]); // ADD A,B -> 0x00, C
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0);
    assert_eq!(c.regs.f(), flags::Z | flags::C);
}

#[test]
fn sub_sbc_cp_flags() {
    let mut c = cpu();
    c.regs.a = 0x10;
    c.regs.b = 0x01;
    let mut b = bus(&[0x90]); // SUB B: half borrow
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x0F);
    assert_eq!(c.regs.f(), flags::N | flags::H);

    let mut c = cpu();
    c.regs.a = 0x00;
    c.regs.b = 0x00;
    c.regs.set_f(flags::C);
    let mut b = bus(&[0x98]); // SBC A,B: 0 - 0 - 1
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0xFF);
    assert_eq!(c.regs.f(), flags::N | flags::H | flags::C);

    let mut c = cpu();
    c.regs.a = 0x42;
    c.regs.b = 0x42;
    let mut b = bus(&[0xB8]); // CP B: equal, A unchanged
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x42);
    assert_eq!(c.regs.f(), flags::Z | flags::N);
}

#[test]
fn and_xor_or_flags() {
    let mut c = cpu();
    c.regs.a = 0xF0;
    c.regs.b = 0x0F;
    let mut b = bus(&[0xA0]); // AND B -> 0, H always set
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0);
    assert_eq!(c.regs.f(), flags::Z | flags::H);

    let mut c = cpu();
    c.regs.a = 0xFF;
    c.regs.b = 0xFF;
    c.regs.set_f(0xF0);
    let mut b = bus(&[0xA8]); // XOR B -> 0
    step(&mut c, &mut b);
    assert_eq!(c.regs.f(), flags::Z);

    let mut c = cpu();
    c.regs.a = 0x00;
    c.regs.b = 0x08;
    c.regs.set_f(0xF0);
    let mut b = bus(&[0xB0]); // OR B
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x08);
    assert_eq!(c.regs.f(), 0);
}

#[test]
fn alu_hl_and_imm_operand_timing() {
    let mut c = cpu();
    c.regs.set_hl(0xC800);
    c.regs.a = 1;
    let mut b = bus(&[0x86, 0xC6, 0x05]); // ADD A,(HL); ADD A,5
    b.mem[0xC800] = 2;
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0, 0x86), Read(0xC800, 0x02)]);
    assert_eq!(c.regs.a, 3);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0 + 1, 0xC6), Read(PC0 + 2, 0x05)]);
    assert_eq!(c.regs.a, 8);
}

// ----- INC/DEC -----

#[test]
fn inc_dec_r8_flags_preserve_carry() {
    let mut c = cpu();
    c.regs.b = 0x0F;
    c.regs.set_f(flags::C);
    let mut b = bus(&[0x04]); // INC B
    step(&mut c, &mut b);
    assert_eq!(c.regs.b, 0x10);
    assert_eq!(c.regs.f(), flags::H | flags::C);

    let mut c = cpu();
    c.regs.b = 0xFF;
    let mut b = bus(&[0x04]);
    step(&mut c, &mut b);
    assert_eq!(c.regs.b, 0x00);
    assert_eq!(c.regs.f(), flags::Z | flags::H);

    let mut c = cpu();
    c.regs.b = 0x10;
    c.regs.set_f(flags::C);
    let mut b = bus(&[0x05]); // DEC B: borrow from bit 4
    step(&mut c, &mut b);
    assert_eq!(c.regs.b, 0x0F);
    assert_eq!(c.regs.f(), flags::N | flags::H | flags::C);

    let mut c = cpu();
    c.regs.b = 0x01;
    let mut b = bus(&[0x05]);
    step(&mut c, &mut b);
    assert_eq!(c.regs.b, 0x00);
    assert_eq!(c.regs.f(), flags::Z | flags::N);

    let mut c = cpu();
    c.regs.b = 0x00;
    let mut b = bus(&[0x05]);
    step(&mut c, &mut b);
    assert_eq!(c.regs.b, 0xFF);
    assert_eq!(c.regs.f(), flags::N | flags::H);
}

#[test]
fn inc_hl_is_read_modify_write() {
    let mut c = cpu();
    c.regs.set_hl(0xC800);
    let mut b = bus(&[0x34]);
    b.mem[0xC800] = 0x0F;
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [Read(PC0, 0x34), Read(0xC800, 0x0F), Write(0xC800, 0x10)]
    );
    assert_eq!(c.regs.f(), flags::H);
}

#[test]
fn inc_dec_rp_trace_and_wrap() {
    let mut c = cpu();
    c.regs.sp = 0xFFFF;
    let mut b = bus(&[0x33, 0x3B]);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0, 0x33), Tick]);
    assert_eq!(c.regs.sp, 0x0000);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0 + 1, 0x3B), Tick]);
    assert_eq!(c.regs.sp, 0xFFFF);
    assert_eq!(c.regs.f(), 0); // no flags
}

// ----- 16-bit arithmetic -----

#[test]
fn add_hl_rp_flags_and_trace() {
    let mut c = cpu();
    c.regs.set_hl(0x0FFF);
    c.regs.set_bc(0x0001);
    c.regs.set_f(flags::Z); // Z must be preserved
    let mut b = bus(&[0x09]);
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0x09), Tick]);
    assert_eq!(c.regs.hl(), 0x1000);
    assert_eq!(c.regs.f(), flags::Z | flags::H);

    let mut c = cpu();
    c.regs.set_hl(0x8000);
    c.regs.set_de(0x8000);
    let mut b = bus(&[0x19]);
    step(&mut c, &mut b);
    assert_eq!(c.regs.hl(), 0x0000);
    assert_eq!(c.regs.f(), flags::C);

    // ADD HL,HL and ADD HL,SP
    let mut c = cpu();
    c.regs.set_hl(0x1234);
    let mut b = bus(&[0x29]);
    step(&mut c, &mut b);
    assert_eq!(c.regs.hl(), 0x2468);

    let mut c = cpu();
    c.regs.set_hl(0x0001);
    c.regs.sp = 0x00FF;
    let mut b = bus(&[0x39]);
    step(&mut c, &mut b);
    assert_eq!(c.regs.hl(), 0x0100);
}

#[test]
fn add_sp_e_timing_and_unsigned_low_byte_flags() {
    let mut c = cpu();
    c.regs.sp = 0x0FF8;
    let mut b = bus(&[0xE8, 0x08]);
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0xE8), Read(PC0 + 1, 0x08), Tick, Tick]);
    assert_eq!(c.regs.sp, 0x1000);
    assert_eq!(c.regs.f(), flags::H | flags::C);

    // negative offset: flags still from unsigned low-byte addition
    let mut c = cpu();
    c.regs.sp = 0xD000;
    let mut b = bus(&[0xE8, 0xFF]); // SP + (-1)
    step(&mut c, &mut b);
    assert_eq!(c.regs.sp, 0xCFFF);
    assert_eq!(c.regs.f(), 0); // 0x00 + 0xFF: no half-carry, no carry
}

#[test]
fn ld_hl_sp_e_timing_and_flags() {
    let mut c = cpu();
    c.regs.sp = 0xFFFF;
    let mut b = bus(&[0xF8, 0x01]);
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0xF8), Read(PC0 + 1, 0x01), Tick]);
    assert_eq!(c.regs.hl(), 0x0000);
    assert_eq!(c.regs.f(), flags::H | flags::C);
    assert_eq!(c.regs.sp, 0xFFFF); // SP unchanged

    let mut c = cpu();
    c.regs.sp = 0xD002;
    let mut b = bus(&[0xF8, 0xF8]); // SP + (-8)
    step(&mut c, &mut b);
    assert_eq!(c.regs.hl(), 0xCFFA);
    assert_eq!(c.regs.f(), 0);
}

// ----- accumulator rotates, DAA, misc flags -----

#[test]
fn rotate_a_ops_never_set_z() {
    let mut c = cpu();
    c.regs.a = 0x80;
    let mut b = bus(&[0x07]); // RLCA
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x01);
    assert_eq!(c.regs.f(), flags::C);

    let mut c = cpu();
    c.regs.a = 0x00;
    c.regs.set_f(0xF0);
    let mut b = bus(&[0x07]); // result 0 but Z stays clear
    step(&mut c, &mut b);
    assert_eq!(c.regs.f(), 0);

    let mut c = cpu();
    c.regs.a = 0x01;
    let mut b = bus(&[0x0F]); // RRCA
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x80);
    assert_eq!(c.regs.f(), flags::C);

    let mut c = cpu();
    c.regs.a = 0x80;
    c.regs.set_f(flags::C);
    let mut b = bus(&[0x17]); // RLA: carry in to bit 0
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x01);
    assert_eq!(c.regs.f(), flags::C);

    let mut c = cpu();
    c.regs.a = 0x01;
    c.regs.set_f(flags::C);
    let mut b = bus(&[0x1F]); // RRA: carry in to bit 7
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x80);
    assert_eq!(c.regs.f(), flags::C);
}

/// Independent DAA model for add mode (N=0), written from the algorithm
/// description in gbctr: accumulate the adjustment, apply it in one
/// step, carry set by the 0x60 correction. Subtract mode is
/// deliberately not re-derived here: a flag-based reference would share
/// `op_daa`'s structure and could not catch a shared misunderstanding.
/// It is checked against decimal arithmetic instead, in
/// `daa_after_sub_computes_bcd_difference_for_all_operands`.
fn daa_add_ref(a: u8, h: bool, c: bool) -> (u8, bool, bool) {
    let mut adjust = 0u8;
    let mut carry = c;
    if h || a & 0x0F > 0x09 {
        adjust += 0x06;
    }
    if c || a > 0x99 {
        adjust += 0x60;
        carry = true;
    }
    let r = a.wrapping_add(adjust);
    (r, r == 0, carry)
}

#[test]
fn daa_matches_reference_for_all_add_mode_inputs() {
    for fbits in 0..4u8 {
        let h = fbits & 1 != 0;
        let cf = fbits & 2 != 0;
        for a in 0..=255u8 {
            let mut c = cpu();
            c.regs.a = a;
            c.regs.set_f(fl(false, false, h, cf));
            let mut b = bus(&[0x27]);
            step(&mut c, &mut b);
            let (ra, rz, rc) = daa_add_ref(a, h, cf);
            let expect_f = fl(rz, false, false, rc);
            assert_eq!(c.regs.a, ra, "a={a:#04x} h={h} c={cf}");
            assert_eq!(c.regs.f(), expect_f, "a={a:#04x} h={h} c={cf}");
        }
    }
}

/// BCD property oracle for DAA's subtract mode, independent of the
/// flag-correction algorithm: for every valid packed-BCD operand pair,
/// SUB (and SBC with carry-in) followed by DAA must yield the decimal
/// difference modulo 100, with C set exactly on decimal borrow, N kept
/// and H cleared.
#[test]
fn daa_after_sub_computes_bcd_difference_for_all_operands() {
    let packed = |v: u8| (v / 10) << 4 | (v % 10);
    for x in 0..100u8 {
        for y in 0..100u8 {
            // SUB B: decimal x - y.
            let mut c = cpu();
            c.regs.a = packed(x);
            c.regs.b = packed(y);
            let mut b = bus(&[0x90, 0x27]); // SUB B; DAA
            step(&mut c, &mut b);
            step(&mut c, &mut b);
            let diff = (100 + x - y) % 100;
            let borrow = x < y;
            assert_eq!(c.regs.a, packed(diff), "sub x={x} y={y}");
            assert_eq!(
                c.regs.f(),
                fl(diff == 0, true, false, borrow),
                "sub x={x} y={y}"
            );

            // SBC B with carry in: decimal x - y - 1.
            let mut c = cpu();
            c.regs.a = packed(x);
            c.regs.b = packed(y);
            c.regs.set_f(fl(false, false, false, true));
            let mut b = bus(&[0x98, 0x27]); // SBC B; DAA
            step(&mut c, &mut b);
            step(&mut c, &mut b);
            let diff = (99 + x - y) % 100;
            let borrow = x <= y;
            assert_eq!(c.regs.a, packed(diff), "sbc x={x} y={y}");
            assert_eq!(
                c.regs.f(),
                fl(diff == 0, true, false, borrow),
                "sbc x={x} y={y}"
            );
        }
    }
}

#[test]
fn daa_bcd_examples() {
    // 0x15 + 0x27 = 0x42 BCD
    let mut c = cpu();
    c.regs.a = 0x15;
    c.regs.b = 0x27;
    let mut b = bus(&[0x80, 0x27]);
    step(&mut c, &mut b);
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x42);
    assert_eq!(c.regs.f(), 0);

    // 0x90 + 0x90 = 0x180 BCD: result 0x80 with carry
    let mut c = cpu();
    c.regs.a = 0x90;
    c.regs.b = 0x90;
    let mut b = bus(&[0x80, 0x27]);
    step(&mut c, &mut b);
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x80);
    assert_eq!(c.regs.f(), flags::C);

    // 0x20 - 0x13 = 0x07 BCD
    let mut c = cpu();
    c.regs.a = 0x20;
    c.regs.b = 0x13;
    let mut b = bus(&[0x90, 0x27]);
    step(&mut c, &mut b);
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0x07);
    assert_eq!(c.regs.f(), flags::N);
}

#[test]
fn cpl_scf_ccf() {
    let mut c = cpu();
    c.regs.a = 0x35;
    c.regs.set_f(flags::Z | flags::C);
    let mut b = bus(&[0x2F]); // CPL: Z,C preserved; N,H set
    step(&mut c, &mut b);
    assert_eq!(c.regs.a, 0xCA);
    assert_eq!(c.regs.f(), flags::Z | flags::N | flags::H | flags::C);

    let mut c = cpu();
    c.regs.set_f(flags::Z | flags::N | flags::H);
    let mut b = bus(&[0x37]); // SCF
    step(&mut c, &mut b);
    assert_eq!(c.regs.f(), flags::Z | flags::C);

    let mut c = cpu();
    c.regs.set_f(flags::N | flags::H | flags::C);
    let mut b = bus(&[0x3F]); // CCF: complement carry
    step(&mut c, &mut b);
    assert_eq!(c.regs.f(), 0);
    let mut b = bus(&[0x3F]);
    c.regs.pc = PC0;
    step(&mut c, &mut b);
    assert_eq!(c.regs.f(), flags::C);
}

// ----- jumps / calls / returns -----

#[test]
fn jp_nn_taken_and_cc_untaken() {
    let mut c = cpu();
    let mut b = bus(&[0xC3, 0x34, 0x12]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0, 0xC3),
            Read(PC0 + 1, 0x34),
            Read(PC0 + 2, 0x12),
            Tick
        ]
    );
    assert_eq!(c.regs.pc, 0x1234);

    let mut c = cpu(); // Z clear -> JP Z untaken
    let mut b = bus(&[0xCA, 0x34, 0x12]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [Read(PC0, 0xCA), Read(PC0 + 1, 0x34), Read(PC0 + 2, 0x12)]
    );
    assert_eq!(c.regs.pc, PC0 + 3);
}

#[test]
fn jp_hl_is_one_cycle() {
    let mut c = cpu();
    c.regs.set_hl(0x1234);
    let mut b = bus(&[0xE9]);
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0xE9)]);
    assert_eq!(c.regs.pc, 0x1234);
}

#[test]
fn jr_taken_negative_offset_and_untaken() {
    let mut c = cpu();
    let mut b = bus(&[0x18, 0xFE]); // JR -2: back to itself
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0x18), Read(PC0 + 1, 0xFE), Tick]);
    assert_eq!(c.regs.pc, PC0);

    let mut c = cpu(); // Z clear -> JR Z untaken: no internal cycle
    let mut b = bus(&[0x28, 0x05]);
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0x28), Read(PC0 + 1, 0x05)]);
    assert_eq!(c.regs.pc, PC0 + 2);

    let mut c = cpu(); // C clear -> JR NC taken
    let mut b = bus(&[0x30, 0x05]);
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0x30), Read(PC0 + 1, 0x05), Tick]);
    assert_eq!(c.regs.pc, PC0 + 7);
}

#[test]
fn call_nn_exact_event_order() {
    // gbctr: fetch, read lo, read hi, internal, push hi, push lo.
    let mut c = cpu();
    let mut b = bus(&[0xCD, 0x34, 0x12]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0, 0xCD),
            Read(PC0 + 1, 0x34),
            Read(PC0 + 2, 0x12),
            Tick,
            Write(SP0 - 1, 0xC0),
            Write(SP0 - 2, 0x03)
        ]
    );
    assert_eq!(c.regs.pc, 0x1234);
    assert_eq!(c.regs.sp, SP0 - 2);
}

#[test]
fn call_cc_taken_and_untaken() {
    let mut c = cpu(); // Z clear: CALL NZ taken
    let mut b = bus(&[0xC4, 0x34, 0x12]);
    step(&mut c, &mut b);
    assert_eq!(b.log.len(), 6);
    assert_eq!(c.regs.pc, 0x1234);

    let mut c = cpu(); // CALL Z untaken: 3 cycles, no pushes
    let mut b = bus(&[0xCC, 0x34, 0x12]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [Read(PC0, 0xCC), Read(PC0 + 1, 0x34), Read(PC0 + 2, 0x12)]
    );
    assert_eq!(c.regs.pc, PC0 + 3);
    assert_eq!(c.regs.sp, SP0);
}

#[test]
fn ret_and_ret_cc_traces() {
    let mut c = cpu();
    let mut b = bus(&[0xC9]);
    b.load(SP0, &[0x34, 0x12]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [Read(PC0, 0xC9), Read(SP0, 0x34), Read(SP0 + 1, 0x12), Tick]
    );
    assert_eq!(c.regs.pc, 0x1234);

    let mut c = cpu(); // RET NZ taken (Z clear): 5 cycles
    let mut b = bus(&[0xC0]);
    b.load(SP0, &[0x34, 0x12]);
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0, 0xC0),
            Tick,
            Read(SP0, 0x34),
            Read(SP0 + 1, 0x12),
            Tick
        ]
    );
    assert_eq!(c.regs.pc, 0x1234);

    let mut c = cpu(); // RET Z untaken: 2 cycles
    let mut b = bus(&[0xC8]);
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0xC8), Tick]);
    assert_eq!(c.regs.pc, PC0 + 1);
    assert_eq!(c.regs.sp, SP0);
}

#[test]
fn rst_timing_like_call_tail() {
    let mut c = cpu();
    let mut b = bus(&[0xEF]); // RST 28h
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0, 0xEF),
            Tick,
            Write(SP0 - 1, 0xC0),
            Write(SP0 - 2, 0x01)
        ]
    );
    assert_eq!(c.regs.pc, 0x0028);
}

// ----- CB-prefixed -----

#[test]
fn cb_register_op_is_two_cycles() {
    let mut c = cpu();
    c.regs.c = 0x88;
    let mut b = bus(&[0xCB, 0x11]); // RL C
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0xCB), Read(PC0 + 1, 0x11)]);
    assert_eq!(c.regs.c, 0x10);
    assert_eq!(c.regs.f(), flags::C);
}

#[test]
fn cb_rot_kinds_results() {
    // (kind opcode on B, input, carry-in, output, carry-out)
    for (op, input, cin, out, cout) in [
        (0x00u8, 0x85u8, false, 0x0Bu8, true), // RLC
        (0x08, 0x01, false, 0x80, true),       // RRC
        (0x10, 0x80, true, 0x01, true),        // RL
        (0x18, 0x01, true, 0x80, true),        // RR
        (0x20, 0xC0, false, 0x80, true),       // SLA
        (0x28, 0x81, false, 0xC0, true),       // SRA keeps bit 7
        (0x30, 0xA5, true, 0x5A, false),       // SWAP clears C
        (0x38, 0x81, false, 0x40, true),       // SRL
    ] {
        let mut c = cpu();
        c.regs.b = input;
        c.regs.set_f(if cin { flags::C } else { 0 });
        let mut b = bus(&[0xCB, op]);
        step(&mut c, &mut b);
        assert_eq!(c.regs.b, out, "op={op:#04x}");
        assert_eq!(c.regs.f(), fl(out == 0, false, false, cout), "op={op:#04x}");
    }
    // Z set by CB rotates (unlike RLCA-family)
    let mut c = cpu();
    c.regs.b = 0;
    let mut b = bus(&[0xCB, 0x00]);
    step(&mut c, &mut b);
    assert_eq!(c.regs.f(), flags::Z);
}

#[test]
fn cb_hl_read_modify_write_is_four_cycles() {
    let mut c = cpu();
    c.regs.set_hl(0xC800);
    let mut b = bus(&[0xCB, 0x26]); // SLA (HL)
    b.mem[0xC800] = 0x81;
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0, 0xCB),
            Read(PC0 + 1, 0x26),
            Read(0xC800, 0x81),
            Write(0xC800, 0x02)
        ]
    );
    assert_eq!(c.regs.f(), flags::C);
}

#[test]
fn bit_hl_is_three_cycles_and_flags() {
    let mut c = cpu();
    c.regs.set_hl(0xC800);
    c.regs.set_f(flags::C);
    let mut b = bus(&[0xCB, 0x7E]); // BIT 7,(HL)
    b.mem[0xC800] = 0x7F;
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [Read(PC0, 0xCB), Read(PC0 + 1, 0x7E), Read(0xC800, 0x7F)]
    );
    // bit 7 clear -> Z set; H set; C preserved
    assert_eq!(c.regs.f(), flags::Z | flags::H | flags::C);

    let mut c = cpu();
    c.regs.h = 0x10;
    let mut b = bus(&[0xCB, 0x64]); // BIT 4,H -> set, Z clear
    step(&mut c, &mut b);
    assert_eq!(c.regs.f(), flags::H);
}

#[test]
fn res_set_hl_are_four_cycles() {
    let mut c = cpu();
    c.regs.set_hl(0xC800);
    c.regs.set_f(0xF0);
    let mut b = bus(&[0xCB, 0x86, 0xCB, 0xFE]); // RES 0,(HL); SET 7,(HL)
    b.mem[0xC800] = 0xFF;
    step(&mut c, &mut b);
    assert_eq!(
        b.take_log(),
        [
            Read(PC0, 0xCB),
            Read(PC0 + 1, 0x86),
            Read(0xC800, 0xFF),
            Write(0xC800, 0xFE)
        ]
    );
    assert_eq!(c.regs.f(), 0xF0); // RES/SET touch no flags
    b.mem[0xC800] = 0x00;
    step(&mut c, &mut b);
    assert_eq!(
        b.take_log(),
        [
            Read(PC0 + 2, 0xCB),
            Read(PC0 + 3, 0xFE),
            Read(0xC800, 0x00),
            Write(0xC800, 0x80)
        ]
    );
}

// ----- EI / DI / IME sequencing -----

#[test]
fn ei_enables_after_following_instruction() {
    // mooneye acceptance/ei_timing: exactly one instruction after EI runs
    // before the interrupt is taken.
    let mut c = cpu();
    let mut b = bus(&[0xFB, 0x04, 0x04]); // EI; INC B; INC B
    b.mem[0xFFFF] = 0x08;
    b.mem[0xFF0F] = 0x08;
    step(&mut c, &mut b); // EI
    assert!(!c.ime);
    step(&mut c, &mut b); // INC B (no dispatch before it)
    assert_eq!(c.regs.b, 1);
    assert!(c.ime);
    b.take_log();
    step(&mut c, &mut b); // dispatch to 0x58
    assert_eq!(
        b.log,
        [
            Read(PC0 + 2, 0x04), // aborted fetch: discarded, PC kept
            Tick,
            Tick,
            Write(SP0 - 1, 0xC0),
            Write(SP0 - 2, 0x02),
            Read(0x0058, 0x00)
        ]
    );
    assert_eq!(c.regs.b, 1); // second INC B did not run
    assert!(!c.ime);
    assert_eq!(b.mem[0xFF0F], 0x00); // serial bit acked
}

#[test]
fn ei_di_leaves_ime_off() {
    // mooneye acceptance/rapid_di_ei: EI directly followed by DI never
    // lets an interrupt through.
    let mut c = cpu();
    let mut b = bus(&[0xFB, 0xF3, 0x00]); // EI; DI; NOP
    b.mem[0xFFFF] = 0x08;
    b.mem[0xFF0F] = 0x08;
    step(&mut c, &mut b);
    step(&mut c, &mut b);
    assert!(!c.ime);
    assert!(!c.ime_pending);
    b.take_log();
    step(&mut c, &mut b); // NOP, no dispatch
    assert_eq!(b.log, [Read(PC0 + 2, 0x00)]);
    assert_eq!(b.mem[0xFF0F], 0x08); // untouched
}

#[test]
fn ei_ei_dispatches_after_second_ei() {
    // mooneye acceptance/ei_sequence: with back-to-back EIs the interrupt
    // is taken right after the *second* EI; the pushed return address is
    // the byte after it.
    let mut c = cpu();
    let mut b = bus(&[0xFB, 0xFB, 0xF3]); // EI; EI; DI
    b.mem[0xFFFF] = 0x08;
    b.mem[0xFF0F] = 0x08;
    step(&mut c, &mut b); // EI #1
    step(&mut c, &mut b); // EI #2; IME commits after it
    assert!(c.ime);
    b.take_log();
    step(&mut c, &mut b); // dispatch; DI never runs
    assert_eq!(
        b.log,
        [
            Read(PC0 + 2, 0xF3), // aborted fetch: DI is discarded
            Tick,
            Tick,
            Write(SP0 - 1, 0xC0),
            Write(SP0 - 2, 0x02),
            Read(0x0058, 0x00)
        ]
    );
    assert!(!c.ime); // handler runs with IME off
}

#[test]
fn ei_ei_di_without_pending_leaves_ime_off() {
    let mut c = cpu();
    let mut b = bus(&[0xFB, 0xFB, 0xF3]);
    step(&mut c, &mut b);
    step(&mut c, &mut b);
    assert!(c.ime);
    step(&mut c, &mut b); // DI
    assert!(!c.ime);
    assert!(!c.ime_pending);
}

#[test]
fn di_takes_effect_immediately() {
    let mut c = cpu();
    c.ime = true;
    let mut b = bus(&[0xF3, 0x00]);
    step(&mut c, &mut b); // DI
    assert!(!c.ime);
    b.mem[0xFFFF] = 0x01;
    b.mem[0xFF0F] = 0x01;
    b.take_log();
    step(&mut c, &mut b); // NOP, no dispatch
    assert_eq!(b.log, [Read(PC0 + 1, 0x00)]);
}

#[test]
fn reti_sets_ime_immediately() {
    // mooneye acceptance/reti_intr_timing: an interrupt pending when RETI
    // executes is dispatched right at the next instruction boundary.
    let mut c = cpu();
    let mut b = bus(&[0xD9]);
    b.load(SP0, &[0x00, 0xC1]); // return to 0xC100
    b.mem[0xFFFF] = 0x01;
    b.mem[0xFF0F] = 0x01;
    step(&mut c, &mut b);
    assert_eq!(
        b.take_log(),
        [Read(PC0, 0xD9), Read(SP0, 0x00), Read(SP0 + 1, 0xC1), Tick]
    );
    assert!(c.ime);
    assert_eq!(c.regs.pc, 0xC100);
    step(&mut c, &mut b); // immediate dispatch, return address 0xC100
    assert_eq!(
        b.log,
        [
            Read(0xC100, 0x00), // aborted fetch at the return address
            Tick,
            Tick,
            Write(SP0 + 1, 0xC1),
            Write(SP0, 0x00),
            Read(0x0040, 0x00)
        ]
    );
}

// ----- interrupt dispatch -----

#[test]
fn dispatch_trace_priority_and_ack() {
    let mut c = cpu();
    c.ime = true;
    c.regs.pc = 0xC123;
    let mut b = TestBus::new();
    b.mem[0xFFFF] = 0x1F;
    b.mem[0xFF0F] = 0x14; // timer (bit 2) + joypad (bit 4)
    step(&mut c, &mut b);
    // timer wins (lowest bit number = highest priority) -> vector 0x50
    assert_eq!(
        b.log,
        [
            Read(0xC123, 0x00), // aborted fetch: discarded, PC kept
            Tick,
            Tick,
            Write(SP0 - 1, 0xC1),
            Write(SP0 - 2, 0x23),
            Read(0x0050, 0x00)
        ]
    );
    assert_eq!(c.regs.pc, 0x0051); // NOP at vector already executed
    assert!(!c.ime);
    assert_eq!(b.mem[0xFF0F], 0x10); // only the timer bit acked
}

#[test]
fn dispatch_only_when_enabled_in_ie() {
    let mut c = cpu();
    c.ime = true;
    let mut b = bus(&[0x00]);
    b.mem[0xFFFF] = 0x02;
    b.mem[0xFF0F] = 0x01; // pending but masked
    step(&mut c, &mut b);
    assert_eq!(b.log, [Read(PC0, 0x00)]);
}

#[test]
fn ie_push_high_byte_cancels_dispatch() {
    // mooneye acceptance/interrupts/ie_push round 1: the PC-high push
    // overwrites IE and clears the only pending bit; dispatch is
    // cancelled, PC := 0x0000, IF is *not* modified, IME stays off.
    let mut c = cpu();
    c.ime = true;
    c.regs.pc = 0x0212; // high byte 0x02 lands in IE
    c.regs.sp = 0x0000;
    let mut b = TestBus::new();
    b.mem[0xFFFF] = 0x04; // timer enabled
    b.mem[0xFF0F] = 0x04; // timer pending
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(0x0212, 0x00), // aborted fetch
            Tick,
            Tick,
            Write(0xFFFF, 0x02), // IE := 0x02, timer no longer enabled
            Write(0xFFFE, 0x12),
            Read(0x0000, 0x00)
        ]
    );
    assert_eq!(c.regs.pc, 0x0001);
    assert_eq!(b.mem[0xFF0F], 0x04); // IF untouched
    assert!(!c.ime); // round 2: IME stays 0 after cancellation
}

#[test]
fn ie_push_low_byte_is_too_late_to_cancel() {
    // ie_push round 3: IE is only clobbered by the PC-low push; the
    // interrupt was already chosen and acked after the high push.
    let mut c = cpu();
    c.ime = true;
    c.regs.pc = 0x0212; // low byte 0x12 clears IE bit 3 - too late
    c.regs.sp = 0x0001;
    let mut b = TestBus::new();
    b.mem[0xFFFF] = 0x08; // serial
    b.mem[0xFF0F] = 0x08;
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(0x0212, 0x00), // aborted fetch
            Tick,
            Tick,
            Write(0x0000, 0x02),
            Write(0xFFFF, 0x12),
            Read(0x0058, 0x00)
        ]
    );
    assert_eq!(c.regs.pc, 0x0059);
    assert_eq!(b.mem[0xFF0F], 0x00); // IF cleared: dispatch went through
}

#[test]
fn ie_push_high_byte_redirects_to_remaining_interrupt() {
    // ie_push round 4: the high push rewrites IE keeping a different
    // pending bit enabled; that interrupt is dispatched instead.
    let mut c = cpu();
    c.ime = true;
    c.regs.pc = 0x0212; // IE := 0x02 keeps STAT enabled
    c.regs.sp = 0x0000;
    let mut b = TestBus::new();
    b.mem[0xFFFF] = 0x03; // vblank + stat
    b.mem[0xFF0F] = 0x03;
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(0x0212, 0x00), // aborted fetch
            Tick,
            Tick,
            Write(0xFFFF, 0x02),
            Write(0xFFFE, 0x12),
            Read(0x0048, 0x00) // STAT vector
        ]
    );
    assert_eq!(b.mem[0xFF0F], 0x01); // STAT acked, vblank still pending
}

// ----- HALT -----

#[test]
fn halt_ime1_waits_then_dispatches() {
    let mut c = cpu();
    c.ime = true;
    let mut b = bus(&[0x76]);
    b.mem[0xFFFF] = 0x04;
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0, 0x76)]);
    assert!(c.halted);
    step(&mut c, &mut b);
    step(&mut c, &mut b);
    // halted: one discarded prefetch read per idle M-cycle
    assert_eq!(b.take_log(), [Read(PC0 + 1, 0x00), Read(PC0 + 1, 0x00)]);
    b.mem[0xFF0F] = 0x04;
    step(&mut c, &mut b);
    assert_eq!(
        b.log,
        [
            Read(PC0 + 1, 0x00), // prefetch observing IF, aborted
            Tick,
            Tick,
            Write(SP0 - 1, 0xC0),
            Write(SP0 - 2, 0x01), // return address = after HALT
            Read(0x0050, 0x00)
        ]
    );
    assert!(!c.halted);
    assert_eq!(b.mem[0xFF0F], 0);
}

#[test]
fn halt_ime0_continues_without_dispatch() {
    let mut c = cpu();
    let mut b = bus(&[0x76, 0x04]); // HALT; INC B
    b.mem[0xFFFF] = 0x04;
    step(&mut c, &mut b);
    assert!(c.halted);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0, 0x76), Read(PC0 + 1, 0x04)]);
    b.mem[0xFF0F] = 0x04;
    step(&mut c, &mut b);
    // wakes with no extra delay and just executes the next instruction
    assert_eq!(b.log, [Read(PC0 + 1, 0x04)]);
    assert_eq!(c.regs.b, 1);
    assert_eq!(b.mem[0xFF0F], 0x04); // IF not acked
    assert!(!c.ime);
}

#[test]
fn halt_ime1_dispatch_reuses_the_if_raising_cycle_as_prefetch() {
    // Servicing an interrupt out of HALT takes "exactly same timing as
    // if a long series of NOP instructions were used to wait for the
    // interrupt" (mooneye acceptance/halt_ime1_timing2-GS, verified on
    // DMG/MGB/SGB/SGB2). In the NOP case the M-cycle whose tick raises
    // IF is the aborted prefetch and the vector fetch lands 5 M-cycles
    // later; the halt idle cycle that observes IF must therefore play
    // the same role - no extra fetch cycle in between.
    let mut c = cpu();
    c.ime = true;
    let mut b = bus(&[0x76]); // HALT
    b.mem[0xFFFF] = 0x04; // timer interrupt enabled
    step(&mut c, &mut b); // cycle 0: HALT fetch
    assert_eq!(b.take_log(), [Read(PC0, 0x76)]);
    assert!(c.halted);
    b.raise_if = Some((3, 0x04)); // IF.2 rises during cycle 3
    step(&mut c, &mut b); // cycle 1: idle
    step(&mut c, &mut b); // cycle 2: idle
    assert!(c.halted);
    b.take_log();
    // Cycle 3 sees IF rise -> it is the aborted prefetch; dispatch
    // follows in the same step and the vector fetch is cycle 8 = 3+5.
    step(&mut c, &mut b);
    assert_eq!(
        b.take_log(),
        [
            Read(PC0 + 1, 0x00),  // cycle 3: IF rises during this cycle
            Tick,                 // cycle 4
            Tick,                 // cycle 5
            Write(SP0 - 1, 0xC0), // cycle 6
            Write(SP0 - 2, 0x01), // cycle 7: return addr = after HALT
            Read(0x0050, 0x00),   // cycle 8: vector fetch, IF cycle + 5
        ]
    );
    assert!(!c.halted);
    assert!(!c.ime);
    assert_eq!(b.mem[0xFF0F], 0); // acked
    assert_eq!(c.regs.pc, 0x0051);
}

#[test]
fn halt_ime0_wake_fetch_is_the_if_raising_cycle() {
    // IME=0 wake: HALT continues "with exactly same timing as if a long
    // series of NOP instructions were used" (mooneye acceptance/
    // halt_ime0_nointr_timing, verified on all models): the cycle whose
    // tick raises IF would be a NOP fetch in the wait loop, so out of
    // halt it is already the next instruction's opcode fetch.
    let mut c = cpu();
    let mut b = bus(&[0x76, 0x04]); // HALT; INC B
    b.mem[0xFFFF] = 0x04;
    step(&mut c, &mut b); // cycle 0: HALT fetch
    b.raise_if = Some((2, 0x04)); // IF.2 rises during cycle 2
    step(&mut c, &mut b); // cycle 1: idle
    assert!(c.halted);
    b.take_log();
    step(&mut c, &mut b); // cycle 2: fetches and executes INC B
    assert_eq!(b.take_log(), [Read(PC0 + 1, 0x04)]);
    assert!(!c.halted);
    assert_eq!(c.regs.b, 1);
    assert_eq!(b.mem[0xFF0F], 0x04); // not acked: no dispatch
}

#[test]
fn halt_ime1_late_if_commit_wakes_one_cycle_later() {
    // A timer IF committed on the last T-substep of an M-cycle is missed
    // by that cycle's halt-exit sampling (Bus::pending_halt_wake): the
    // *next* idle prefetch becomes the aborted dispatch prefetch, so the
    // vector fetch lands 6 M-cycles after the commit cycle instead of 5
    // (gambatte tima/tc*_irq_*; wilbertpol acceptance/timer/timer_if
    // rounds 5/6 — dispatch-from-HALT — vs rounds 3/4 — dispatch from a
    // NOP sled, which keeps the end-of-fetch sampling and +5).
    let mut c = cpu();
    c.ime = true;
    let mut b = bus(&[0x76]); // HALT
    b.mem[0xFFFF] = 0x04;
    step(&mut c, &mut b); // cycle 0: HALT fetch
    assert!(c.halted);
    b.late_if = true;
    b.raise_if = Some((3, 0x04)); // last-substep commit during cycle 3
    step(&mut c, &mut b); // cycle 1: idle
    step(&mut c, &mut b); // cycle 2: idle
    b.take_log();
    step(&mut c, &mut b); // cycle 3: IF commits too late to be seen
    assert!(c.halted, "the commit cycle's wake check misses the IF bit");
    step(&mut c, &mut b); // cycle 4: wake -> aborted prefetch + dispatch
    assert_eq!(
        b.take_log(),
        [
            Read(PC0 + 1, 0x00),  // cycle 3: still idle
            Read(PC0 + 1, 0x00),  // cycle 4: aborted dispatch prefetch
            Tick,                 // cycle 5
            Tick,                 // cycle 6
            Write(SP0 - 1, 0xC0), // cycle 7
            Write(SP0 - 2, 0x01), // cycle 8: return addr = after HALT
            Read(0x0050, 0x00),   // cycle 9: vector fetch = commit + 6
        ]
    );
    assert!(!c.halted);
    assert_eq!(b.mem[0xFF0F], 0); // acked
}

#[test]
fn halt_ime0_late_if_commit_also_wakes_one_cycle_later() {
    // The same intra-cycle sample feeds the IME=0 resume (SameBoy
    // sm83_cpu.c `GB_cpu_run`: one `interrupt_queue` sample serves both
    // wake paths): a late IF commit keeps the IME=0 halt asleep for one
    // more idle cycle too. Mooneye halt_ime0_nointr_timing stays exact
    // because its wake source is the vblank IF, which is not a late
    // commit (it also anchors its DIV reset with the same one-cycle
    // shift, so even a shifted source would cancel out there).
    let mut c = cpu();
    let mut b = bus(&[0x76, 0x04]); // HALT; INC B
    b.mem[0xFFFF] = 0x04;
    step(&mut c, &mut b); // cycle 0: HALT fetch
    b.late_if = true;
    b.raise_if = Some((2, 0x04)); // second-half commit during cycle 2
    step(&mut c, &mut b); // cycle 1: idle
    assert!(c.halted);
    b.take_log();
    step(&mut c, &mut b); // cycle 2: commit invisible to the wake check
    assert!(c.halted, "the commit cycle's wake check misses the IF bit");
    step(&mut c, &mut b); // cycle 3: wakes and executes INC B
    assert_eq!(
        b.take_log(),
        [Read(PC0 + 1, 0x04), Read(PC0 + 1, 0x04)] // cycles 2 and 3
    );
    assert!(!c.halted);
    assert_eq!(c.regs.b, 1);
    assert_eq!(b.mem[0xFF0F], 0x04); // not acked: no dispatch
}

#[test]
fn halt_gates_core_clock_after_the_post_halt_prefetch() {
    // The core clock gate (Bus::set_halted — the OAM DMA controller
    // freezes with it) engages only after the post-HALT prefetch
    // M-cycle, not on HALT execution: the SM83 prefetches the next
    // opcode before sleeping (gbctr halt bug prefetch), and madness/
    // mgb_oam_dma_halt_sprites.s pins the hardware OAM DMA freeze two
    // M-cycles after the HALT opcode fetch.
    let mut c = cpu();
    let mut b = bus(&[0x76, 0x04]); // HALT; INC B
    b.mem[0xFFFF] = 0x04;
    step(&mut c, &mut b); // cycle 0: HALT fetch
    assert!(c.halted);
    assert!(b.halt_calls.is_empty(), "no gate during HALT itself");
    step(&mut c, &mut b); // cycle 1: idle prefetch, then the gate engages
    assert_eq!(b.halt_calls, [(2, true)]);
    step(&mut c, &mut b); // cycle 2: idle, gate stays on (idempotent)
    step(&mut c, &mut b); // cycle 3
    assert_eq!(b.halt_calls, [(2, true)]);
    b.raise_if = Some((4, 0x04)); // IF.2 rises during cycle 4
    step(&mut c, &mut b); // cycle 4 observes IF: wake, gate released
    assert!(!c.halted);
    assert_eq!(b.halt_calls, [(2, true), (5, false)]);
    assert_eq!(c.regs.b, 1, "woke into INC B");
}

#[test]
fn halt_with_immediate_wake_never_gates_the_clock() {
    let mut c = cpu();
    let mut b = bus(&[0x76, 0x04]); // HALT; INC B
    b.mem[0xFFFF] = 0x04;
    step(&mut c, &mut b); // HALT fetch
    b.raise_if = Some((1, 0x04)); // IF rises during the first prefetch
    step(&mut c, &mut b); // prefetch observes IF: immediate wake
    assert!(!c.halted);
    assert!(b.halt_calls.is_empty(), "clock never stopped");
}

#[test]
fn stop_gates_core_clock_after_the_first_idle_cycle() {
    // Stop mode switches the same core clock gate (and so freezes an
    // in-flight OAM DMA the same way halt mode does).
    let mut c = cpu();
    let mut b = bus(&[0x10, 0x00, 0x04]); // STOP; (skipped); INC B
    step(&mut c, &mut b); // cycle 0: STOP fetch
    assert!(c.stopped);
    assert!(b.halt_calls.is_empty(), "no gate during STOP itself");
    step(&mut c, &mut b); // cycle 1: idle tick, then the gate engages
    assert_eq!(b.halt_calls, [(2, true)]);
    step(&mut c, &mut b); // cycle 2: idle
    assert_eq!(b.halt_calls, [(2, true)]);
    // Joypad wake: the gate is released before execution resumes.
    b.mem[0xFFFF] = 0x10;
    b.mem[0xFF0F] = 0x10;
    step(&mut c, &mut b); // wakes, executes INC B
    assert!(!c.stopped);
    assert_eq!(b.halt_calls, [(2, true), (3, false)]);
    assert_eq!(c.regs.b, 1);
}

#[test]
fn halt_bug_fetches_next_opcode_twice() {
    // HALT with IME=0 while IE & IF != 0: PC fails to increment for the
    // following opcode fetch (gbctr).
    let mut c = cpu();
    let mut b = bus(&[0x76, 0x3C]); // HALT; INC A
    b.mem[0xFFFF] = 0x04;
    b.mem[0xFF0F] = 0x04;
    step(&mut c, &mut b);
    assert!(!c.halted);
    assert!(c.halt_bug);
    step(&mut c, &mut b);
    assert_eq!(c.regs.pc, PC0 + 1); // PC stuck
    assert_eq!(c.regs.a, 1);
    step(&mut c, &mut b);
    assert_eq!(c.regs.pc, PC0 + 2); // now it advances
    assert_eq!(c.regs.a, 2); // INC A ran twice
    assert_eq!(
        b.log,
        [Read(PC0, 0x76), Read(PC0 + 1, 0x3C), Read(PC0 + 1, 0x3C)]
    );
}

#[test]
fn halt_bug_with_multibyte_instruction_reads_opcode_as_operand() {
    let mut c = cpu();
    let mut b = bus(&[0x76, 0x3E, 0x99]); // HALT; LD A,n
    b.mem[0xFFFF] = 0x01;
    b.mem[0xFF0F] = 0x01;
    step(&mut c, &mut b);
    step(&mut c, &mut b);
    // LD A,n reads its own opcode byte as the operand
    assert_eq!(c.regs.a, 0x3E);
    assert_eq!(c.regs.pc, PC0 + 2);
}

#[test]
fn ei_halt_with_pending_behaves_like_ime1() {
    // mooneye acceptance/halt_ime0_ei: EI directly before HALT means the
    // delayed enable commits while halting; the interrupt dispatches with
    // the return address after the HALT - no halt bug.
    let mut c = cpu();
    let mut b = bus(&[0xFB, 0x76]); // EI; HALT
    b.mem[0xFFFF] = 0x01;
    b.mem[0xFF0F] = 0x01;
    step(&mut c, &mut b); // EI
    step(&mut c, &mut b); // HALT: no halt bug despite pending + IME=0
    assert!(c.halted);
    assert!(!c.halt_bug);
    assert!(c.ime);
    b.take_log();
    step(&mut c, &mut b); // dispatch out of halt
    assert_eq!(
        b.log,
        [
            Read(PC0 + 2, 0x00), // aborted fetch after the halt wakes
            Tick,
            Tick,
            Write(SP0 - 1, 0xC0),
            Write(SP0 - 2, 0x02),
            Read(0x0040, 0x00)
        ]
    );
}

#[test]
fn halted_cpu_consumes_idle_prefetch_cycles() {
    // Each halted M-cycle is a discarded prefetch read of PC (so the
    // cycle that eventually observes IF can double as the real fetch).
    let mut c = cpu();
    c.halted = true;
    let mut b = TestBus::new();
    for _ in 0..5 {
        step(&mut c, &mut b);
    }
    assert_eq!(b.log, [Read(PC0, 0); 5]);
    assert!(c.halted);
}

// ----- STOP -----

#[test]
fn stop_skips_following_byte_and_sleeps_until_joypad_wake() {
    // Pan Docs STOP flowchart: no interrupt pending -> 2-byte opcode;
    // bus.stop() == false -> deep stop, the CPU burns tick cycles like
    // halt until the joypad interrupt becomes pending.
    let mut c = cpu();
    let mut b = bus(&[0x10, 0x00, 0x04]); // STOP; (skipped); INC B
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0, 0x10)]);
    assert_eq!(b.stop_calls, 1);
    assert_eq!(c.regs.pc, PC0 + 2);
    assert!(c.stopped);
    for _ in 0..3 {
        step(&mut c, &mut b);
    }
    assert_eq!(b.take_log(), [Tick, Tick, Tick]);
    assert_eq!(c.regs.pc, PC0 + 2);
    // Joypad interrupt wakes it; execution resumes after the skipped
    // byte with no extra delay cycles.
    b.mem[0xFFFF] = 0x10;
    b.mem[0xFF0F] = 0x10;
    step(&mut c, &mut b);
    assert!(!c.stopped);
    assert_eq!(b.log, [Read(PC0 + 2, 0x04)]);
    assert_eq!(c.regs.b, 1);
}

#[test]
fn stop_with_speed_switch_continues_normally() {
    // bus.stop() == true: the bus performed the armed CGB speed switch,
    // so the CPU does not enter stop mode and keeps executing.
    let mut c = cpu();
    let mut b = bus(&[0x10, 0x00, 0x04]);
    b.stop_result = true;
    step(&mut c, &mut b);
    assert_eq!(b.stop_calls, 1);
    assert_eq!(c.regs.pc, PC0 + 2);
    assert!(!c.stopped);
    step(&mut c, &mut b);
    assert_eq!(c.regs.b, 1);
}

#[test]
fn stop_with_pending_interrupt_is_one_byte_opcode() {
    // Pan Docs STOP flowchart: with IE & IF != 0, STOP is a 1-byte
    // opcode - the byte after it is executed, not skipped.
    let mut c = cpu();
    let mut b = bus(&[0x10, 0x04]); // STOP; INC B
    b.mem[0xFFFF] = 0x10;
    b.mem[0xFF0F] = 0x10;
    step(&mut c, &mut b);
    assert_eq!(c.regs.pc, PC0 + 1);
    // The already-pending interrupt also ends the stop immediately
    // (IME=0, so it is not dispatched).
    step(&mut c, &mut b);
    assert!(!c.stopped);
    assert_eq!(c.regs.b, 1);
}

// ----- illegal opcodes -----

#[test]
fn illegal_opcode_locks_cpu_forever() {
    let mut c = cpu();
    let mut b = bus(&[0xD3, 0x04]);
    step(&mut c, &mut b);
    assert_eq!(b.take_log(), [Read(PC0, 0xD3)]);
    assert!(c.locked);
    let regs = c.regs;
    for _ in 0..3 {
        step(&mut c, &mut b);
    }
    assert_eq!(b.log, [Tick, Tick, Tick]);
    assert_eq!(c.regs, regs);
    // not even interrupts get it out
    b.mem[0xFFFF] = 0x01;
    b.mem[0xFF0F] = 0x01;
    c.ime = true;
    b.take_log();
    step(&mut c, &mut b);
    assert_eq!(b.log, [Tick]);
}

#[test]
fn all_illegal_opcodes_lock() {
    for op in [
        0xD3u8, 0xDB, 0xDD, 0xE3, 0xE4, 0xEB, 0xEC, 0xED, 0xF4, 0xFC, 0xFD,
    ] {
        let mut c = cpu();
        let mut b = bus(&[op]);
        step(&mut c, &mut b);
        assert!(c.locked, "opcode {op:#04x} must lock");
    }
}

/// The lock state is exposed to harnesses (wilbertpol's mooneye fork ends
/// its tests with 0xED) and is distinct from the LD B,B breakpoint.
#[test]
fn illegal_opcode_reports_debug_undefined_hit() {
    let mut c = cpu();
    let mut b = bus(&[0xED]);
    assert!(!c.debug_undefined_hit());
    step(&mut c, &mut b);
    assert!(c.debug_undefined_hit());
    assert!(!c.debug_breakpoint_hit());
}

// ----- whole-opcode-space cycle count sweeps -----

/// Expected M-cycles per base opcode, given the branch outcome of each
/// condition code. Numbers from the gbctr instruction tables.
fn base_cycles(op: u8, taken: impl Fn(u8) -> bool) -> usize {
    match op {
        0x01 | 0x11 | 0x21 | 0x31 => 3,
        0x02 | 0x12 | 0x22 | 0x32 | 0x0A | 0x1A | 0x2A | 0x3A => 2,
        0x03 | 0x13 | 0x23 | 0x33 | 0x0B | 0x1B | 0x2B | 0x3B => 2,
        0x34..=0x36 => 3,
        0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x3E => 2,
        0x08 => 5,
        0x09 | 0x19 | 0x29 | 0x39 => 2,
        0x18 => 3,
        0x20 | 0x28 | 0x30 | 0x38 => {
            if taken((op >> 3) & 3) {
                3
            } else {
                2
            }
        }
        0x76 => 1, // HALT (no pending interrupt in the sweep)
        0x40..=0x7F => {
            if (op >> 3) & 7 == 6 || op & 7 == 6 {
                2
            } else {
                1
            }
        }
        0x80..=0xBF => {
            if op & 7 == 6 {
                2
            } else {
                1
            }
        }
        0xC0 | 0xC8 | 0xD0 | 0xD8 => {
            if taken((op >> 3) & 3) {
                5
            } else {
                2
            }
        }
        0xC1 | 0xD1 | 0xE1 | 0xF1 => 3,
        0xC2 | 0xCA | 0xD2 | 0xDA => {
            if taken((op >> 3) & 3) {
                4
            } else {
                3
            }
        }
        0xC3 => 4,
        0xC4 | 0xCC | 0xD4 | 0xDC => {
            if taken((op >> 3) & 3) {
                6
            } else {
                3
            }
        }
        0xC5 | 0xD5 | 0xE5 | 0xF5 => 4,
        0xC6 | 0xCE | 0xD6 | 0xDE | 0xE6 | 0xEE | 0xF6 | 0xFE => 2,
        0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => 4,
        0xC9 | 0xD9 => 4,
        0xCD => 6,
        0xE0 | 0xF0 => 3,
        0xE2 | 0xF2 => 2,
        0xE8 => 4,
        0xE9 => 1,
        0xEA | 0xFA => 4,
        0xF8 => 3,
        0xF9 => 2,
        // 1-cycle ops, STOP, illegal opcodes (lock after the fetch)
        _ => 1,
    }
}

fn run_sweep(flags: u8) {
    let taken = |cc: u8| match cc {
        0 => flags & flags::Z == 0,
        1 => flags & flags::Z != 0,
        2 => flags & flags::C == 0,
        _ => flags & flags::C != 0,
    };
    for op in 0..=255u8 {
        if op == 0xCB {
            continue; // prefix, swept separately
        }
        let mut c = cpu();
        c.regs.set_f(flags);
        c.regs.set_hl(0xC800);
        c.regs.set_bc(0xC700);
        c.regs.set_de(0xC701);
        let mut b = bus(&[op, 0x00, 0x00]);
        step(&mut c, &mut b);
        assert_eq!(
            b.log.len(),
            base_cycles(op, taken),
            "opcode {op:#04x} flags {flags:#04x}"
        );
        assert_eq!(
            c.regs.f() & 0x0F,
            0,
            "opcode {op:#04x} dirtied F low nibble"
        );
    }
}

#[test]
fn cycle_counts_all_base_opcodes_flags_clear() {
    run_sweep(0x00);
}

#[test]
fn cycle_counts_all_base_opcodes_flags_set() {
    run_sweep(flags::Z | flags::C);
}

#[test]
fn cycle_counts_all_cb_opcodes() {
    for op in 0..=255u8 {
        let expected = if op & 7 != 6 {
            2
        } else if (0x40..=0x7F).contains(&op) {
            3 // BIT n,(HL): no write-back
        } else {
            4
        };
        let mut c = cpu();
        c.regs.set_hl(0xC800);
        let mut b = bus(&[0xCB, op]);
        step(&mut c, &mut b);
        assert_eq!(b.log.len(), expected, "CB {op:#04x}");
        assert_eq!(c.regs.f() & 0x0F, 0, "CB {op:#04x} dirtied F low nibble");
    }
}
