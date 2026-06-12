//! Memory map, peripheral wiring, IF/IE, OAM DMA, CGB extras.
//! Interconnect work package.
//!
//! Implements [`crate::cpu::Bus`]. Each `read`/`write`/`tick` advances every
//! peripheral by one M-cycle (PPU: 4 dots, 2 in CGB double speed) and then
//! performs the access. Owns: WRAM (banked on CGB), HRAM, IF/IE, OAM DMA
//! engine (bus conflicts included), CGB regs (KEY1 speed switch, VBK, SVBK,
//! HDMA/GDMA, BCPS/BCPD/OCPS/OCPD routing, OPRI, FF72-FF77), and the
//! per-model post-boot hardware state.

use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::cpu::Bus;
use crate::joypad::Joypad;
use crate::model::Model;
use crate::ppu::{OamBugKind, Ppu};
use crate::serial::Serial;
use crate::timer::Timer;

/// The five implemented interrupt sources: IF/IE bits 0-4 (VBlank, STAT,
/// timer, serial, joypad). Bits 5-7 of FF0F/FFFF are unmapped (Pan Docs
/// "Interrupts").
const IF_MASK: u8 = 0x1F;
/// IF bit 1 (STAT), for the line-0 OAM-rise dispatch-late mask.
const IF_STAT_BIT: u8 = 0x02;

/// OAM DMA source classes (gambatte-core memptrs.h `OamDmaSrc`, classified
/// from the FF46 page by memory.cpp `oamDmaInitSetup`). The class decides
/// what the engine reads and which address pages a CPU access conflicts
/// with while a byte is in flight (gbctr "OAM DMA": the DMA controller
/// drives the bus it reads from).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DmaSrcKind {
    /// FF46 $00-$7F.
    Rom,
    /// FF46 $80-$9F.
    Vram,
    /// FF46 $A0-$BF.
    Sram,
    /// FF46 $C0-$DF, plus $E0-$FF on the DMG family (incomplete address
    /// decoding re-reads WRAM there, acceptance/oam_dma/sources-GS).
    Wram,
    /// FF46 $E0-$FF on CGB/AGB: the engine reads the idle bus value $FF
    /// (gambatte memory.cpp `oamDmaSrcPtr` → `rdisabledRam`).
    Invalid,
}

/// An OAM DMA transfer in progress: `idx` is the next byte to copy (one per
/// M-cycle).
#[derive(Clone, Copy)]
struct OamDmaRun {
    src: u16,
    idx: u8,
}

/// Conflict state of the current M-cycle: a DMA byte was copied, and CPU
/// accesses to conflicting pages observe or derail it (see
/// [`Interconnect::read_no_tick`] / [`Interconnect::write_no_tick`]).
#[derive(Clone, Copy)]
struct DmaConflict {
    kind: DmaSrcKind,
    /// FF46 of the running transfer; bit 4 selects the WRAM page of the
    /// CGB WRAM-region redirect.
    src_hi: u8,
    /// OAM index the byte was committed to this cycle.
    idx: u8,
    /// The byte the DMA engine drove onto the bus (= the byte committed).
    byte: u8,
}

/// A freshly written FF46 value waiting out its 1 M-cycle setup delay
/// (acceptance/oam_dma_start: the cycle after the write still reads OAM).
struct OamDmaStart {
    src: u16,
    delay: u8,
}

/// HBlank DMA arming, mirroring gambatte-core's `memevent_hdma` time
/// encoding (video.cpp `enableHdma`/`disableHdma`/`lcdcChange`):
/// `disabled_time` = off, `disabled_time - 1` = armed with the LCD off,
/// a real mode-0 time = armed with the LCD on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HdmaMode {
    Disabled,
    ArmedLcdOff,
    ArmedLcdOn,
}

/// A flagged VRAM-DMA request, serviced at the head of the CPU's next bus
/// operation (gambatte-core `flagHdmaReq`/`flagGdmaReq` set the
/// `intevent_dma` event; see [`Interconnect::service_vram_dma`] for the
/// exact seam the `_1`/`_2` ROM pairs pin).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VramDmaReq {
    /// One 16-byte HBlank block.
    Hblank,
    /// One 16-byte HBlank block re-flagged by a halt/stop wake: it skips
    /// the teardown M-cycle (gambatte Memory::event `intevent_dma`:
    /// `cc -= 4` when `haltHdmaState_ == hdma_requested`).
    HblankUnhalt,
    /// The whole remaining length at once.
    Gdma,
}

/// HBlank-DMA bookkeeping across a halt (HALT, deep STOP, or the
/// speed-switch pause) — gambatte-core memory.h `HdmaState`. While the
/// core clock is gated the LCD's mode-0 entries do not flag block requests
/// (video.h `EventTimes::flagHdmaReq` is suppressed while halted); this
/// records what the wake must re-evaluate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HaltHdmaState {
    /// Halt began outside the hblank window: a wake landing inside one
    /// fires a block.
    Low,
    /// Halt began *inside* the hblank window: the same hblank must not
    /// retrigger at wake.
    High,
    /// A flagged block request was deferred by the halt; the wake
    /// re-flags it ([`VramDmaReq::HblankUnhalt`]).
    Requested,
}

pub struct Interconnect {
    model: Model,
    cart: Cartridge,
    ppu: Ppu,
    apu: Apu,
    timer: Timer,
    serial: Serial,
    joypad: Joypad,
    /// Elapsed T-cycles since power-on (normal-speed dots).
    cycles: u64,

    /// CGB hardware running a CGB-flagged cart. CGB hardware with a DMG
    /// cart runs in DMG compatibility mode: KEY1/SVBK/HDMA/RP/FF74 and the
    /// palette data ports are disabled (misc/boot_hwio-C).
    cgb_mode: bool,
    double_speed: bool,
    /// KEY1 bit 0: speed switch armed for the next STOP.
    key1_armed: bool,

    /// 0x2000 bytes on DMG, 8 banks of 0x1000 on CGB.
    wram: Vec<u8>,
    /// SVBK as written (3 bits); bank 0 acts as bank 1.
    svbk: u8,
    hram: [u8; 0x7F],
    /// IF, low 5 bits (upper 3 read 1).
    intf: u8,
    /// IE, all 8 bits stored and readable.
    ie: u8,
    /// Timer IF bits committed in the *second half* of the current (most
    /// recent) M-cycle: the halt-exit sampling misses them until the next
    /// cycle, while IF reads and the running CPU's end-of-fetch sampling
    /// see them immediately ([`Bus::pending_halt_wake`]; `Timer::tick`'s
    /// `late`).
    if_late: u8,
    /// STAT IF bit raised by the PPU in the *current* M-cycle's second
    /// half (line-0 OAM rise): readable via FF0F at once, but the CPU's
    /// interrupt sample for this cycle must not see it
    /// (`Ppu::take_stat_late`).
    if_stat_late: u8,
    /// Dispatch-ack source sync-ahead (gambatte-core memory.cpp
    /// `Memory::ackIrq`): the IF clear of an interrupt dispatch happens
    /// slightly *into* the low-push M-cycle on hardware, so it also
    /// consumes a hardware re-set of the acked source that lands just
    /// after the ack — `updateSerial(cc + 3 + isCgb())`,
    /// `updateTimaIrq(cc + 2 + isCgb())`, `lcd_.update(cc + 2)` run
    /// before `intreq_.ackIrq(bit)`. Translated to this core's
    /// tick-then-access grid: a timer/serial set produced by the next
    /// machine tick (the next two on CGB/AGB — the timer IF commits on
    /// the last T-substep, the serial IF on the DIV-edge boundary), and
    /// a STAT/VBlank rise in the first 2 dots of the next tick, are
    /// swallowed by the preceding [`Bus::ack`]. The 2-dot LCD window is
    /// deliberately NOT widened to the line-anchored rises' second-half
    /// emission dots at single speed (gambatte's `cc + 2` does not reach
    /// them — m2int_m2irq_late_retrigger_1 and late_m0irq_retrigger_scx1_1
    /// pin the keeps); in double speed the 2-dot window spans the whole
    /// tick, which is what flips the `*_late_retrigger_ds_2` rows.
    /// Pinned by gambatte tima/tc00_irq_late_retrigger_2/3 (dmg08_outE4
    /// vs cgb04c_outE0), serial/start_wait_trigger_int8_read_if_2/3 and
    /// the irq_precedence/m1/ly0/lyc153int `*_late_retrigger` rows.
    /// `ack_squash_mask` is the acked source's IF bit (only that source
    /// is consumed — others get *flagged*, which our per-tick OR already
    /// models); `ack_squash_ticks`/`ack_squash_dots` are the remaining
    /// windows.
    ack_squash_mask: u8,
    ack_squash_ticks: u8,
    ack_squash_dots: u8,

    /// FF46 readback is simply the last written value
    /// (acceptance/oam_dma/reg_read).
    dma_reg: u8,
    dma_run: Option<OamDmaRun>,
    dma_start: Option<OamDmaStart>,
    /// CPU core clock gated off by HALT/STOP (see [`Self::set_cpu_halted`]).
    /// The OAM DMA controller shares that clock and freezes with it.
    cpu_halted: bool,
    /// Set while a DMA byte is copied during the current M-cycle.
    dma_conflict: Option<DmaConflict>,
    /// FEA0-FEFF on CPU CGB C ([`Model::Cgb`]): 24 bytes of extra OAM RAM,
    /// mirrored 4 times because low-address bits 3-4 don't decode (Pan
    /// Docs "FEA0-FEFF range" revisions 0-D; gambatte-core memory.cpp
    /// indexes `(addr - 0xFE00) & 0xE7`). AGB and the DMG family never
    /// touch this (see [`Self::prohibited_read`]).
    extra_oam: [u8; 24],

    // CGB VRAM DMA (FF51-FF55).
    /// Live source address counter as assembled from HDMA1/2 writes and
    /// advanced by the engine (gambatte memory.cpp `dmaSource_`).
    hdma_src: u16,
    /// Live destination counter, FULL 16 bits: only the VRAM write masks
    /// it to 0x1FFF, and the transfer terminates when the counter crosses
    /// 0x10000 (gambatte `dmaDestination_`; SameBoy `hdma_current_dest`).
    hdma_dst: u16,
    /// FF55 as the live register (gambatte `ioamhram_[0x155]`): bits 0-6 =
    /// remaining blocks - 1, bit 7 set = no HBlank transfer registered
    /// (completion, cancel, or halted abort). Reads back verbatim.
    hdma5: u8,
    /// HBlank DMA arming (see [`HdmaMode`]).
    hdma_mode: HdmaMode,
    /// Flagged block/GDMA request awaiting the next bus operation.
    vram_dma_req: Option<VramDmaReq>,
    /// HBlank-DMA state across a core-clock gate (see [`HaltHdmaState`]).
    halt_hdma: HaltHdmaState,
    /// Previous `hblank_active` level for the per-dot mode-0 edge detector.
    hdma_prev_hblank: bool,
    /// Re-entrancy guard: a VRAM DMA transfer is stalling the CPU and
    /// ticking the machine internally.
    vram_dma_stall: bool,
    /// Set for the stolen byte-copy M-cycles of a VRAM DMA service (not
    /// the teardown cycle): the VRAM DMA owns the bus, so the OAM DMA
    /// controller performs no source reads of its own — it advances by
    /// latching the VRAM DMA's bus traffic instead (gambatte memory.cpp
    /// `dma()` sets `lastOamDmaUpdate_ = disabled_time` around its copy
    /// loop and advances `oamDmaPos_` inline; see
    /// [`Self::oam_dma_bus_capture`]).
    vram_dma_owns_bus: bool,

    // CGB misc registers.
    /// FF56 RP bits 0/6/7 as written. No IR peer is modelled: bit 1
    /// ("received signal") always reads 1 (= not receiving).
    rp: u8,
    /// FF72/FF73: fully readable/writable scratch (exist on CGB in both
    /// modes, boot_hwio-C).
    ff72: u8,
    ff73: u8,
    /// FF74: scratch, CGB mode only (reads $FF in DMG mode).
    ff74: u8,
    /// FF75: bits 4-6 writable, others read 1.
    ff75: u8,
}

/// DMG-compat palettes installed by the CGB boot ROM for DMG carts. The
/// boot ROM consults its title-checksum lookup table only when the licensee
/// is Nintendo (old licensee byte $14B == $01, or $33 with "01" at
/// $144-$145); every other cart gets this *default* combination — entries
/// OBJ0=4, OBJ1=4, BG=29 of SameBoy's BootROMs/cgb_boot.asm palette tables
/// (Pan Docs "Compatibility palettes"). The per-game hash table is
/// deliberately not modelled; if it ever is, gate it on the licensee check
/// first or non-Nintendo homebrew will mis-color.
const CGB_COMPAT_BG_PALETTE: [u16; 4] = [0x7FFF, 0x1BEF, 0x6180, 0x0000];
const CGB_COMPAT_OBJ_PALETTE: [u16; 4] = [0x7FFF, 0x421F, 0x1CF2, 0x0000];

impl Interconnect {
    pub fn new(model: Model, cart: Cartridge) -> Self {
        // CGB mode iff the hardware is a CGB/AGB *and* the cart opts in via
        // header byte 0x143 bit 7 (same predicate `GameBoy::auto_model`
        // uses: `cartridge::cgb_flag`).
        let cgb_mode = model.is_cgb() && cart.supports_cgb();
        // SGB packet/multiplayer port: SGB-family hardware with a cart
        // whose header unlocks SGB functions (Pan Docs "SGB flag").
        let sgb_joypad = matches!(model, Model::Sgb | Model::Sgb2) && cart.supports_sgb();
        let mut ppu = Ppu::new(model);
        ppu.set_dmg_compat(model.is_cgb() && !cgb_mode);
        Self {
            model,
            cart,
            ppu,
            apu: Apu::new(model.is_cgb()),
            timer: Timer::new(),
            // The serial fast-clock bit (SC bit 1) exists in CGB mode only;
            // in DMG compatibility mode SC reads $7E (misc/boot_hwio-C).
            serial: Serial::new(cgb_mode),
            joypad: Joypad::new(sgb_joypad),
            cycles: 0,
            cgb_mode,
            double_speed: false,
            key1_armed: false,
            wram: vec![0; if model.is_cgb() { 0x8000 } else { 0x2000 }],
            svbk: 0,
            hram: [0; 0x7F],
            intf: 0,
            ie: 0,
            if_late: 0,
            if_stat_late: 0,
            ack_squash_mask: 0,
            ack_squash_ticks: 0,
            ack_squash_dots: 0,
            dma_reg: 0,
            dma_run: None,
            dma_start: None,
            cpu_halted: false,
            dma_conflict: None,
            extra_oam: [0; 24],
            hdma_src: 0,
            hdma_dst: 0,
            hdma5: 0xFF,
            hdma_mode: HdmaMode::Disabled,
            vram_dma_req: None,
            halt_hdma: HaltHdmaState::Low,
            hdma_prev_hblank: false,
            vram_dma_stall: false,
            vram_dma_owns_bus: false,
            rp: 0,
            ff72: 0,
            ff73: 0,
            ff74: 0,
            ff75: 0,
        }
    }

    /// True when the machine runs in native CGB mode (CGB/AGB hardware with
    /// a CGB-flagged cart, as opposed to DMG compatibility mode).
    pub(crate) fn cgb_mode(&self) -> bool {
        self.cgb_mode
    }

    /// Initialise hardware registers and DIV to the post-boot state of the
    /// model (called once from `GameBoy::new`).
    ///
    /// Special cases (everything else goes through the normal IO write
    /// paths):
    /// * LCD: the boot ROM turned the LCD on long before hand-off, so LCDC
    ///   is written first and the PPU is ticked through its glitched enable
    ///   line (70224-4 dots) plus `lcd_phase_dots` to reach the exact
    ///   mid-frame position `boot_hwio-*` measure. IF bits produced during
    ///   this warmup are discarded — the table's IF value ($E1) already
    ///   represents them.
    /// * FF46 is installed as a plain register value; an IO write would
    ///   start a transfer.
    /// * DIV is set directly (`Timer::set_div`); an FF04 write resets the
    ///   counter and can clock TIMA through the falling-edge detector.
    /// * CGB compat palettes are written through BCPS/BCPD before the mode
    ///   gate would block them (the boot ROM writes them while still in CGB
    ///   mode, then locks compatibility mode via KEY0).
    /// * Serial and APU get one seeding tick with the final DIV value so
    ///   their internal previous-DIV edge detectors start in phase
    ///   (boot_sclk_align-dmgABCmgb). A seeding tick from prev_div = 0
    ///   cannot produce a spurious falling edge.
    pub fn apply_post_boot_state(&mut self) {
        let s = self.model.post_boot_state();

        self.install_power_on_wram();
        self.install_boot_logo_vram();

        // The CGB/AGB boot ROM hands a CGB-flagged cart off 0x7D8
        // T-cycles (502 M-cycles) *earlier* than a DMG cart: the
        // DMG-compat path does its compatibility-palette work after the
        // logo. DIV and the LCD run uninterrupted through that tail, so
        // both shift together by exactly the DIV difference. The
        // CGB-cart DIV is pinned by gambatte div/start_inc_1/2 ($143=$C0
        // carts: FF04 reads $1E at +96 T right before an increment, $1F
        // at +100 → counter $1E9C = $2674 - $7D8) and cross-checked by
        // tima/tc00_start_1/2 ($1E9C + 356 ≡ 0 mod $400 puts the first
        // TIMA increment exactly between rounds); the DMG-cart values
        // stay pinned by mooneye misc/boot_div-cgbABCDE/-A (DMG carts).
        let cgb_cart_cut: u32 = if self.cgb_mode { 0x7D8 } else { 0 };

        // LCD warmup: glitched enable line (452 dots) + 153 normal lines
        // brings the PPU to line 0 dot 0; then advance to the hand-off
        // phase.
        self.ppu.write(0xFF40, 0x91);
        for _ in 0..(70224 - 4 + s.lcd_phase_dots - cgb_cart_cut) {
            self.ppu.tick();
        }

        if self.model.is_cgb() {
            // Compat palette: BG palette 0 (8 bytes) leaves BCPS = $88,
            // OBJ palettes 0+1 (16 bytes) leave OCPS = $90 — boot_hwio-C
            // reads $C8/$D0.
            self.ppu.write(0xFF68, 0x80);
            for c in CGB_COMPAT_BG_PALETTE {
                self.ppu.write(0xFF69, c as u8);
                self.ppu.write(0xFF69, (c >> 8) as u8);
            }
            self.ppu.write(0xFF6A, 0x80);
            for _ in 0..2 {
                for c in CGB_COMPAT_OBJ_PALETTE {
                    self.ppu.write(0xFF6B, c as u8);
                    self.ppu.write(0xFF6B, (c >> 8) as u8);
                }
            }
            // OPRI: DMG-compat mode uses DMG-style X priority (FF6C reads
            // $FF), CGB mode uses OAM-index priority ($FE).
            self.ppu.write(0xFF6C, u8::from(!self.cgb_mode));
        }

        for &(addr, value) in s.hwio {
            if addr == 0xFF46 {
                self.dma_reg = value;
            } else {
                self.write_no_tick(addr, value);
            }
        }

        // SGB boot duration depends on the cartridge header: the boot ROM
        // sends it to the SNES bit by bit, and the zero-bit branch of its
        // send loop is one M-cycle longer than the one-bit branch (see
        // `sgb_header_zero_bits` for the boot ROM derivation).
        let div = if matches!(self.model, Model::Sgb | Model::Sgb2) {
            s.div_counter
                .wrapping_add((4 * sgb_header_zero_bits(&self.cart)) as u16)
        } else {
            s.div_counter.wrapping_sub(cgb_cart_cut as u16)
        };
        self.timer.set_div(div);
        self.serial.tick(div);
        self.apu.tick(div, false);

        // APU warmup: the hwio replay above re-triggered the boot beep, but
        // on hardware the beep plays while the logo is shown, well before
        // hand-off, and its decaying envelope (NR12=$F3: 15 steps x 3
        // frame-sequencer envelope ticks = 45/64 s) has reached volume 0 by
        // PC=0x100 — channel 1 stays *enabled* (NR52 still reads $F1; volume
        // is not an enable condition), it just outputs digital 0, so the CGB
        // PCM12 register reads $00 at hand-off (oracle: misc/boot_hwio-C and
        // misc/bits/unused_hwio-C, which expect FF76 == $00). Run the APU
        // through one emulated second of synthetic DIV time to decay the
        // envelope; samples produced are discarded. The real DIV counter and
        // the serial/APU edge detectors are re-seeded afterwards.
        let mut warm_div = div;
        for _ in 0..1_048_576u32 {
            warm_div = warm_div.wrapping_add(4);
            self.apu.tick(warm_div, false);
        }
        self.apu.tick(div, false);
        let mut sink = Vec::new();
        self.apu.drain_samples(&mut sink);
    }

    /// WRAM as it powers up. Real DMG-family WRAM is not zeroed: it wakes
    /// in a deterministic stripe pattern — alternating $00/$FF half-pages
    /// (256 B) with the polarity inverted across each 2 KiB half, and
    /// C000-CFFF mirrored into D000-DFFF — plus a sprinkle of per-board
    /// bit noise. The pattern is the gambatte-core mem_dumps.h
    /// `setInitialDmgWram` hardware dump (captured on the same DMG-CPU-08
    /// board as the `dmg08` expectation corpus; every DMG emulator with a
    /// hardware dump agrees on the stripe base). The per-board
    /// `dmgWramDumpDiff` noise bytes are deliberately NOT vendored: no
    /// test in the corpus distinguishes them, and they are unit noise,
    /// not architecture. Pinned by gambatte `oamdma_srcFE00_*` (an OAM
    /// DMA from $FE00 reads the $DE00 WRAM echo page, which must read
    /// $FF). The boot ROM only clears VRAM/HRAM, never WRAM, so the
    /// pattern survives to PC=0x100; mooneye `boot_hwio-*` masks WRAM
    /// out. CGB WRAM keeps the zero fill: its power-on pattern differs
    /// per bank (gambatte `setInitialCgbWram`) and nothing in the corpus
    /// pins it.
    fn install_power_on_wram(&mut self) {
        if self.model.is_cgb() {
            return;
        }
        for (i, byte) in self.wram.iter_mut().enumerate() {
            // Half-page index 0..15 within the (mirrored) 4 KiB bank:
            // $00 for even half-pages in C000-C7FF and odd ones in
            // C800-CFFF, $FF otherwise.
            let half_page = (i >> 8) & 0x0F;
            let inverted = half_page >= 8;
            *byte = if (half_page & 1 == 0) != inverted {
                0x00
            } else {
                0xFF
            };
        }
    }

    /// VRAM tile data as the boot ROM leaves it: the Nintendo logo
    /// decompressed into tiles $01-$18 and the (R) trademark tile at $19
    /// — even (low-bitplane) bytes only, the boot routine writes one
    /// bitplane (DMG boot ROM "Graphic routine"; gambatte initstate.cpp
    /// setInitialVram models the same bytes). mealybug m3_scx_low_3_bits
    /// renders the leftover (R) tile straight out of this data.
    ///
    /// The boot ROM decompresses the logo from the cartridge header, but
    /// it also locks up unless the header bytes equal the canonical logo
    /// — so on hardware VRAM only ever holds the standard image, and the
    /// fixed constant below covers every cart (including the gambatte
    /// test ROMs, whose headers carry no logo at all; gambatte's
    /// initstate uses the same fixed dump).
    ///
    /// Deliberately NOT modelled: the DMG boot also leaves the logo's two
    /// tile-map rows at $9904/$9924 (+ the (R) entry at $9910). The
    /// pinned gambatte reference PNGs are emulator captures from before
    /// gambatte modelled initial VRAM — they encode a cleared map, and
    /// several otherwise-passing screens (scx_during_m3/old,
    /// bgtilemap/bgtiledata) show the logo rows if the entries are
    /// installed, while no test in the corpus needs them.
    fn install_boot_logo_vram(&mut self) {
        const NINTENDO_LOGO: [u8; 48] = [
            0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C,
            0x00, 0x0D, 0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6,
            0xDD, 0xDD, 0xD9, 0x99, 0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC,
            0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
        ];
        // Each logo nibble is bit-doubled into one tile-row byte, written
        // twice (two consecutive tile rows).
        let double = |n: u8| -> u8 {
            let mut out = 0u8;
            for bit in 0..4 {
                if n & (1 << bit) != 0 {
                    out |= 0b11 << (bit * 2);
                }
            }
            out
        };
        for (i, byte) in NINTENDO_LOGO.into_iter().enumerate() {
            let base = 0x8010 + 8 * i as u16;
            for (j, nibble) in [byte >> 4, byte & 0x0F].into_iter().enumerate() {
                let row = double(nibble);
                self.ppu.vram_write_raw(base + 4 * j as u16, row);
                self.ppu.vram_write_raw(base + 4 * j as u16 + 2, row);
            }
        }
        // The (R) trademark tile data lives in the boot ROM itself.
        const TRADEMARK: [u8; 8] = [0x3C, 0x42, 0xB9, 0xA5, 0xB9, 0xA5, 0x42, 0x3C];
        for (i, b) in TRADEMARK.into_iter().enumerate() {
            self.ppu.vram_write_raw(0x8190 + 2 * i as u16, b);
        }
    }

    pub fn model(&self) -> Model {
        self.model
    }

    pub fn cycles(&self) -> u64 {
        self.cycles
    }

    pub fn frame_count(&self) -> u64 {
        self.ppu.frame_count()
    }

    pub fn ppu(&self) -> &Ppu {
        &self.ppu
    }

    pub fn ppu_mut(&mut self) -> &mut Ppu {
        &mut self.ppu
    }

    pub fn apu_mut(&mut self) -> &mut Apu {
        &mut self.apu
    }

    pub fn joypad_mut(&mut self) -> &mut Joypad {
        &mut self.joypad
    }

    pub fn cartridge(&self) -> &Cartridge {
        &self.cart
    }

    pub fn cartridge_mut(&mut self) -> &mut Cartridge {
        &mut self.cart
    }

    /// Drain captured serial output (test-harness hook; see
    /// `Serial::take_output`).
    pub(crate) fn take_serial_output(&mut self) -> Vec<u8> {
        self.serial.take_output()
    }

    /// Side-effect-free, time-free view of memory for test harnesses:
    /// `&self` guarantees no peripheral ticks and no read side effects.
    ///
    /// Deliberately omniscient — unlike a CPU read it ignores PPU
    /// mode-based VRAM/OAM lockout and OAM DMA bus conflicts.
    /// ROM/VRAM/cart-RAM/WRAM follow the live banking; disabled cart RAM
    /// still reads $FF like a real access (`Cartridge::read_ram`). IO
    /// registers (FF00-FF7F) are *not* peekable — their values are
    /// computed from live peripheral state under the tick-then-access
    /// contract, and reading them out of band would mislead harnesses —
    /// and the FEA0-FEFF prohibited area has no stable content; both read
    /// $FF here.
    pub(crate) fn peek(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cart.read_rom(addr),
            0x8000..=0x9FFF => self.ppu.vram_read_raw(addr),
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xFDFF => self.wram[self.wram_index(addr)],
            0xFE00..=0xFE9F => self.ppu.oam_read_raw(addr),
            0xFEA0..=0xFF7F => 0xFF,
            0xFF80..=0xFFFE => self.hram[usize::from(addr - 0xFF80)],
            0xFFFF => self.ie,
        }
    }

    /// Advance the whole machine by one CPU M-cycle (docs/ARCHITECTURE.md
    /// §Timing: timer, OAM DMA engine, PPU dots, VRAM DMA, APU, serial,
    /// joypad; IF bits OR-ed in as produced).
    fn tick_machine(&mut self) {
        let dots: u64 = if self.double_speed { 2 } else { 4 };
        self.cycles += dots;
        // Dispatch-ack sync-ahead window for this tick (see `ack`):
        // timer/serial sets produced by an in-window tick are consumed
        // by the preceding ack instead of re-raising IF.
        let tick_squash = if self.ack_squash_ticks > 0 {
            self.ack_squash_ticks -= 1;
            self.ack_squash_mask & 0x0C
        } else {
            0
        };
        let t = self.timer.tick();
        // IF reads must see a second-half commit within its own cycle
        // (mooneye tima_reload access sequences) — only the halt-exit
        // sampling misses it, via the `if_late` mask.
        let t_iff = t.iff & IF_MASK & !tick_squash;
        self.intf |= t_iff;
        self.if_late = if t.late { t_iff } else { 0 };
        self.oam_dma_tick();
        self.if_stat_late = 0;
        for i in 0..dots {
            // STAT/VBlank rises in the first 2 dots after the ack are
            // consumed too (gambatte ackIrq lcd_.update(cc + 2); in
            // double speed the window spans the whole tick — see `ack`).
            let dot_squash = if self.ack_squash_dots > 0 {
                self.ack_squash_dots -= 1;
                self.ack_squash_mask & 0x03
            } else {
                0
            };
            self.intf |= self.ppu.tick() & IF_MASK & !dot_squash;
            if self.ppu.take_stat_late() {
                // The line-0 OAM STAT rise sits in the second half of the
                // M-cycle: the IF bit is readable at once, but this
                // cycle's interrupt sample must not see it (see
                // Ppu::stat_events_tick; mealybug "line 0 timing is different
                // by 4 cycles").
                self.if_stat_late |= IF_STAT_BIT;
            }
            if self.ppu.take_stat_halt_late() {
                // Second-half STAT IF commit (line-start OAM pulses):
                // readable at once, but the halt-exit sampler misses it
                // for one cycle — the same shape as the timer's `if_late`
                // mask (SameBoy GB_cpu_run halt path; gbmicrotest
                // int_oam_* grids pin the law).
                self.if_late |= IF_STAT_BIT;
            }
            if self.ppu.take_m0_rise() && 2 * (i + 1) > dots {
                // The mode-0 STAT rise carries the second-half halt law
                // — the same shape as the line-start OAM pulses — but
                // its dot moves with SCX/sprites/window, so the half is
                // decided here against the CPU's M-cycle: a rise in the
                // second half (PPU dots 3-4 within the cycle; the last
                // dot in double speed) is readable at once and fully
                // visible to the running CPU's interrupt sample, yet
                // missed by the halt-exit sampler for one M-cycle
                // (SameBoy GB_cpu_run samples the halt exit mid-cycle).
                // mooneye hblank_ly_scx_timing-GS and the gbmicrotest
                // int_hblank_halt_scx0-7 grid pin all eight SCX phases
                // between them.
                self.if_late |= IF_STAT_BIT;
            }
            // Dot-exact mode-0 entry: each visible line's hblank start
            // requests one HBlank DMA block, serviced at the head of the
            // CPU's next bus operation (gambatte video.cpp: memevent_hdma
            // fires at predictedNextM0Time). The flag is suppressed while
            // the core clock is gated (video.h EventTimes::flagHdmaReq:
            // `if (!intreq_.halted())`); the level detector keeps
            // tracking so a wake never sees a stale edge.
            let hb = self.ppu.hdma_trigger_level();
            if hb
                && !self.hdma_prev_hblank
                && self.hdma_mode == HdmaMode::ArmedLcdOn
                && !self.cpu_halted
            {
                self.vram_dma_req = Some(VramDmaReq::Hblank);
            }
            self.hdma_prev_hblank = hb;
        }
        let div = self.timer.div_counter();
        self.apu.tick(div, self.double_speed);
        self.intf |= self.serial.tick(div) & IF_MASK & !tick_squash;
        self.intf |= self.joypad.take_irq() & IF_MASK;
        // RTC wall time is dot time (2 dots per M-cycle in double speed).
        self.cart.tick_rtc(dots as u32);
    }

    // ---- OAM DMA engine ------------------------------------------------

    /// Gate (true) or ungate (false) the OAM DMA controller's clock.
    ///
    /// The OAM DMA controller is clocked by the CPU core clock, which HALT
    /// (and STOP) switches off while the PPU keeps running on its own clock.
    /// A transfer in progress therefore does not proceed while the CPU is
    /// halted: bytes already copied stay, the byte in flight never commits,
    /// and the rest of OAM keeps its old contents — the PPU renders from
    /// that mixture for as long as the CPU sleeps. Hardware-verified by
    /// mooneye madness/mgb_oam_dma_halt_sprites.s ("OAM DMA is in the middle
    /// of OAM access (but not proceeding with it!)"); its observed sprite
    /// data pins the freeze mid-byte, with the overwritten OAM byte intact.
    ///
    /// Called by the CPU wiring on halt/stop entry and exit (via
    /// [`Bus::set_halted`]); the halted CPU performs no bus accesses on
    /// hardware, so the CPU-visible bus state during the freeze is
    /// unobservable and no bus conflict is modelled.
    ///
    /// While a transfer sits frozen mid-byte, the PPU is handed the frozen
    /// access (OAM index about to be replaced + in-flight source byte): the
    /// DMA controller is "in the middle of OAM access (but not proceeding
    /// with it!)" and the MGB PPU's OAM scan sees glitched data derived
    /// from exactly these bytes (madness/mgb_oam_dma_halt_sprites.s; see
    /// `Ppu::set_oam_dma_freeze`). A freeze during the setup delay has no
    /// OAM access in flight and hands over nothing.
    pub fn set_cpu_halted(&mut self, halted: bool) {
        if self.cpu_halted == halted {
            return;
        }
        if halted {
            // gambatte Memory::halt: a flagged-but-unserviced block
            // request is deferred (hdma_requested) and re-flagged at
            // wake — HBlank DMA never proceeds while the core clock is
            // gated; otherwise remember whether the hblank window was
            // already active so the same hblank cannot retrigger at wake.
            self.halt_hdma = if self.vram_dma_req.take().is_some() {
                HaltHdmaState::Requested
            } else if self.hdma_mode == HdmaMode::ArmedLcdOn && self.ppu.hdma_period() {
                HaltHdmaState::High
            } else {
                HaltHdmaState::Low
            };
        }
        self.engage_halt_gate(halted);
        if !halted {
            // The halt-mode wake restarts the OAM DMA controller's clock
            // one M-cycle ahead of the CPU pipeline: a single catch-up
            // cycle runs at the wake itself, before the CPU's first
            // post-wake M-cycle (SameBoy sm83_cpu.c `GB_cpu_run` halt
            // exit: `gb->dma_cycles = 4; GB_dma_run(gb)` on both the
            // IME=0 resume and the dispatch path, while `GB_dma_run`
            // returns early whenever `gb->halted`; hardware-pinned by
            // gambatte oamdma/oamdmasrc80_halt_*_read8000 out81 and
            // dma/hdma_transition_oamdma_2 out67, which read the
            // in-flight source index after a wake). The speed-switch
            // pause's gate release deliberately does NOT take this path:
            // oamdma/oamdmasrcC0_speedchange_readC000 out11 pins the
            // un-caught-up resume there (SameBoy's
            // speed_switch_halt_countdown expiry likewise skips the
            // catch-up). The conflict state left behind is unobservable —
            // every CPU bus access ticks the machine, refreshing it,
            // before the access.
            self.oam_dma_tick();
            self.vram_dma_unhalt();
        }
    }

    /// The raw core-clock gate: freezes the OAM DMA controller and hands
    /// the frozen access to the PPU (see [`Self::set_cpu_halted`] for the
    /// HBlank-DMA bookkeeping layered on top; `Interconnect::stop` drives
    /// this directly because the speed-switch pause sequences the HDMA
    /// state itself).
    fn engage_halt_gate(&mut self, halted: bool) {
        self.cpu_halted = halted;
        let freeze = if halted {
            self.dma_run
                .as_ref()
                .map(|run| (run.idx, self.oam_dma_source_read(run.src, run.idx)))
        } else {
            None
        };
        self.ppu.set_oam_dma_freeze(freeze);
    }

    fn oam_dma_tick(&mut self) {
        self.dma_conflict = None;
        // HALT/STOP gate the CPU core clock that drives this controller:
        // neither the setup delay nor the transfer advances while the CPU
        // is halted (see set_cpu_halted; madness/mgb_oam_dma_halt_sprites).
        // While a VRAM DMA owns the bus the controller cannot read its
        // source either: it advances via [`Self::oam_dma_bus_capture`]
        // from the steal loop instead of here.
        if self.cpu_halted || self.vram_dma_owns_bus {
            return;
        }
        self.oam_dma_promote_start();
        if let Some(run) = self.dma_run {
            let byte = self.oam_dma_source_read(run.src, run.idx);
            self.ppu.oam_dma_write(run.idx, byte);
            self.dma_conflict = Some(DmaConflict {
                kind: self.dma_src_kind(run.src),
                src_hi: (run.src >> 8) as u8,
                idx: run.idx,
                byte,
            });
            self.dma_run = (run.idx < 159).then_some(OamDmaRun {
                idx: run.idx + 1,
                ..run
            });
        }
    }

    /// Promote a pending start whose setup delay has elapsed. The old
    /// transfer (if any) keeps copying during the delay cycle
    /// (acceptance/oam_dma_restart) and is replaced exactly when the new
    /// one copies its first byte. Shared by the controller's own clock
    /// ([`Self::oam_dma_tick`]) and the VRAM-DMA steal advances
    /// ([`Self::oam_dma_bus_capture`]): gambatte's `oamDmaPos_` counts
    /// toward `oamDmaStartPos_` identically on both paths.
    fn oam_dma_promote_start(&mut self) {
        match &mut self.dma_start {
            Some(s) if s.delay == 0 => {
                let src = s.src;
                self.dma_start = None;
                self.dma_run = Some(OamDmaRun { src, idx: 0 });
            }
            Some(s) => s.delay -= 1,
            None => {}
        }
    }

    /// One OAM DMA controller advance during a VRAM-DMA bus steal: the
    /// controller cannot perform its own source read — the VRAM DMA owns
    /// the bus — so it latches the stolen transfer's bus traffic instead,
    /// writing `data` to the OAM cell addressed by the *bus address* low
    /// byte (`src & 0xFF`, NOT the controller's own position), or to the
    /// CGB-C extra OAM RAM with the usual bits-3/4 alias for low bytes ≥
    /// $A0. The position still advances, so the skipped source bytes are
    /// never copied (gambatte-core memory.cpp `dma()`:
    /// `ioamhram_[src & 0xFF] = data` / `ioamhram_[p & 0xE7] = data`
    /// per `oamDmaPos_` advance, gated on `!halted()`; hardware-pinned by
    /// gambatte dma/hdma_transition_oamdma_1/_2 and
    /// oamdma/oamdmasrcC000_hdmasrc0000).
    ///
    /// Called once per stolen M-cycle with that cycle's *last* byte: at
    /// normal speed gambatte's 4-cc advance grid (`cc - 3 >
    /// lOamDmaUpdate`, both M-cycle-aligned) lands every advance on the
    /// second of the two bytes copied per M-cycle; in double speed each
    /// M-cycle copies one byte and every byte advances.
    fn oam_dma_bus_capture(&mut self, src: u16, data: u8) {
        // The speed-switch pause can service a block while the core
        // clock is gated: the OAM DMA controller is frozen with the CPU
        // and does not advance (gambatte dma(): `&& !halted()`).
        if self.cpu_halted {
            return;
        }
        self.oam_dma_promote_start();
        if let Some(run) = self.dma_run {
            let p = src & 0xFF;
            if p < 0xA0 {
                self.ppu.oam_dma_write(p as u8, data);
            } else if self.model == Model::Cgb {
                // AGB skips the extra-RAM write (gambatte `!agbFlag_`).
                self.extra_oam[Self::extra_oam_index(src)] = data;
            }
            self.dma_run = (run.idx < 159).then_some(OamDmaRun {
                idx: run.idx + 1,
                ..run
            });
        }
    }

    /// Source class of a transfer from `src` (gambatte-core memory.cpp
    /// `oamDmaInitSetup`; see [`DmaSrcKind`]).
    fn dma_src_kind(&self, src: u16) -> DmaSrcKind {
        match src >> 8 {
            0x00..=0x7F => DmaSrcKind::Rom,
            0x80..=0x9F => DmaSrcKind::Vram,
            0xA0..=0xBF => DmaSrcKind::Sram,
            0xC0..=0xDF => DmaSrcKind::Wram,
            _ if self.model.is_cgb() => DmaSrcKind::Invalid,
            _ => DmaSrcKind::Wram,
        }
    }

    /// What the OAM DMA engine reads for byte `idx` of a transfer from
    /// `src`. Mode-based PPU blocking does not apply; ROM/SRAM/VRAM
    /// banking is live (gambatte memory.cpp `oamDmaSrcPtr`).
    fn oam_dma_source_read(&self, src: u16, idx: u8) -> u8 {
        let addr = src + u16::from(idx);
        match self.dma_src_kind(src) {
            DmaSrcKind::Rom => self.cart.read_rom(addr),
            DmaSrcKind::Vram => self.ppu.vram_read_raw(addr),
            DmaSrcKind::Sram => self.cart.read_ram(addr),
            // DMG $E0-$FF: incomplete address decoding re-reads WRAM
            // (acceptance/oam_dma/sources-GS: $FE/$FF read $DE00/$DF00).
            DmaSrcKind::Wram => {
                let addr = if addr >= 0xE000 { addr - 0x2000 } else { addr };
                self.wram[self.wram_index(addr)]
            }
            DmaSrcKind::Invalid => 0xFF,
        }
    }

    /// Whether a CPU access to `addr` collides with the running transfer's
    /// bus. One 16-bit mask per source class, bit n = 4 KiB page n
    /// conflicts (transcribed from gambatte-core memptrs.cpp
    /// `OamDmaConflictMap`; the FE/FF page never conflicts):
    ///
    /// * ROM/SRAM sources drive the external bus; on CGB the WRAM pages
    ///   C-F conflict *too*, with accesses redirected to WRAM (see
    ///   [`Self::dma_redirect_wram_index`]).
    /// * VRAM sources collide only with the VRAM pages 8-9.
    /// * WRAM sources collide with everything but VRAM on DMG (WRAM sits
    ///   on the external bus there) but only with pages C-F on CGB (its
    ///   own bus).
    fn in_dma_conflict_area(&self, kind: DmaSrcKind, addr: u16) -> bool {
        let pages: u16 = match kind {
            DmaSrcKind::Rom | DmaSrcKind::Sram | DmaSrcKind::Invalid => 0xFCFF,
            DmaSrcKind::Vram => 0x0300,
            DmaSrcKind::Wram if self.model.is_cgb() => 0xF000,
            DmaSrcKind::Wram => 0xFCFF,
        };
        addr < 0xFE00 && pages >> (addr >> 12) & 1 != 0
    }

    /// CGB conflict redirect for WRAM-region accesses during a non-WRAM
    /// transfer: the cell actually accessed is WRAM page 0 or the banked
    /// page — chosen by FF46 bit 4, not by the address — at offset
    /// `addr & 0xFFF` (gambatte memory.cpp:
    /// `cart_.wramdata(ioamhram_[0x146] >> 4 & 1)[p & 0xFFF]`; pinned by
    /// oamdma_srcXXXX_busypopDFFF/busypushC001+ cgb04c rows).
    fn dma_redirect_wram_index(&self, c: &DmaConflict, addr: u16) -> usize {
        if c.src_hi & 0x10 != 0 {
            self.wram_index(0xD000 | (addr & 0x0FFF))
        } else {
            usize::from(addr & 0x0FFF)
        }
    }

    // ---- CGB VRAM DMA ---------------------------------------------------

    /// Service any flagged VRAM-DMA request at the *head* of a CPU bus
    /// operation, before that operation's machine tick: the bus steal sits
    /// between the M-cycle whose dots flagged the request and the CPU's
    /// next M-cycle. M-cycle (not instruction) granularity is what the
    /// gambatte hdma_late_destl/_length/_wrambank and hdma_start `_1`/`_2`
    /// adjacent-cycle pairs resolve, and they also split by access type:
    /// a *write* whose own cycle contains the trigger commits before the
    /// steal (hdma_late_destl_1: the racing FF54 write wins), while a
    /// *read* in the trigger cycle yields — the DMA owns the bus before
    /// the read samples (hdma_start_2: the fetch's data byte is the
    /// post-block value), hence the second `service_vram_dma` call after
    /// the tick in `Bus::read`/`read_inc`. A request flagged during the
    /// STOP opcode fetch is deliberately still pending when `Bus::stop`
    /// runs (no bus operation in between) — the
    /// hdma_transition_speedchange matrix needs exactly that request.
    fn service_vram_dma(&mut self) {
        while self.vram_dma_req.is_some() && !self.vram_dma_stall {
            self.run_vram_dma();
        }
    }

    /// Service one flagged VRAM-DMA request, stalling the CPU while the
    /// rest of the machine keeps running. Pace: 2 bytes per stolen M-cycle
    /// at normal speed, 1 in double speed (gambatte memory.cpp `dma()`:
    /// `cc += 2 + 2 * doubleSpeed` per byte), plus one teardown M-cycle
    /// per service (`cc += 4`) — skipped when the block was deferred by a
    /// halt wake (`Memory::event` intevent_dma: `cc -= 4` for an
    /// unhalt-requested block).
    fn run_vram_dma(&mut self) {
        let Some(req) = self.vram_dma_req.take() else {
            return;
        };
        self.vram_dma_stall = true;
        let mut remaining = (usize::from(self.hdma5 & 0x7F) + 1) * 0x10;
        let mut length = if req == VramDmaReq::Gdma {
            remaining
        } else {
            0x10
        };
        // The full 16-bit destination counter terminates the transfer at
        // the 0x10000 crossing, latching FF55 bit 7 (gambatte dma():
        // `if (dmaDest + length >= 0x10000) { length = 0x10000 - dmaDest;
        // ioamhram_[0x155] |= 0x80; }`; hardware capture dma/dma_dst_wrap_2
        // — the transfer does NOT wrap back into VRAM).
        let to_wrap = 0x1_0000 - usize::from(self.hdma_dst);
        if length >= to_wrap {
            length = to_wrap;
            self.hdma5 |= 0x80;
        }
        remaining -= length;
        // A GDMA with the display off always retires its whole length
        // (gambatte dma(): `if (!(lcdc & en) && gdmaReqFlagged) dmaLength
        // = 0`), so a 0x10000-truncated copy still reads back $FF.
        if req == VramDmaReq::Gdma && !self.ppu.lcd_enabled() {
            remaining = 0;
        }
        let per_m = if self.double_speed { 1 } else { 2 };
        while length > 0 {
            // The byte-copy M-cycles own the bus: the OAM DMA controller's
            // own clocking is suppressed (the teardown cycle below is a
            // plain machine cycle again — gambatte restores
            // lastOamDmaUpdate_ before its `cc += 4`).
            self.vram_dma_owns_bus = true;
            self.tick_machine();
            self.vram_dma_owns_bus = false;
            for i in 0..per_m.min(length) {
                let src = self.hdma_src;
                let byte = self.vram_dma_source_read(src);
                self.ppu
                    .vram_write_raw(0x8000 | (self.hdma_dst & 0x1FFF), byte);
                self.hdma_src = src.wrapping_add(1);
                self.hdma_dst = self.hdma_dst.wrapping_add(1);
                length -= 1;
                if i + 1 == per_m {
                    // A concurrent OAM DMA advances once per stolen
                    // M-cycle, latching this cycle's last bus byte.
                    self.oam_dma_bus_capture(src, byte);
                }
            }
        }
        if req != VramDmaReq::HblankUnhalt {
            self.tick_machine(); // teardown (gambatte dma(): cc += 4)
        }
        if self.cpu_halted {
            // A block serviced while the core clock is gated — only the
            // speed-switch pause can do this (see `Interconnect::stop`) —
            // aborts the HBlank transfer: FF55 keeps its length bits and
            // gains bit 7 (gambatte dma(): `ioamhram_[0x155] = halted() ?
            // ioamhram_[0x155] | 0x80 : …`; pinned by
            // hdma_transition_speedchange_hdmalen*_hdma5).
            self.hdma5 |= 0x80;
        } else {
            self.hdma5 = ((remaining / 0x10) as u8).wrapping_sub(1) | (self.hdma5 & 0x80);
        }
        if self.hdma5 & 0x80 != 0 {
            // Completion/termination/abort disarms the HBlank engine
            // (gambatte dma(): `if ((FF55 & 0x80) && hdmaIsEnabled())
            // lcd_.disableHdma(cc)`).
            self.hdma_mode = HdmaMode::Disabled;
        }
        self.vram_dma_stall = false;
    }

    /// Wake-side HBlank-DMA re-evaluation, shared by halt/stop wake and
    /// the end of the speed-switch pause (gambatte Memory::event,
    /// `intevent_unhalt` and the halted `intevent_interrupts` path): a
    /// deferred request re-flags (without the teardown cycle), and an
    /// armed transfer whose halt began outside the hblank window fires if
    /// the wake lands inside one.
    fn vram_dma_unhalt(&mut self) {
        match self.halt_hdma {
            HaltHdmaState::Requested => self.vram_dma_req = Some(VramDmaReq::HblankUnhalt),
            HaltHdmaState::Low
                if self.hdma_mode == HdmaMode::ArmedLcdOn && self.ppu.hdma_period() =>
            {
                self.vram_dma_req = Some(VramDmaReq::Hblank);
            }
            _ => {}
        }
        self.halt_hdma = HaltHdmaState::Low;
    }

    /// VRAM DMA source read. VRAM itself and the 0xE000+ region are not
    /// valid sources (Pan Docs); they read as 0xFF here (SameBoy
    /// Core/memory.c GB_hdma_run drives the bus only for ROM/SRAM/WRAM
    /// sources and leaves the idle data-bus byte for everything else —
    /// `gdma_invalid_sources_fill_destination_with_ff`; gambatte reads its
    /// decaying cart-bus latch, unmodelled here).
    fn vram_dma_source_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cart.read_rom(addr),
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xDFFF => self.wram[self.wram_index(addr)],
            _ => 0xFF,
        }
    }

    /// FF55 write (gambatte memory.cpp nontrivial_ff_write case 0x55; the
    /// `hdma_mode` branches mirror `lcd_.hdmaIsEnabled()`).
    fn hdma5_write(&mut self, value: u8) {
        // The write always replaces the live length bits — cancelling
        // latches the *written* length with bit 7 set, not the old
        // remaining count (gambatte: `ioamhram_[0x155] = data & 0x7F`
        // before the cancel branch; SameBoy memory.c GB_IO_HDMA5 likewise
        // sets hdma_steps_left first).
        self.hdma5 = value & 0x7F;
        if self.hdma_mode != HdmaMode::Disabled {
            if value & 0x80 == 0 {
                self.hdma5 |= 0x80;
                self.hdma_mode = HdmaMode::Disabled;
            }
            // Bit 7 set while armed: only the length bits change.
        } else if value & 0x80 != 0 {
            // gambatte video.cpp enableHdma: with the LCD off one block
            // copies immediately (flagHdmaReq, SameBoy: `(STAT & 3) == 0
            // && display_state != 7 → hdma_on = true`); with the LCD on a
            // block fires at once only inside the hblank window.
            if self.ppu.lcd_enabled() {
                self.hdma_mode = HdmaMode::ArmedLcdOn;
                if self.ppu.hdma_period() {
                    self.vram_dma_req = Some(VramDmaReq::Hblank);
                }
            } else {
                self.hdma_mode = HdmaMode::ArmedLcdOff;
                self.vram_dma_req = Some(VramDmaReq::Hblank);
            }
        } else {
            // General-purpose DMA: requested now, serviced at the next
            // instruction boundary (gambatte flagGdmaReq). Note a stale
            // active-looking FF55 (HBlank arming killed by an LCD
            // disable) reaches this branch too, exactly like upstream.
            self.vram_dma_req = Some(VramDmaReq::Gdma);
        }
    }

    // ---- memory routing -------------------------------------------------

    fn wram_index(&self, addr: u16) -> usize {
        let offset = usize::from(addr & 0x1FFF);
        if offset < 0x1000 {
            offset
        } else {
            let bank = if self.model.is_cgb() {
                usize::from(self.svbk & 7).max(1)
            } else {
                1
            };
            bank * 0x1000 + (offset - 0x1000)
        }
    }

    /// DMG OAM corruption bug (Pan Docs "OAM Corruption Bug"): a CPU
    /// access cycle with a $FE00-$FEFF value on the address bus while the
    /// PPU's mode-2 scan is on a corruptible row mangles that row.
    /// `addr` is the bus value: the access address for reads/writes, the
    /// 16-bit inc/dec unit's register value for internal cycles
    /// ([`Bus::tick_addr`]). The whole page triggers, including the
    /// FEA0-FEFF prohibited area (SameBoy keys on `addr < 0xFF00`; blargg
    /// oam_bug/8-instr_effect pops from $FEF0). Suppressed
    ///
    /// * on CGB/AGB — the bug is DMG-family-only,
    /// * while the core clock is gated off — the halted CPU performs no
    ///   bus accesses on hardware, keeping the discarded halt prefetch
    ///   (see `cpu::execute::step`) phantom-free, and
    /// * while the OAM DMA engine owns OAM — the interplay is untested on
    ///   hardware (SameBoy carries the same Todo), so the conservative
    ///   gate wins; `dma_conflict` is `Some` exactly in the M-cycles a
    ///   DMA byte copies.
    fn maybe_oam_bug(&mut self, addr: u16, kind: OamBugKind) {
        if !(0xFE00..=0xFEFF).contains(&addr)
            || self.model.is_cgb()
            || self.cpu_halted
            || self.dma_conflict.is_some()
        {
            return;
        }
        self.ppu.oam_bug(kind);
    }

    /// Cell of [`Self::extra_oam`] a FEA0-FEFF access decodes to: bits 3-4
    /// of the low address byte are ignored (gambatte-core memory.cpp masks
    /// the OAM-relative offset with $E7), leaving 24 cells — offsets
    /// $A0-$A7/$C0-$C7/$E0-$E7 — each mirrored 4 times.
    fn extra_oam_index(addr: u16) -> usize {
        let masked = usize::from(addr & 0xE7);
        (masked >> 5) * 8 + (masked & 7) - 40
    }

    /// FEA0-FEFF "prohibited" reads (Pan Docs "FEA0-FEFF range").
    ///
    /// * DMG family: $00 while OAM is idle, $FF while the PPU has OAM
    ///   locked (the mode-2 corruption itself lives in
    ///   [`Self::maybe_oam_bug`]).
    /// * CPU CGB C ([`Model::Cgb`], ARCHITECTURE §CGB revision policy):
    ///   extra OAM RAM behind the same lockout as OAM proper
    ///   ([`Self::extra_oam`]; gambatte oamdma busypushFEA1/FF01 rows).
    /// * AGB (and CGB revision E): the high nibble of the low address byte
    ///   twice.
    fn prohibited_read(&self, addr: u16) -> u8 {
        match self.model {
            Model::Cgb => {
                if self.ppu.oam_read_blocked() {
                    0xFF
                } else {
                    self.extra_oam[Self::extra_oam_index(addr)]
                }
            }
            Model::Agb => {
                let lo = addr as u8;
                (lo & 0xF0) | (lo >> 4)
            }
            _ if self.ppu.mode_bits() >= 2 => 0xFF,
            _ => 0x00,
        }
    }

    /// Write counterpart of [`Self::prohibited_read`]: only the CGB-C
    /// extra OAM RAM is writable, under the same gating as OAM proper
    /// (dropped while the PPU scans or a DMA byte is in flight; the
    /// in-flight gate is gambatte memory.cpp's `oamDmaPos_ < oam_size`).
    fn prohibited_write(&mut self, addr: u16, value: u8) {
        if self.model == Model::Cgb && self.dma_conflict.is_none() && !self.ppu.oam_write_blocked()
        {
            self.extra_oam[Self::extra_oam_index(addr)] = value;
        }
    }

    fn read_no_tick(&mut self, addr: u16) -> u8 {
        if let Some(c) = self.dma_conflict {
            // OAM (and the prohibited area behind it) reads $FF while a DMA
            // byte is in flight (gbctr OAM DMA).
            if (0xFE00..=0xFEFF).contains(&addr) {
                return 0xFF;
            }
            // Reads on conflicting pages see the DMA engine's bus, except
            // the CGB WRAM-region redirect (gambatte-core memory.cpp
            // nontrivial_read's OAM-DMA conflict block).
            if self.in_dma_conflict_area(c.kind, addr) {
                if self.model.is_cgb() && c.kind != DmaSrcKind::Wram && addr >= 0xC000 {
                    return self.wram[self.dma_redirect_wram_index(&c, addr)];
                }
                if self.model.is_cgb() && c.kind == DmaSrcKind::Vram {
                    // CGB quirk: the conflicted read also clears the
                    // in-flight OAM byte (gambatte memory.cpp:
                    // `ioamhram_[oamDmaPos_] = 0` after a vram-source
                    // conflict read).
                    self.ppu.oam_dma_write(c.idx, 0);
                }
                return c.byte;
            }
        }
        match addr {
            0x0000..=0x7FFF => self.cart.read_rom(addr),
            0x8000..=0x9FFF => self.ppu.read(addr),
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xFDFF => self.wram[self.wram_index(addr)],
            0xFE00..=0xFE9F => self.ppu.read(addr),
            0xFEA0..=0xFEFF => self.prohibited_read(addr),
            0xFF00..=0xFF7F => self.io_read(addr),
            0xFF80..=0xFFFE => self.hram[usize::from(addr - 0xFF80)],
            0xFFFF => self.ie,
        }
    }

    fn write_no_tick(&mut self, addr: u16, value: u8) {
        // A CPU write on a conflicting page derails: the addressed memory
        // (including MBC registers) never sees it. On DMG the byte lands
        // in the in-flight OAM slot — wire-ANDed with the DMA byte for
        // WRAM sources, as-is otherwise; CGB additionally zeroes the data
        // for VRAM sources and redirects WRAM-region writes to WRAM
        // instead of OAM (gambatte-core memory.cpp nontrivial_write's
        // OAM-DMA conflict block; pinned by the gambatte
        // oamdma_srcXXXX_busypush/busypop matrix).
        if let Some(c) = self.dma_conflict {
            if self.in_dma_conflict_area(c.kind, addr) {
                if self.model.is_cgb() {
                    if addr < 0xC000 {
                        let byte = if c.kind == DmaSrcKind::Vram { 0 } else { value };
                        self.ppu.oam_dma_write(c.idx, byte);
                    } else if c.kind != DmaSrcKind::Wram {
                        let i = self.dma_redirect_wram_index(&c, addr);
                        self.wram[i] = value;
                    }
                    // WRAM source + WRAM region: the write is swallowed.
                } else {
                    let byte = if c.kind == DmaSrcKind::Wram {
                        c.byte & value
                    } else {
                        value
                    };
                    self.ppu.oam_dma_write(c.idx, byte);
                }
                return;
            }
        }
        match addr {
            0x0000..=0x7FFF => self.cart.write_rom(addr, value),
            0x8000..=0x9FFF => self.intf |= self.ppu.write(addr, value) & IF_MASK,
            0xA000..=0xBFFF => self.cart.write_ram(addr, value),
            0xC000..=0xFDFF => {
                let i = self.wram_index(addr);
                self.wram[i] = value;
            }
            0xFE00..=0xFE9F => {
                // CPU OAM writes are dropped while DMA owns OAM.
                if self.dma_conflict.is_none() {
                    self.intf |= self.ppu.write(addr, value) & IF_MASK;
                }
            }
            0xFEA0..=0xFEFF => self.prohibited_write(addr, value),
            0xFF00..=0xFF7F => self.io_write(addr, value),
            0xFF80..=0xFFFE => self.hram[usize::from(addr - 0xFF80)] = value,
            0xFFFF => self.ie = value,
        }
    }

    fn io_read(&self, addr: u16) -> u8 {
        match addr {
            0xFF00 => self.joypad.read(),
            0xFF01 | 0xFF02 => self.serial.read(addr),
            0xFF04..=0xFF07 => self.timer.read(addr),
            0xFF0F => 0xE0 | self.intf,
            0xFF10..=0xFF3F => self.apu.read(addr),
            0xFF46 => self.dma_reg,
            0xFF40..=0xFF45 | 0xFF47..=0xFF4B => self.ppu.read(addr),
            0xFF4D if self.cgb_mode => {
                0x7E | (u8::from(self.double_speed) << 7) | u8::from(self.key1_armed)
            }
            // VBK reads $FE|bank on CGB even in DMG mode (boot_hwio-C).
            0xFF4F => self.ppu.read(addr),
            // FF55 reads the live register verbatim: remaining blocks - 1
            // while a HBlank transfer runs, bit 7 set when none is
            // registered (gambatte ioamhram_[0x155]).
            0xFF55 if self.cgb_mode => self.hdma5,
            // RP: bits 2-5 unimplemented (1), bit 1 = received signal,
            // active low — no peer, so never receiving.
            0xFF56 if self.cgb_mode => 0x3C | (self.rp & 0xC1) | 0x02,
            // BCPS/OCPS stay readable in DMG-compat mode (boot_hwio-C reads
            // the boot leftovers $C8/$D0); the data ports do not.
            0xFF68 | 0xFF6A => self.ppu.read(addr),
            0xFF69 | 0xFF6B if self.cgb_mode => self.ppu.read(addr),
            0xFF6C => self.ppu.read(addr),
            0xFF70 if self.cgb_mode => 0xF8 | self.svbk,
            0xFF72 if self.model.is_cgb() => self.ff72,
            0xFF73 if self.model.is_cgb() => self.ff73,
            0xFF74 if self.cgb_mode => self.ff74,
            0xFF75 if self.model.is_cgb() => 0x8F | (self.ff75 & 0x70),
            // FF76/FF77 (PCM12/PCM34): read-only per-channel 4-bit digital
            // outputs (Pan Docs "PCM amplitude readouts").
            0xFF76 if self.model.is_cgb() => self.apu.pcm12(),
            0xFF77 if self.model.is_cgb() => self.apu.pcm34(),
            // FF50 (boot ROM disable) and everything unmapped: $FF.
            _ => 0xFF,
        }
    }

    fn io_write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF00 => self.joypad.write(value),
            0xFF01 | 0xFF02 => self.serial.write(addr, value),
            // A timer write never requests IF directly: a write-induced TIMA
            // overflow raises it only at the reload, from `Timer::tick`.
            0xFF04..=0xFF07 => {
                if addr == 0xFF04 {
                    // The DIV-reset falling edge must reach the serial
                    // clock within this cycle: the once-per-M-cycle
                    // sampled tick would miss it for the CGB fast clock,
                    // whose DIV bit is high again by the next sample
                    // (`Serial::div_write`; gambatte serial/
                    // start83_late_div_write_*).
                    let iff = self.serial.div_write(self.timer.div_counter());
                    self.intf |= iff & IF_MASK;
                    // Same for the frame sequencer: the reset's DIV-APU
                    // falling edge lands in this cycle (`Apu::div_write`).
                    self.apu.div_write(self.double_speed);
                }
                self.timer.write(addr, value)
            }
            0xFF0F => self.intf = value & IF_MASK,
            0xFF10..=0xFF3F => self.apu.write(addr, value),
            0xFF46 => {
                self.dma_reg = value;
                let src = u16::from(value) << 8;
                // A rewrite mid-flight retargets the running transfer
                // immediately: the handover copies before the new
                // transfer's byte 0 read the NEW source at the old
                // indices, and conflict like it (gambatte-core memory.cpp
                // FF46 handler updates ioamhram_[0x146] and re-runs
                // oamDmaInitSetup before the next copy; pinned by gambatte
                // oamdma_src8000_srcchange0000_busyread0000_1/2 — mooneye
                // oam_dma_restart restarts with the same page and cannot
                // discriminate).
                if let Some(run) = &mut self.dma_run {
                    run.src = src;
                }
                self.dma_start = Some(OamDmaStart { src, delay: 1 });
            }
            // PPU register writes can raise the STAT line in this very
            // cycle (stat_lyc_onoff round 4): `Ppu::write` returns the IF
            // bits the write raised, OR-ed in immediately.
            0xFF40 => {
                let was_on = self.ppu.lcd_enabled();
                self.intf |= self.ppu.write(addr, value) & IF_MASK;
                let now_on = self.ppu.lcd_enabled();
                if was_on && !now_on && self.hdma_mode == HdmaMode::ArmedLcdOn {
                    // Display disabled while HBlank DMA is armed: the
                    // arming dies with the LCD — FF55 keeps reading
                    // "active" but no further block ever copies until
                    // FF55 is rewritten (gambatte video.cpp lcdcChange:
                    // the disable branch sets every memevent, including
                    // memevent_hdma, to disabled_time).
                    self.hdma_mode = HdmaMode::Disabled;
                } else if !was_on && now_on && self.hdma_mode == HdmaMode::ArmedLcdOff {
                    // HBlank DMA armed while the LCD was off resumes at
                    // the new frame's mode-0 entries (gambatte
                    // lcdcChange enable branch: `if (hdmaIsEnabled())
                    // setm<memevent_hdma>(predictedNextM0Time())`).
                    self.hdma_mode = HdmaMode::ArmedLcdOn;
                }
            }
            0xFF41..=0xFF45 | 0xFF47..=0xFF4B => self.intf |= self.ppu.write(addr, value) & IF_MASK,
            0xFF4D if self.cgb_mode => self.key1_armed = value & 1 != 0,
            0xFF4F if self.cgb_mode => self.intf |= self.ppu.write(addr, value) & IF_MASK,
            // FF51-FF54 are the *live* DMA address counters, not start
            // latches: `hdma_src`/`hdma_dst` advance as blocks copy, and a
            // register write merges into the current counter value
            // (gambatte memory.cpp cases 0x51-0x54; SameBoy GB_IO_HDMA1-4
            // agree). The high-byte writes keep the counter's full low
            // byte and apply no destination mask — the dest counter is a
            // full 16-bit register, masked only at the VRAM write
            // (`hdma_partial_src_rewrite_blends_live_counter`,
            // `gdma_continues_from_incremented_addresses`,
            // `gdma_terminates_at_dest_0x10000_crossing`).
            0xFF51 if self.cgb_mode => {
                self.hdma_src = (self.hdma_src & 0x00FF) | (u16::from(value) << 8)
            }
            0xFF52 if self.cgb_mode => {
                self.hdma_src = (self.hdma_src & 0xFF00) | u16::from(value & 0xF0)
            }
            0xFF53 if self.cgb_mode => {
                self.hdma_dst = (self.hdma_dst & 0x00FF) | (u16::from(value) << 8)
            }
            0xFF54 if self.cgb_mode => {
                self.hdma_dst = (self.hdma_dst & 0xFF00) | u16::from(value & 0xF0)
            }
            0xFF55 if self.cgb_mode => self.hdma5_write(value),
            0xFF56 if self.cgb_mode => self.rp = value & 0xC1,
            0xFF68 | 0xFF6A => self.intf |= self.ppu.write(addr, value) & IF_MASK,
            0xFF69 | 0xFF6B if self.cgb_mode => self.intf |= self.ppu.write(addr, value) & IF_MASK,
            // OPRI is set up by the boot ROM and locked outside CGB mode.
            0xFF6C if self.cgb_mode => self.intf |= self.ppu.write(addr, value) & IF_MASK,
            0xFF70 if self.cgb_mode => self.svbk = value & 7,
            0xFF72 if self.model.is_cgb() => self.ff72 = value,
            0xFF73 if self.model.is_cgb() => self.ff73 = value,
            0xFF74 if self.cgb_mode => self.ff74 = value,
            0xFF75 if self.model.is_cgb() => self.ff75 = value & 0x70,
            // FF50 boot-disable: we start post-boot; writes are ignored.
            _ => {}
        }
    }
}

/// Zero bits among the bytes the SGB boot ROM transfers to the SNES: six
/// 16-byte packets, each a command byte ($F1 + 2×packet), a checksum byte
/// (8-bit sum of the payload) and 14 payload bytes from $0104 + 14×packet
/// (addresses ≥ $0150 read as $00).
///
/// Each zero bit costs one extra M-cycle of boot time relative to a one
/// bit. Derivation — the SGB boot ROM's per-bit send loop, $0095-$00A5 in
/// the dumped ROM (sha1 aa2f50a77dfb4823da96ba99309085a3c6278515), clocks
/// each bit out to the SNES via JOYP P14/P15:
///
/// ```text
/// $0095  BIT 0,D      ; 2 M-cycles
/// $0097  LD A,$10     ; 2          (P15 low = "1" bit)
/// $0099  JR NZ,$009D  ; 3 taken (one bit) / 2 not taken (zero bit)
/// $009B  LD A,$20     ; 2          (P14 low = "0" bit), zero path only
/// $009D  LDH (C),A    ; pulse, then $30 restores both lines high
/// ```
///
/// A one bit takes 2+2+3 = 7 M-cycles to reach the pulse; a zero bit
/// takes 2+2+2+2 = 8 — exactly one M-cycle (4 DIV T-cycles) more. The
/// per-packet reset and stop pulses are unconditional and sit in the DIV
/// base value instead. Verified against acceptance/boot_div-S and
/// boot_div2-S, which differ only in the global checksum bytes
/// (`model::tests::sgb_div_base_matches_both_checksum_roms`).
fn sgb_header_zero_bits(cart: &Cartridge) -> u32 {
    let mut zeros = 0;
    for packet in 0..6u16 {
        let cmd = 0xF1 + 2 * packet as u8;
        let mut sum = 0u8;
        for i in 0..14 {
            let addr = 0x104 + 14 * packet + i;
            let byte = if addr < 0x150 { cart.read_rom(addr) } else { 0 };
            sum = sum.wrapping_add(byte);
            zeros += byte.count_zeros();
        }
        zeros += cmd.count_zeros() + sum.count_zeros();
    }
    zeros
}

impl Bus for Interconnect {
    fn read(&mut self, addr: u16) -> u8 {
        self.service_vram_dma();
        self.tick_machine();
        // A trigger inside this very cycle still steals the bus before
        // the read samples (see `service_vram_dma`: reads yield, writes
        // in flight commit first).
        self.service_vram_dma();
        self.maybe_oam_bug(addr, OamBugKind::Read);
        self.read_no_tick(addr)
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.service_vram_dma();
        // The CPU drives the data bus during the second half of the write
        // M-cycle (gbctr "Memory access timing"), which the dot-clocked
        // pixel pipeline can observe mid-cycle: stage rendering-register
        // writes with the PPU before ticking. The architectural commit
        // below is unchanged — `Ppu::stage_write` affects only the
        // pipeline's register view (mealybug m3_* mid-mode-3 writes).
        if let 0xFF40 | 0xFF42 | 0xFF43 | 0xFF47..=0xFF4B = addr {
            let dots = if self.double_speed { 1 } else { 2 };
            self.ppu.stage_write(addr, value, dots);
        }
        self.tick_machine();
        // Corruption first, then the (mode-blocked) write attempt — during
        // the scan the CPU byte never lands (oam_write_blocked).
        self.maybe_oam_bug(addr, OamBugKind::Write);
        self.write_no_tick(addr, value);
    }

    fn tick(&mut self) {
        self.service_vram_dma();
        self.tick_machine();
    }

    fn tick_addr(&mut self, value: u16) {
        self.service_vram_dma();
        self.tick_machine();
        self.maybe_oam_bug(value, OamBugKind::Write);
    }

    fn read_inc(&mut self, addr: u16) -> u8 {
        self.service_vram_dma();
        self.tick_machine();
        self.service_vram_dma(); // reads yield to a same-cycle trigger
        self.maybe_oam_bug(addr, OamBugKind::ReadIncrease);
        self.read_no_tick(addr)
    }

    fn pending(&self) -> u8 {
        self.intf & self.ie & IF_MASK & !self.if_stat_late
    }

    fn pending_halt_wake(&self) -> u8 {
        // The halt-exit logic samples IE & IF *within* the M-cycle, not at
        // its end (SameBoy sm83_cpu.c `GB_cpu_run`: DMG samples mid-cycle
        // after 2 of 4 T-cycles, CGB/AGB at the start of the cycle), so a
        // timer reload + IF commit — which lands on the last T-substep
        // under the hardware DIV phase (div ≡ 0 mod 4 at boundaries) — is
        // missed until the next cycle: the halt wake comes one M-cycle
        // later than a running-CPU dispatch would (gambatte tima/tc*_irq_*
        // on both models; wilbertpol timer_if rounds 5/6 vs 3/4).
        //
        // The STAT bit joins the mask per event, not wholesale: the PPU
        // flags its second-half IF commits (line-start OAM pulses, mode-0
        // rises on dots ≡ 3,0 mod 4) via `Ppu::take_stat_halt_late`, which
        // ORs IF_STAT into `if_late` for exactly those cycles — the
        // gbmicrotest int_oam_*/int_hblank_halt_scx* grids and the
        // mooneye/wilbertpol hblank halt groupings pin the law, while
        // first-half STAT commits and the vblank IF stay live
        // (halt_ime1_timing2-GS, vblank, DMG). The known unmodelled
        // remainder is the CGB/AGB start-of-cycle staleness for first-half
        // PPU commits (halt_ime1_timing2-GS's "fail: CGB, AGB, AGS";
        // gambatte halt/*_cgb04c split rows): landing it requires a
        // per-model widening of the halt-late mask, a separate work
        // package.
        (self.intf & !self.if_late) & self.ie & IF_MASK
    }

    fn ack(&mut self, bit: u8) {
        self.intf &= !(1 << bit);
        // gambatte Memory::ackIrq syncs the acked bit's source a few
        // T-cycles past the ack point before clearing, so a hardware
        // re-set landing just after the dispatch's IF clear is consumed
        // by it (see the `ack_squash_*` field docs for the window
        // derivation and the pinning ROMs).
        match bit {
            0 | 1 => {
                // lcd_.update(cc + 2), no isCgb term: 2 dots into the
                // next machine tick on both families and at both speeds
                // (in double speed that is the whole 2-dot tick). The
                // line-anchored rises' single-speed second-half emission
                // dots stay OUT of reach — see the field docs.
                self.ack_squash_mask = 1 << bit;
                self.ack_squash_ticks = 0;
                self.ack_squash_dots = 2;
            }
            2 | 3 => {
                // updateTimaIrq(cc + 2 + isCgb()) / updateSerial(cc + 3 +
                // isCgb()): with the timer IF on the last T-substep and
                // the serial IF on the DIV-edge boundary, both windows
                // cover the set produced by the next machine tick on the
                // DMG family and the next two on CGB/AGB.
                self.ack_squash_mask = 1 << bit;
                self.ack_squash_ticks = if self.model.is_cgb() { 2 } else { 1 };
                self.ack_squash_dots = 0;
            }
            _ => {}
        }
    }

    fn stop(&mut self, skipped_addr: u16, interrupt_pending: bool) -> bool {
        let switching = self.cgb_mode && self.key1_armed;
        let entering_ds = switching && !self.double_speed;
        // gambatte Memory::stop snapshots the HDMA situation at the
        // pre-read cc: a block request still pending when STOP executes
        // (flagged mid-instruction — no boundary came) is deferred when
        // leaving double speed (haltHdmaState_ = hdma_requested +
        // ackDmaReq) but stays flagged when entering it, firing *inside*
        // the pause where the gated core clock aborts the HBlank transfer
        // with the count latched (dma()'s halted path; pinned by
        // hdma_transition_speedchange_hdmalen*_hdma5 → $80|len vs
        // hdma_late_m3speedchange_hdma5_*_ds_1 → still active).
        let in_window = self.hdma_mode == HdmaMode::ArmedLcdOn && self.ppu.hdma_period();
        let pending_req = self.vram_dma_req.take();
        if switching && !entering_ds {
            // Leaving double speed: the PPU/APU re-pace from the cycle
            // right after the STOP opcode fetch (gambatte lcd_/psg_
            // .speedChange at cc_ = cc + 8 * !isDoubleSpeed(): offset 0
            // leaving, +8 entering), so the toggle precedes the
            // skipped-byte read below; entering double speed it lands
            // after the read + internal cycle instead.
            self.double_speed = false;
            self.ppu.set_double_speed(false);
        }
        if !interrupt_pending {
            // The skipped byte costs one real read M-cycle (SameBoy
            // stop(): `cycle_read(gb, gb->pc++)`, gated on no pending
            // interrupt). The value is discarded; the address still
            // drives the bus (OAM bug).
            self.tick_machine();
            self.maybe_oam_bug(skipped_addr, OamBugKind::Read);
            let _ = self.read_no_tick(skipped_addr);
        }
        // STOP resets DIV on every model (Pan Docs "FF04 — DIV"),
        // committing like a write occupying the skipped-byte read slot:
        // gambatte Memory::stop timestamps `nontrivial_ff_write(0x04, 0,
        // cc)` at the slot's *start* cc, and gambatte write timestamps are
        // start-of-cycle (cpu.cpp FF_WRITE advances cc afterwards) where
        // ours commit after the tick — so the reset lands here, after that
        // cycle's tick (the gambatte speedchange tima/div a/b phase pairs
        // pin the TIMA falling-edge quirk to this cell). Modelled as a DIV
        // write so the falling-edge effects apply (frame-sequencer edge
        // included, `Apu::div_write` — the speedchange ch2_nr52 families).
        self.apu.div_write(self.double_speed);
        self.timer.write(0xFF04, 0);
        if !switching {
            // Deep stop: hand a still-pending block request back — the
            // CPU's stop idle engages the halt gate, which defers it
            // (gambatte's non-switch stop path calls Memory::halt).
            self.vram_dma_req = pending_req.or(self.vram_dma_req);
            return false;
        }
        self.key1_armed = false;
        if interrupt_pending {
            // With IE & IF pending the switch is instantaneous: no
            // skipped-byte read, no pause (SameBoy stop() gates the halt
            // countdown on !interrupt_pending; age caution/
            // spsw-interrupts).
            if entering_ds {
                self.double_speed = true;
                self.ppu.set_double_speed(true);
            }
            self.vram_dma_req = pending_req.or(self.vram_dma_req);
            return true;
        }
        // The OAM DMA controller freezes after the read cycle (gambatte
        // Memory::stop: updateOamDma(cc + 4), then intreq_.halt()); the
        // halt-hdma snapshot below is installed first so the wake path
        // can re-evaluate it.
        self.halt_hdma = if pending_req.is_some() && !entering_ds {
            HaltHdmaState::Requested
        } else if in_window {
            HaltHdmaState::High
        } else {
            HaltHdmaState::Low
        };
        self.engage_halt_gate(true);
        // One internal M-cycle before the pause (gambatte Memory::stop
        // returns cc + 8: the operand read plus one cycle), still at the
        // old PPU/APU pace when entering double speed.
        self.tick_machine();
        if entering_ds {
            self.double_speed = true;
            self.ppu.set_double_speed(true);
        }
        // Mode-0 entries seen by the two cycles above never flag a block:
        // gambatte defers all LCD events into the pause, where the halted
        // gate suppresses the flag; the live window is re-checked at wake.
        self.vram_dma_req = None;
        // The pause: the CPU sleeps for 0x7FFF more M-cycles on the *new*
        // clock — with the read + internal cycles that totals 0x8001
        // M-cycles ≙ gambatte's unhalt event at cc + 0x20000 + 4 (cc
        // counts 4 per M-cycle at either speed) — while PPU/APU/timer run
        // on. IE & IF != 0 ends it early, exactly like halt mode
        // (gambatte's pause *is* a halt: the halted intevent_interrupts
        // path unhalts; SameBoy keeps gb->halted under
        // speed_switch_halt_countdown). SameBoy instead uses a flat
        // 0x20008 8-MHz-clock countdown — half the pause when leaving
        // double speed; gambatte's cgb04c expectations are this suite's
        // oracle, and the speedchange2/3/4/5 (DS→single) LY families
        // confirm its doubled length.
        let dots_per_m: u64 = if self.double_speed { 2 } else { 4 };
        let target = self.cycles + 0x7FFF * dots_per_m;
        if entering_ds && pending_req.is_some() {
            // The surviving block request fires at the first event check
            // inside the pause: the halted service aborts the transfer
            // (see run_vram_dma). Its stall counts toward the pause.
            self.vram_dma_req = pending_req;
            self.run_vram_dma();
        }
        while self.cycles < target && self.intf & self.ie & IF_MASK == 0 {
            self.tick_machine();
        }
        self.engage_halt_gate(false);
        self.vram_dma_unhalt();
        true
    }

    fn set_halted(&mut self, halted: bool) {
        self.set_cpu_halted(halted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 32 KiB no-MBC cart. `0x1000..0x1100` carries a recognisable pattern
    /// for DMA source tests.
    fn test_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 0x8000];
        for i in 0..0x100usize {
            rom[0x1000 + i] = (i as u8) ^ 0x5A;
        }
        rom
    }

    fn ic(model: Model) -> Interconnect {
        Interconnect::new(model, Cartridge::from_bytes(test_rom()).unwrap())
    }

    fn ic_cgb_mode() -> Interconnect {
        let mut rom = test_rom();
        rom[0x143] = 0x80;
        Interconnect::new(Model::Cgb, Cartridge::from_bytes(rom).unwrap())
    }

    fn ticks(b: &mut Interconnect, n: u32) {
        for _ in 0..n {
            b.tick();
        }
    }

    // ---- memory map -----------------------------------------------------

    #[test]
    fn rom_reads_route_to_cartridge() {
        let mut b = ic(Model::Dmg);
        assert_eq!(b.read(0x1000), 0x5A);
        assert_eq!(b.read(0x1001), 0x5B);
    }

    #[test]
    fn wram_and_echo_are_the_same_memory() {
        let mut b = ic(Model::Dmg);
        b.write(0xC000, 0x11);
        b.write(0xDDFF, 0x22);
        assert_eq!(b.read(0xE000), 0x11);
        assert_eq!(b.read(0xFDFF), 0x22);
        b.write(0xE123, 0x33);
        assert_eq!(b.read(0xC123), 0x33);
    }

    #[test]
    fn hram_round_trips() {
        let mut b = ic(Model::Dmg);
        b.write(0xFF80, 0xAB);
        b.write(0xFFFE, 0xCD);
        assert_eq!(b.read(0xFF80), 0xAB);
        assert_eq!(b.read(0xFFFE), 0xCD);
    }

    #[test]
    fn ie_stores_all_8_bits() {
        let mut b = ic(Model::Dmg);
        b.write(0xFFFF, 0xFF);
        assert_eq!(b.read(0xFFFF), 0xFF);
        b.write(0xFFFF, 0xE4);
        assert_eq!(b.read(0xFFFF), 0xE4);
    }

    #[test]
    fn if_upper_three_bits_read_one() {
        let mut b = ic(Model::Dmg);
        b.write(0xFF0F, 0x00);
        assert_eq!(b.read(0xFF0F), 0xE0);
        b.write(0xFF0F, 0xFF);
        assert_eq!(b.read(0xFF0F), 0xFF);
        assert_eq!(b.pending(), 0); // IE = 0
        b.write(0xFFFF, 0x1F);
        assert_eq!(b.pending(), 0x1F);
        b.ack(0);
        assert_eq!(b.read(0xFF0F), 0xFE);
    }

    // ---- halt-exit IE & IF sampling (Bus::pending_halt_wake) ------------

    /// Arm the timer so that the reload + IF commit lands on the last
    /// T-substep of M-cycle 5 (div starts at 0, TAC bit 3 = 16 T period:
    /// falling edge at div 16 on the last substep of cycle 4, reload one
    /// cycle later on the same substep).
    fn arm_late_timer_irq(b: &mut Interconnect) {
        b.ie = 0x04;
        b.timer.write(0xFF07, 0x05);
        b.timer.write(0xFF05, 0xFF);
    }

    /// A timer IF committed in the second half of an M-cycle is readable
    /// and `pending()`-visible in that cycle (the running CPU's frozen
    /// end-of-fetch sampling), but the mid-cycle halt-exit sampling misses
    /// it until the next cycle, on every model (gambatte tima/tc*_irq_*
    /// dmg08+cgb04c shared expectations; wilbertpol timer_if rounds 5/6
    /// vs 3/4 on its full model matrix; SameBoy `GB_cpu_run`).
    #[test]
    fn halt_wake_misses_late_timer_if_for_one_cycle() {
        for model in [Model::Dmg, Model::Cgb, Model::Agb] {
            let mut b = ic(model);
            arm_late_timer_irq(&mut b);
            ticks(&mut b, 5); // cycle 5 = the reload + IF commit cycle
            assert_eq!(b.read_no_tick(0xFF0F) & 0x04, 0x04, "{model:?}: IF read");
            assert_eq!(b.pending(), 0x04, "{model:?}: running-CPU sampling");
            assert_eq!(b.pending_halt_wake(), 0, "{model:?}: halt wake misses it");
            b.tick();
            assert_eq!(b.pending_halt_wake(), 0x04, "{model:?}: visible next cycle");
        }
    }

    /// Non-timer IF bits stay live for the halt wake: the PPU IRQ anchors
    /// are calibrated against the running CPU's end-of-fetch sampling, so
    /// the intra-cycle offset is already absorbed there (mooneye
    /// intr_2_0_timing passes on all models against this view; see
    /// `pending_halt_wake` for the unmodelled CGB remainder).
    #[test]
    fn halt_wake_sees_non_timer_if_in_the_same_cycle() {
        for model in [Model::Dmg, Model::Cgb] {
            let mut b = ic(model);
            b.ie = 0x01;
            b.write(0xFF0F, 0x01); // bit lands during this M-cycle
            assert_eq!(b.pending_halt_wake(), 0x01, "{model:?}");
        }
    }

    /// The mode-0 STAT rise's half-cycle halt law (`Ppu::take_m0_rise` →
    /// `if_late`): the IF bit is readable and dispatch-visible within its
    /// own M-cycle for every phase, but the halt-exit sampler misses a
    /// rise committed in the cycle's second half (PPU dots 3-4) for one
    /// M-cycle. With the LCD enabled at an M-cycle boundary the rise dot
    /// is 254 + SCX%8 on line 1 (glitch line 452 dots, ≡ 0 mod 4):
    /// SCX=0 → dot ≡ 2 (first half, halt-visible at once), SCX=1 →
    /// dot ≡ 3 (second half, halt-late). mooneye hblank_ly_scx_timing-GS
    /// and gbmicrotest int_hblank_halt_scx0-7 pin all eight phases.
    #[test]
    fn m0_rise_second_half_commit_is_halt_late() {
        for (scx, late) in [(0u8, false), (1, true)] {
            let mut b = ic(Model::Dmg);
            b.ie = 0x02;
            b.write(0xFF43, scx);
            b.write(0xFF41, 0x08); // hblank STAT source
            b.write(0xFF40, 0x91);
            // Line 1 starts at dot 452 (the enable line is 4 dots
            // short); its mode-0 rise lands at 452 + 254 + SCX%8.
            let rise = 452 + 254 + u32::from(scx);
            // Run whole M-cycles up to the one containing the rise,
            // then drop the enable line's own rise from IF.
            ticks(&mut b, rise.div_ceil(4) - 1);
            b.intf = 0;
            assert_eq!(b.pending(), 0, "scx {scx}: not risen yet");
            b.tick();
            assert_eq!(b.pending(), 0x02, "scx {scx}: dispatch-visible");
            assert_eq!(
                b.pending_halt_wake(),
                if late { 0 } else { 0x02 },
                "scx {scx}: halt-wake view"
            );
            b.tick();
            assert_eq!(b.pending_halt_wake(), 0x02, "scx {scx}: next cycle");
        }
    }

    #[test]
    fn ff50_reads_ff_and_ignores_writes() {
        let mut b = ic(Model::Dmg);
        b.write(0xFF50, 0x00);
        assert_eq!(b.read(0xFF50), 0xFF);
    }

    #[test]
    fn unmapped_io_reads_ff() {
        let mut b = ic(Model::Dmg);
        for addr in [
            0xFF03, 0xFF08, 0xFF0E, 0xFF4C, 0xFF4E, 0xFF57, 0xFF6D, 0xFF7F,
        ] {
            assert_eq!(b.read(addr), 0xFF, "{addr:04X}");
        }
    }

    #[test]
    fn dmg_has_no_cgb_registers() {
        let mut b = ic(Model::Dmg);
        for addr in [
            0xFF4D, 0xFF4F, 0xFF51, 0xFF52, 0xFF53, 0xFF54, 0xFF55, 0xFF56, 0xFF68, 0xFF69, 0xFF6A,
            0xFF6B, 0xFF6C, 0xFF70, 0xFF72, 0xFF73, 0xFF74, 0xFF75, 0xFF76, 0xFF77,
        ] {
            b.write(addr, 0x00);
            assert_eq!(b.read(addr), 0xFF, "{addr:04X}");
        }
    }

    // ---- dispatch-ack source sync-ahead (gambatte Memory::ackIrq) -------

    /// A timer IF set produced by the machine tick right after a
    /// dispatch ack is consumed by it on both families (gambatte ackIrq
    /// `updateTimaIrq(cc + 2 + isCgb())` reaches past the last-substep
    /// commit of the next M-cycle's reload; tima/tc00_irq_late_retrigger_3
    /// reads E0 on dmg08 *and* cgb04c). The TMA reload itself still
    /// happens — only the IF bit is consumed.
    #[test]
    fn dispatch_ack_consumes_timer_set_due_next_cycle() {
        for model in [Model::Dmg, Model::Cgb] {
            let mut b = ic(model);
            arm_late_timer_irq(&mut b);
            ticks(&mut b, 4); // overflow armed; reload + IF due next tick
            b.ack(2); // the dispatch's IF clear
            ticks(&mut b, 1);
            assert_eq!(b.read_no_tick(0xFF0F) & 0x04, 0, "{model:?}");
            assert_eq!(
                b.timer.read(0xFF05),
                b.timer.read(0xFF06),
                "{model:?}: reload"
            );
        }
    }

    /// The sync-ahead window is one M-cycle on the DMG family and two on
    /// CGB/AGB (`+ isCgb()`): a set committing in the second tick after
    /// the ack survives on DMG and is consumed on CGB — the
    /// tc00_irq_late_retrigger_2 dmg08_outE4 / cgb04c_outE0 split. Three
    /// cycles out it survives everywhere.
    #[test]
    fn dispatch_ack_timer_window_is_one_cycle_dmg_two_cgb() {
        for (model, expect) in [
            (Model::Dmg, 0x04),
            (Model::Sgb, 0x04),
            (Model::Cgb, 0x00),
            (Model::Agb, 0x00),
        ] {
            let mut b = ic(model);
            arm_late_timer_irq(&mut b);
            ticks(&mut b, 3);
            b.ack(2);
            ticks(&mut b, 2); // overflow in tick 4, reload + IF in tick 5
            assert_eq!(b.read_no_tick(0xFF0F) & 0x04, expect, "{model:?}");
        }
        for model in [Model::Dmg, Model::Cgb] {
            let mut b = ic(model);
            arm_late_timer_irq(&mut b);
            ticks(&mut b, 2);
            b.ack(2);
            ticks(&mut b, 3);
            assert_eq!(
                b.read_no_tick(0xFF0F) & 0x04,
                0x04,
                "{model:?}: past window"
            );
        }
    }

    /// Serial transfer-complete IF: same ack windows via gambatte's
    /// `updateSerial(cc + 3 + isCgb())` — with the completion on the
    /// DIV-edge boundary, DMG consumes the set due in the next tick,
    /// CGB also the one after (serial/start_wait_trigger_int8_read_if_2:
    /// dmg08_outE8 vs cgb04c_outE0; round 3 E0 on both).
    #[test]
    fn dispatch_ack_consumes_serial_set_like_gambatte_ackirq() {
        // Completion (8th shift) at div 4096 = machine tick 1024.
        for (model, gap, expect) in [
            (Model::Dmg, 1, 0x00),
            (Model::Cgb, 1, 0x00),
            (Model::Dmg, 2, 0x08),
            (Model::Cgb, 2, 0x00),
            (Model::Dmg, 3, 0x08),
            (Model::Cgb, 3, 0x08),
        ] {
            let mut b = ic(model);
            b.serial.write(0xFF01, 0x00);
            b.serial.write(0xFF02, 0x81);
            ticks(&mut b, 1024 - gap);
            b.ack(3);
            ticks(&mut b, gap);
            assert_eq!(b.read_no_tick(0xFF0F) & 0x08, expect, "{model:?} gap {gap}");
            assert_eq!(
                b.serial.read(0xFF02) & 0x80,
                0,
                "{model:?}: transfer still ends"
            );
        }
    }

    /// The ack only consumes the *acked* source: a timer ack does not
    /// swallow a serial set in the window (gambatte ackIrq clears one
    /// bit; the sync-ahead merely flags the others earlier).
    #[test]
    fn dispatch_ack_squash_is_per_source() {
        let mut b = ic(Model::Cgb);
        b.serial.write(0xFF02, 0x81);
        ticks(&mut b, 1023);
        b.ack(2); // timer ack, serial completion due next tick
        ticks(&mut b, 1);
        assert_eq!(b.read_no_tick(0xFF0F) & 0x08, 0x08);
    }

    /// STAT/VBlank rises go through `lcd_.update(cc + 2)` — only the
    /// first 2 dots of the next tick. The vblank rise is a line-anchored
    /// event emitted in the *second half* of its M-cycle at single
    /// speed, so an ack in the cycle before must NOT consume it
    /// (gambatte m2int_m2irq_late_retrigger_1 and
    /// irq_precedence/late_m0irq_retrigger_scx1_1 pin the keeps; the
    /// consumed cases live on the gambatte `*_late_retrigger_ds_2` rows,
    /// where the 2-dot window spans the whole double-speed tick, and on
    /// the mode-0 rise's early-dot grid).
    #[test]
    fn dispatch_ack_does_not_reach_single_speed_line_anchored_rises() {
        for model in [Model::Dmg, Model::Cgb] {
            // Find the tick of the first vblank IF after an LCD enable
            // (per model: the CGB line timeline may shift it).
            let rise = {
                let mut b = ic(model);
                b.write_no_tick(0xFF40, 0x91);
                let mut n = 0;
                while b.read_no_tick(0xFF0F) & 0x01 == 0 {
                    b.tick();
                    n += 1;
                }
                n
            };
            for gap in [1, 2] {
                let mut b = ic(model);
                b.write_no_tick(0xFF40, 0x91);
                ticks(&mut b, rise - gap);
                b.ack(0);
                ticks(&mut b, gap);
                assert_eq!(
                    b.read_no_tick(0xFF0F) & 0x01,
                    0x01,
                    "{model:?} gap {gap}: kept"
                );
            }
        }
    }

    // ---- tick-then-access -----------------------------------------------

    #[test]
    fn access_observes_state_after_the_cycles_tick() {
        let mut b = ic(Model::Dmg);
        // TAC = freq 01 (DIV bit 3, every 16 T). Write cycle: div 0 -> 4.
        b.write(0xFF07, 0x05);
        b.tick(); // div 8
        assert_eq!(b.read(0xFF05), 0, "read cycle: div 12, no edge yet");
        // This read's own tick takes div to 16 — the bit-3 falling edge
        // clocks TIMA *before* the access observes it.
        assert_eq!(b.read(0xFF05), 1);
    }

    #[test]
    fn timer_overflow_requests_if_bit2() {
        let mut b = ic(Model::Dmg);
        b.write(0xFF05, 0xFF);
        b.write(0xFF07, 0x05);
        ticks(&mut b, 8);
        assert_eq!(b.read(0xFF0F) & 0x04, 0x04);
    }

    #[test]
    fn joypad_press_requests_if_bit4() {
        let mut b = ic(Model::Dmg);
        b.write(0xFF00, 0x10); // select the button column
        b.joypad_mut().press(crate::joypad::Button::Start);
        b.tick();
        assert_eq!(b.read(0xFF0F) & 0x10, 0x10);
        assert_eq!(b.read(0xFF00), 0xD7);
    }

    #[test]
    fn vblank_requests_if_bit0() {
        let mut b = ic(Model::Dmg);
        b.write(0xFF40, 0x91);
        // 145 lines is comfortably past the vblank IF at 144:4.
        ticks(&mut b, 145 * 114);
        assert_eq!(b.read(0xFF0F) & 0x01, 0x01);
    }

    #[test]
    fn serial_transfer_requests_if_bit3() {
        let mut b = ic(Model::Dmg);
        b.write(0xFF02, 0x81);
        ticks(&mut b, 8 * 128 + 2);
        assert_eq!(b.read(0xFF0F) & 0x08, 0x08);
        assert_eq!(b.read(0xFF01), 0xFF);
    }

    // ---- OAM DMA ---------------------------------------------------------

    /// Fill WRAM 0xC000.. with `base+i` through untimed writes.
    fn fill_wram(b: &mut Interconnect, addr: u16, base: u8, len: u16) {
        for i in 0..len {
            b.write_no_tick(addr + i, base.wrapping_add(i as u8));
        }
    }

    #[test]
    fn oam_dma_setup_cycle_leaves_oam_accessible() {
        let mut b = ic(Model::Dmg);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        b.write(0xFF46, 0xC0); // cycle W
        // Cycle W+1: setup delay, OAM still reads its old content
        // (oam_dma_start executes an opcode from OAM here).
        assert_eq!(b.read(0xFE00), 0x00);
        // Cycle W+2: byte 0 is in flight, OAM reads $FF.
        assert_eq!(b.read(0xFE00), 0xFF);
    }

    /// acceptance/oam_dma_timing: OAM unlocks exactly 162 M-cycles after
    /// the FF46 write cycle (1 setup + 160 transfer + the access cycle).
    #[test]
    fn oam_dma_timing_exact() {
        for (extra, expected) in [(0u32, 0xFF), (1, 0x80)] {
            let mut b = ic(Model::Dmg);
            fill_wram(&mut b, 0xC000, 0x80, 160);
            b.write(0xFF46, 0xC0);
            ticks(&mut b, 160 + extra);
            assert_eq!(b.read(0xFE00), expected, "extra={extra}");
        }
    }

    #[test]
    fn oam_dma_copies_all_160_bytes() {
        let mut b = ic(Model::Dmg);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        b.write(0xFF46, 0xC0);
        ticks(&mut b, 161);
        assert_eq!(b.read(0xFE00), 0x80);
        assert_eq!(b.read(0xFE9F), 0x80u8.wrapping_add(159));
    }

    #[test]
    fn oam_dma_reg_reads_back_last_write() {
        let mut b = ic(Model::Dmg);
        b.write(0xFF46, 0x90);
        assert_eq!(b.read(0xFF46), 0x90);
        b.write(0xFF46, 0x8F); // restart mid-transfer
        assert_eq!(b.read(0xFF46), 0x8F);
    }

    /// acceptance/oam_dma/sources-GS: source pages $E0-$FF re-read WRAM,
    /// including $FE/$FF -> $DE00/$DF00.
    #[test]
    fn oam_dma_high_sources_read_wram_echo() {
        for (page, base) in [(0xE0u8, 0x80u8), (0xFE, 0x21), (0xFF, 0x42)] {
            let mut b = ic(Model::Dmg);
            fill_wram(&mut b, 0xC000, 0x80, 160);
            fill_wram(&mut b, 0xDE00, 0x21, 0x100);
            fill_wram(&mut b, 0xDF00, 0x42, 0x100);
            b.write(0xFF46, page);
            ticks(&mut b, 161);
            assert_eq!(b.read(0xFE00), base, "page {page:02X}");
            assert_eq!(b.read(0xFE01), base + 1, "page {page:02X}");
        }
    }

    #[test]
    fn oam_dma_from_rom_and_vram() {
        let mut b = ic(Model::Dmg);
        b.write(0x9000, 0x77); // LCD off: VRAM writable
        b.write(0xFF46, 0x10); // ROM pattern page
        ticks(&mut b, 161);
        assert_eq!(b.read(0xFE00), 0x5A);
        b.write(0xFF46, 0x90);
        ticks(&mut b, 161);
        assert_eq!(b.read(0xFE00), 0x77);
    }

    #[test]
    fn oam_writes_dropped_and_reads_ff_during_dma() {
        let mut b = ic(Model::Dmg);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        b.write(0xFF46, 0xC0);
        b.tick(); // setup
        b.write(0xFE10, 0x99); // transfer running: dropped
        assert_eq!(b.read(0xFEA0), 0xFF); // prohibited area also $FF
        ticks(&mut b, 161);
        assert_eq!(b.read(0xFE10), 0x90, "DMA value, not the CPU write");
    }

    /// gbctr bus conflicts: a CPU read on the bus the DMA is using returns
    /// the byte the DMA is transferring; the other bus is unaffected.
    /// (Write at cycle W; byte i is in flight at cycle W+2+i, so reads at
    /// W+3, W+4, ... observe bytes 1, 2, ...)
    #[test]
    fn oam_dma_bus_conflicts() {
        // ROM source (external bus): ROM/WRAM reads conflict on DMG, VRAM
        // reads do not.
        let mut b = ic(Model::Dmg);
        b.write(0x8500, 0x33);
        b.write(0xFF46, 0x10); // cycle W
        b.tick(); // W+1 setup
        b.tick(); // W+2: byte 0 in flight
        assert_eq!(b.read(0x4242), 0x5A ^ 1, "ROM read sees DMA byte 1");
        assert_eq!(b.read(0xC000), 0x5A ^ 2, "DMG WRAM shares the bus");
        assert_eq!(b.read(0x8500), 0x33, "VRAM bus unaffected");

        // VRAM source: external bus unaffected.
        let mut b = ic(Model::Dmg);
        b.write(0x8000, 0x44);
        b.write(0x8001, 0x45);
        b.write(0xFF46, 0x80);
        b.tick();
        b.tick();
        assert_eq!(b.read(0x9999), 0x45, "VRAM read sees DMA byte 1");
        assert_eq!(b.read(0x1000), 0x5A, "external bus unaffected");
    }

    /// The OAM DMA controller runs on the CPU core clock, which HALT gates
    /// off (the PPU keeps its own clock): a transfer in progress does not
    /// proceed while the CPU is halted. Bytes already copied stay, the byte
    /// in flight never commits, the rest of OAM keeps its old contents, and
    /// the transfer resumes exactly where it stopped when the CPU wakes.
    /// Hardware-verified by madness/mgb_oam_dma_halt_sprites.s: halting
    /// after the third byte's read leaves that OAM byte un-replaced, and the
    /// PPU renders from the old/new mixture indefinitely.
    #[test]
    fn oam_dma_freezes_while_cpu_halted() {
        let mut b = ic(Model::Mgb);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        b.write_no_tick(0xFE02, 0x30); // old OAM byte the freeze must keep
        b.write(0xFF46, 0xC0); // cycle W
        b.tick(); // W+1: setup delay
        b.tick(); // W+2: byte 0 in flight
        b.tick(); // W+3: byte 1 in flight
        b.set_cpu_halted(true);
        // Frozen for hundreds of M-cycles: no progress. (On hardware the
        // halted CPU performs no bus accesses, so these reads observe
        // unobservable state: raw OAM, no bus conflict — LCD is off here.)
        for _ in 0..200 {
            assert_eq!(b.read(0xFE00), 0x80, "copied byte 0 stays");
        }
        assert_eq!(b.read(0xFE01), 0x81, "copied byte 1 stays");
        assert_eq!(b.read(0xFE02), 0x30, "frozen: old OAM byte persists");
        assert_eq!(b.read(0xC000), 0x80, "no DMA traffic on the external bus");
        // Waking copies byte 2 in the release's catch-up cycle (see
        // `halt_wake_advances_oam_dma_one_catchup_cycle`); 157 transfer
        // cycles remain after it.
        b.set_cpu_halted(false);
        ticks(&mut b, 156);
        assert_eq!(b.read(0xFE00), 0xFF, "byte 159 in flight: OAM blocked");
        assert_eq!(b.read(0xFE00), 0x80, "transfer complete");
        assert_eq!(b.read(0xFE02), 0x82, "resumed transfer replaced the byte");
        assert_eq!(b.read(0xFE9F), 0x80u8.wrapping_add(159));
    }

    /// Releasing the core-clock gate advances a frozen OAM DMA by one
    /// catch-up M-cycle *at the release itself*, before the CPU's first
    /// post-wake cycle: the controller's clock restarts with the halt
    /// exit, one M-cycle ahead of the CPU pipeline (SameBoy sm83_cpu.c
    /// `GB_cpu_run` halt exit: `gb->dma_cycles = 4; GB_dma_run(gb)` on
    /// both the IME=0 resume and the dispatch path, while `GB_dma_run`
    /// itself returns early whenever `gb->halted`). Hardware-pinned by
    /// gambatte oamdma/oamdmasrc80_halt_lycirq_read8000 /
    /// _m2irq_read8000 (out81, both models), dma/hdma_transition_oamdma_2
    /// (out67) and dma/hdma_transition_speedchange_oamdma (out71), all of
    /// which observe the in-flight source index after a wake.
    #[test]
    fn halt_wake_advances_oam_dma_one_catchup_cycle() {
        let mut b = ic(Model::Dmg);
        fill_wram(&mut b, 0xC000, 0x50, 0xA0);
        b.write(0xFF46, 0xC0); // cycle W
        ticks(&mut b, 6); // W+2..W+6 copy idx 0..4
        b.set_cpu_halted(true);
        ticks(&mut b, 50);
        assert_eq!(b.peek(0xFE05), 0x00, "frozen");
        b.set_cpu_halted(false);
        assert_eq!(b.peek(0xFE05), 0x55, "catch-up copy at the gate release");
        assert_eq!(b.peek(0xFE06), 0x00, "exactly one cycle of catch-up");
        b.tick();
        assert_eq!(b.peek(0xFE06), 0x56);
    }

    /// The speed-switch pause releases the same core-clock gate but
    /// performs *no* catch-up cycle: the next OAM DMA byte copies on the
    /// first machine cycle after the pause, not at the release (gambatte
    /// oamdma/oamdmasrcC0_speedchange_readC000 out11 pins the exact
    /// post-pause in-flight index, one position below a caught-up resume;
    /// SameBoy's `speed_switch_halt_countdown` expiry likewise just clears
    /// `halted` with no `GB_dma_run` call, unlike its halt-exit paths).
    #[test]
    fn speed_switch_pause_exit_does_not_catch_up_oam_dma() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x50, 0xA0);
        b.write(0xFF4D, 0x01); // arm the switch
        b.write(0xFF46, 0xC0); // cycle W
        ticks(&mut b, 6); // W+2..W+6 copy idx 0..4
        assert!(b.stop(0x0000, false)); // read cycle copies idx 5, then pause
        assert_eq!(b.peek(0xFE05), 0x55);
        assert_eq!(b.peek(0xFE06), 0x00, "frozen across the pause, no catch-up");
        b.tick();
        assert_eq!(
            b.peek(0xFE06),
            0x56,
            "resumes on the first post-pause cycle"
        );
    }

    /// The FF46 1 M-cycle setup delay counts on the same gated clock, so a
    /// CPU halting right after the FF46 write freezes the transfer before
    /// its first byte (companion to `oam_dma_freezes_while_cpu_halted`).
    #[test]
    fn oam_dma_setup_delay_freezes_while_cpu_halted() {
        let mut b = ic(Model::Mgb);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        b.write(0xFF46, 0xC0);
        b.set_cpu_halted(true);
        for _ in 0..10 {
            assert_eq!(b.read(0xFE00), 0x00, "setup delay frozen: no transfer");
        }
        // The release's catch-up cycle elapses the setup delay; the next
        // cycle copies byte 0.
        b.set_cpu_halted(false);
        assert_eq!(b.read(0xFE00), 0xFF, "byte 0 in flight");
        ticks(&mut b, 159);
        assert_eq!(b.read(0xFE00), 0x80, "transfer complete");
    }

    /// Gating the clock mid-transfer hands the PPU the frozen in-flight
    /// access (OAM index + source byte) for the MGB OAM scan glitch
    /// (madness/mgb_oam_dma_halt_sprites.s); ungating (or freezing with no
    /// transfer / only the setup delay in flight) hands over nothing.
    #[test]
    fn cpu_halt_hands_frozen_dma_access_to_ppu() {
        let mut b = ic(Model::Mgb);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        b.set_cpu_halted(true);
        assert_eq!(b.ppu.oam_dma_freeze(), None, "no transfer running");
        b.set_cpu_halted(false);
        b.write(0xFF46, 0xC0); // cycle W
        b.set_cpu_halted(true);
        assert_eq!(b.ppu.oam_dma_freeze(), None, "setup delay: no OAM access");
        b.set_cpu_halted(false); // catch-up cycle: setup delay elapses
        b.tick(); // byte 0 in flight
        b.tick(); // byte 1 in flight
        b.set_cpu_halted(true);
        assert_eq!(
            b.ppu.oam_dma_freeze(),
            Some((2, 0x82)),
            "byte 2 frozen mid-access"
        );
        b.set_cpu_halted(false);
        assert_eq!(b.ppu.oam_dma_freeze(), None, "cleared on wake");
    }

    /// CGB WRAM has its own bus: a WRAM-source transfer leaves the
    /// external bus alone, and a ROM-source transfer never puts its byte
    /// on the WRAM bus — a WRAM-region read mid-transfer goes through the
    /// conflict *redirect* (same cell here: FF46 bit 4 = 0, offset 0)
    /// rather than observing the ROM byte.
    #[test]
    fn cgb_wram_is_a_separate_bus() {
        let mut b = ic(Model::Cgb);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        b.write(0xFF46, 0x00); // ROM source
        b.tick();
        b.tick();
        assert_eq!(b.read(0xC000), 0x80, "no ROM byte on the CGB WRAM bus");
        let mut b = ic(Model::Cgb);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        b.write(0xFF46, 0xC0); // WRAM source
        b.tick();
        b.tick();
        assert_eq!(b.read(0x1000), 0x5A, "ROM does not conflict with CGB WRAM");
        assert_eq!(b.read(0xC050), 0x82, "WRAM read sees DMA byte 2");
    }

    // ---- OAM DMA bus-conflict writes and CGB quirks ----------------------
    //
    // Semantics mirrored from gambatte-core memory.cpp (nontrivial_read /
    // nontrivial_write OAM-DMA conflict blocks) and calibrated against the
    // hardware-recorded gambatte/oamdma expectation matrix; per-test
    // citations name the pinning ROMs.

    /// DMG: a CPU write on pages the running transfer occupies derails
    /// into the in-flight OAM slot (pure CPU byte for a ROM source) and
    /// never reaches the addressed memory
    /// (oamdma_src0000_busypushC001_dmg08_out55AA1234: both pushed bytes
    /// land in OAM $9D/$9E, the WRAM/SRAM marker bytes survive).
    #[test]
    fn dmg_conflicted_write_lands_in_oam_slot_not_memory() {
        let mut b = ic(Model::Dmg);
        b.write_no_tick(0xC050, 0x34); // marker
        b.write(0xFF46, 0x10); // ROM source, cycle W
        b.tick(); // W+1 setup
        b.tick(); // W+2: byte 0 in flight
        // Cycle W+3: byte 1 (ROM $1001 = $5B) is in flight; the WRAM write
        // is on the conflicting external bus.
        b.write(0xC050, 0xAA);
        ticks(&mut b, 165); // run the transfer out
        assert_eq!(b.read(0xFE01), 0xAA, "CPU byte replaced DMA byte 1");
        assert_eq!(b.read(0xFE02), 0x58, "byte 2 unmolested (ROM $1002)");
        assert_eq!(b.read(0xC050), 0x34, "memory write suppressed");
    }

    /// DMG WRAM-source conflict wire-ANDs the CPU byte into the in-flight
    /// byte (oamdma_srcC000_busypushC001_dmg08_out45221234: $65&$55=$45,
    /// $76&$AA=$22).
    #[test]
    fn dmg_wram_source_write_conflict_is_wired_and() {
        let mut b = ic(Model::Dmg);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        b.write(0xFF46, 0xC0);
        b.tick();
        b.tick();
        b.write(0x4000, 0x55); // ROM page: same external bus on DMG
        ticks(&mut b, 165);
        assert_eq!(b.read(0xFE01), 0x81 & 0x55, "wired-AND of DMA and CPU byte");
    }

    /// CGB VRAM-source conflicts: a conflicted write puts $00 in the slot
    /// (oamdma_src8000_busypush8001_cgb04c_out00761234), and a conflicted
    /// read returns the in-flight byte but zeroes the OAM slot afterwards
    /// (gambatte memory.cpp nontrivial_read: `ioamhram_[oamDmaPos_] = 0`
    /// for vram sources). DMG keeps the pure CPU byte on writes
    /// (src8000_busypush8001_dmg08_out55761234).
    #[test]
    fn cgb_vram_source_conflicts_zero_oam() {
        for (model, expect_w) in [(Model::Cgb, 0x00), (Model::Dmg, 0x55)] {
            let mut b = ic(model);
            b.write(0x8000, 0x44);
            b.write(0x8001, 0x45);
            b.write(0x8002, 0x46);
            b.write(0xFF46, 0x80);
            b.tick();
            b.tick(); // byte 0 in flight
            b.write(0x9123, 0x55); // byte 1 cycle: VRAM-bus write conflict
            assert_eq!(b.read(0x9456), 0x46, "byte 2 cycle: conflicted read");
            ticks(&mut b, 162);
            assert_eq!(b.read(0xFE01), expect_w, "{model:?}: write conflict");
            let expect_r = if model.is_cgb() { 0x00 } else { 0x46 };
            assert_eq!(b.read(0xFE02), expect_r, "{model:?}: read zeroes slot");
        }
    }

    /// CGB: ROM/SRAM-source transfers conflict with the WRAM pages too,
    /// but accesses there are redirected to WRAM bank 0 / the banked page
    /// (selected by FF46 bit 4) at offset `addr & 0xFFF` — they never
    /// touch OAM (oamdma_src0000_busypopDFFF_cgb04c_out657655AA: a $DFFF
    /// read mid-transfer returns WRAM0[$FFF];
    /// oamdma_srcE000_busypushC001_cgb04c_outFFAA1255: the $C000 write
    /// lands in WRAM0[0], read back as $55 post-DMA).
    #[test]
    fn cgb_conflict_wram_access_redirects_to_ff46_bank() {
        let mut b = ic(Model::Cgb);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        b.write_no_tick(0xCFFF, 0x21);
        b.write_no_tick(0xDFFF, 0x43);
        b.write(0xFF46, 0x00); // ROM source, FF46 bit 4 = 0
        b.tick();
        b.tick();
        assert_eq!(b.read(0xDFFF), 0x21, "read redirected to WRAM0[$FFF]");
        b.write(0xD123, 0x99); // redirected to WRAM0[$123]
        ticks(&mut b, 162);
        assert_eq!(b.read(0xC123), 0x99, "write landed in WRAM bank 0");
        assert_eq!(b.read(0xD123), 0x00, "addressed cell untouched");
        assert_eq!(b.read(0xFE02), 0x00, "OAM untouched by the redirect");

        // FF46 bit 4 set: the banked page is addressed instead.
        let mut b = ic(Model::Cgb);
        b.write_no_tick(0xD456, 0x77);
        b.write(0xFF46, 0x10); // ROM source, bit 4 = 1
        b.tick();
        b.tick();
        assert_eq!(b.read(0xC456), 0x77, "read redirected to banked WRAM page");
    }

    /// CGB WRAM-source transfers conflict only with the WRAM pages, and
    /// CPU writes there are swallowed entirely
    /// (oamdma_srcC000_busypushE001_cgb04c_out65761234: markers intact,
    /// OAM untouched).
    #[test]
    fn cgb_wram_source_wram_write_swallowed() {
        let mut b = ic(Model::Cgb);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        b.write_no_tick(0xC050, 0x34);
        b.write(0xFF46, 0xC0);
        b.tick();
        b.tick();
        b.write(0xC050, 0xAA);
        ticks(&mut b, 165);
        assert_eq!(b.read(0xFE01), 0x81, "OAM untouched");
        assert_eq!(b.read(0xC050), 0x34, "write swallowed");
    }

    /// CGB: FF46 ≥ $E0 is an invalid source — the engine reads $FF
    /// (gambatte memory.cpp oamDmaSrcPtr → rdisabledRam; every
    /// srcE000/EF00/F000/FE00/FF00 cgb04c expectation shows $FF OAM
    /// bytes) while conflicting like a ROM source
    /// (srcE000_busypush8001_cgb04c_outFFAA1255). DMG keeps the WRAM echo
    /// (mooneye sources-GS, `oam_dma_high_sources_read_wram_echo`).
    #[test]
    fn cgb_high_sources_read_ff_and_conflict() {
        let mut b = ic(Model::Cgb);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        b.write(0xFF46, 0xE0);
        b.tick();
        b.tick(); // byte 0 in flight
        assert_eq!(b.read(0x4000), 0xFF, "ROM page read sees the $FF byte");
        b.write(0x4000, 0xAA); // conflicted write lands in the OAM slot
        ticks(&mut b, 162);
        assert_eq!(b.read(0xFE00), 0xFF);
        assert_eq!(b.read(0xFE02), 0xAA, "CPU byte in slot 2");
        assert_eq!(b.read(0xFE9F), 0xFF);
    }

    /// Restarting a transfer retargets the in-flight run immediately: the
    /// handover copies before the new transfer's byte 0 read from the NEW
    /// source at the old indices (gambatte memory.cpp FF46 handler updates
    /// ioamhram_[0x146] + oamDmaInitSetup before the next copy;
    /// hardware-pinned by oamdma_src8000_srcchange0000_busyread0000_1/2.
    /// mooneye oam_dma_restart restarts with the same page and cannot
    /// discriminate).
    #[test]
    fn oam_dma_restart_handover_copies_from_new_source() {
        let mut b = ic(Model::Dmg);
        fill_wram(&mut b, 0xC000, 0x80, 160); // old source
        fill_wram(&mut b, 0xD000, 0x10, 160); // new source
        b.write(0xFF46, 0xC0); // cycle W
        b.tick(); // W+1 setup
        b.tick(); // W+2 old byte 0
        b.write(0xFF46, 0xD0); // cycle W+3: old byte 1 copied, then retarget
        // Cycle W+4 (new setup): the handover copy reads the NEW source at
        // the old index 2. Observe it through the external-bus conflict.
        assert_eq!(b.read(0x0000), 0x12, "handover byte came from $D002");
        // Cycle W+5: new transfer byte 0.
        assert_eq!(b.read(0x0000), 0x10);
        ticks(&mut b, 161);
        assert_eq!(b.read(0xFE00), 0x10);
        assert_eq!(b.read(0xFE05), 0x15);
    }

    // ---- prohibited area ------------------------------------------------

    #[test]
    fn prohibited_area_dmg() {
        let mut b = ic(Model::Dmg);
        assert_eq!(b.read(0xFEA0), 0x00, "LCD off: OAM idle");
        b.write(0xFEA0, 0x55); // writes ignored
        assert_eq!(b.read(0xFEA0), 0x00);
        b.write(0xFF40, 0x91);
        // Advance into mode 3 of a steady line (the glitched enable line
        // blocks from dot 78 already, take line 1 to be safe).
        ticks(&mut b, (452 + 120) / 4);
        assert_eq!(b.read(0xFEA0), 0xFF, "OAM locked: reads $FF");
    }

    /// FEA0-FEFF on CPU CGB C (the silicon [`Model::Cgb`] pins, see
    /// ARCHITECTURE §CGB revision policy): extra OAM RAM whose low address
    /// bits 3-4 don't decode, so each of the 24 cells is mirrored 4 times
    /// across the region (Pan Docs "FEA0-FEFF range", revisions 0-D;
    /// gambatte-core memory.cpp indexes `ioamhram_[(p - 0xFE00) & 0xE7]`;
    /// pinned by gambatte oamdma_srcXXXX_busypushFEA1/FF01 cgb04c rows,
    /// whose markers written there survive a dropped mid-DMA push).
    #[test]
    fn prohibited_area_cgb_c_is_extra_ram_with_mirrors() {
        let mut b = ic(Model::Cgb);
        b.write(0xFEA0, 0x12);
        b.write(0xFEC1, 0x34);
        b.write(0xFEFF, 0x56);
        assert_eq!(b.read(0xFEA0), 0x12);
        for mirror in [0xFEA8, 0xFEB0, 0xFEB8] {
            assert_eq!(b.read(mirror), 0x12, "{mirror:04X} mirrors FEA0");
        }
        assert_eq!(b.read(0xFEC9), 0x34, "FEC9 mirrors FEC1");
        assert_eq!(b.read(0xFEF7), 0x56, "FEF7 mirrors FEFF");
        assert_eq!(b.read(0xFEA1), 0x00, "distinct cell untouched");
    }

    /// The extra RAM sits behind the same OAM gating as FE00-FE9F: $FF /
    /// dropped while a DMA byte is in flight (gambatte memory.cpp:
    /// `oamDmaPos_ < oam_size` guards both paths).
    #[test]
    fn cgb_extra_ram_blocked_during_oam_dma() {
        let mut b = ic(Model::Cgb);
        b.write(0xFEA0, 0x12);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        b.write(0xFF46, 0xC0);
        b.tick(); // setup
        b.write(0xFEA0, 0x99); // in flight: dropped
        assert_eq!(b.read(0xFEA0), 0xFF, "in flight: reads $FF");
        ticks(&mut b, 161);
        assert_eq!(b.read(0xFEA0), 0x12, "marker survived the transfer");
    }

    /// AGB (and CGB revision E) instead echo the high nibble of the low
    /// address byte twice (Pan Docs "FEA0-FEFF range").
    #[test]
    fn prohibited_area_agb_echoes_high_nibble() {
        let mut b = ic(Model::Agb);
        assert_eq!(b.read(0xFEA3), 0xAA);
        assert_eq!(b.read(0xFEB0), 0xBB);
        assert_eq!(b.read(0xFEFF), 0xFF);
    }

    // ---- CGB registers and modes ------------------------------------------

    #[test]
    fn cgb_dmg_compat_mode_disables_cgb_only_registers() {
        let mut b = ic(Model::Cgb); // DMG cart on CGB hardware
        assert!(!b.cgb_mode);
        for addr in [
            0xFF4D, 0xFF51, 0xFF55, 0xFF56, 0xFF69, 0xFF6B, 0xFF70, 0xFF74,
        ] {
            b.write(addr, 0x00);
            assert_eq!(b.read(addr), 0xFF, "{addr:04X}");
        }
        assert_eq!(b.read(0xFF4F), 0xFE, "VBK still reads bank 0");
        b.write(0xFF4F, 0x01); // locked: write ignored
        assert_eq!(b.read(0xFF4F), 0xFE);
        // FF72/73/75 exist in both modes (boot_hwio-C).
        b.write(0xFF72, 0xAB);
        assert_eq!(b.read(0xFF72), 0xAB);
        b.write(0xFF75, 0xFF);
        assert_eq!(b.read(0xFF75), 0xFF);
        b.write(0xFF75, 0x00);
        assert_eq!(b.read(0xFF75), 0x8F);
        assert_eq!(b.read(0xFF76), 0x00);
        assert_eq!(b.read(0xFF77), 0x00);
        // SVBK locked: D000 stays bank 1.
        b.write(0xC000, 1);
        b.write(0xD000, 2);
        b.write(0xFF70, 0x03);
        assert_eq!(b.read(0xD000), 2);
    }

    #[test]
    fn cgb_mode_decodes_only_header_bit7() {
        // Pan Docs "CGB flag" (0x143): the CGB boot ROM tests only bit 7,
        // so 0x84 enables CGB mode just like 0x80/0xC0 — and `auto_model`
        // must agree (shared predicate, `cartridge::cgb_flag`).
        let mut rom = test_rom();
        rom[0x143] = 0x84;
        assert_eq!(crate::GameBoy::auto_model(&rom), Model::Cgb);
        let b = Interconnect::new(Model::Cgb, Cartridge::from_bytes(rom).unwrap());
        assert!(b.cgb_mode);
    }

    #[test]
    fn cgb_mode_vbk_banks_vram() {
        let mut b = ic_cgb_mode();
        b.write(0x8000, 0x11);
        b.write(0xFF4F, 0x01);
        assert_eq!(b.read(0xFF4F), 0xFF);
        assert_eq!(b.read(0x8000), 0x00);
        b.write(0x8000, 0x22);
        b.write(0xFF4F, 0xFE); // only bit 0 matters
        assert_eq!(b.read(0x8000), 0x11);
        b.write(0xFF4F, 0x01);
        assert_eq!(b.read(0x8000), 0x22);
    }

    #[test]
    fn cgb_mode_svbk_banks_wram() {
        let mut b = ic_cgb_mode();
        assert_eq!(b.read(0xFF70), 0xF8);
        for bank in 1..8u8 {
            b.write(0xFF70, bank);
            b.write(0xD000, 0xB0 + bank);
        }
        for bank in 1..8u8 {
            b.write(0xFF70, 0xF8 | bank); // upper bits ignored
            assert_eq!(b.read(0xFF70), 0xF8 | bank);
            assert_eq!(b.read(0xD000), 0xB0 + bank, "bank {bank}");
        }
        // Bank 0 selects bank 1; C000 region is always bank 0.
        b.write(0xFF70, 0x00);
        assert_eq!(b.read(0xD000), 0xB1);
        b.write(0xC000, 0x77);
        assert_eq!(b.read(0xC000), 0x77);
        assert_eq!(b.read(0xE000), 0x77);
        // Echo of D000 region follows the bank.
        b.write(0xFF70, 0x04);
        assert_eq!(b.read(0xF000), 0xB4);
    }

    #[test]
    fn key1_speed_switch_via_stop() {
        // Register semantics only: `interrupt_pending = true` takes the
        // instantaneous-switch path (SameBoy gates the pause and the
        // skipped-byte read on !interrupt_pending), keeping the pause
        // machinery out of this test (covered separately below).
        let mut b = ic_cgb_mode();
        assert_eq!(b.read(0xFF4D), 0x7E);
        assert!(!b.stop(0x0000, true), "not armed: deep stop");
        b.write(0xFF4D, 0xFF);
        assert_eq!(b.read(0xFF4D), 0x7F);
        ticks(&mut b, 100);
        assert!(b.stop(0x0000, true), "armed: switch performed");
        assert_eq!(b.read(0xFF4D), 0xFE, "double speed, no longer armed");
        assert_eq!(b.read(0xFF04), 0x00, "STOP reset DIV");
        // Switch back.
        b.write(0xFF4D, 0x01);
        assert!(b.stop(0x0000, true));
        assert_eq!(b.read(0xFF4D), 0x7E);
    }

    /// With IE & IF pending an armed switch is instantaneous — no
    /// skipped-byte read, no pause (SameBoy sm83_cpu.c stop() gates both
    /// on !interrupt_pending; age caution/spsw-interrupts).
    #[test]
    fn speed_switch_with_pending_interrupt_takes_no_time() {
        let mut b = ic_cgb_mode();
        b.write(0xFF4D, 0x01);
        let c0 = b.cycles();
        assert!(b.stop(0x0000, true));
        assert_eq!(b.cycles() - c0, 0);
        assert_eq!(b.read(0xFF4D), 0xFE);
    }

    #[test]
    fn stop_resets_div_on_dmg() {
        let mut b = ic(Model::Dmg);
        ticks(&mut b, 100);
        assert_ne!(b.read(0xFF04), 0);
        assert!(!b.stop(0x0000, true));
        assert_eq!(b.read(0xFF04), 0);
    }

    /// STOP's skipped byte costs one real read M-cycle when no interrupt
    /// is pending (SameBoy sm83_cpu.c stop(): `cycle_read(gb, gb->pc++)`),
    /// and none when one is (1-byte-opcode path).
    #[test]
    fn stop_skipped_byte_costs_one_read_cycle() {
        let mut b = ic(Model::Dmg);
        let c0 = b.cycles();
        assert!(!b.stop(0x0000, false));
        assert_eq!(b.cycles() - c0, 4, "one read M-cycle");
        let c0 = b.cycles();
        assert!(!b.stop(0x0000, true));
        assert_eq!(b.cycles() - c0, 0, "pending interrupt: no read");
    }

    /// The STOP-triggered switch pauses the CPU while the rest of the
    /// machine runs: ~0x8000 M-cycles measured on the *new* clock
    /// (gambatte memory.cpp Memory::stop:
    /// `intreq_.setEventTime<intevent_unhalt>(cc + 0x20000 + 4)` with cc
    /// counting 4 per M-cycle at either speed — so the dot cost doubles
    /// when leaving double speed; the gambatte speedchange LY families
    /// pin that asymmetry against SameBoy's flat 0x20008 8-MHz countdown).
    #[test]
    fn speed_switch_pause_advances_machine_on_the_new_clock() {
        let mut b = ic_cgb_mode();
        b.write(0xFF4D, 0x01);
        let c0 = b.cycles();
        assert!(b.stop(0x0000, false));
        // Read + internal cycle at the old pace (4 dots each, gambatte
        // re-paces the LCD at cc + 8 when entering), pause at the new.
        assert_eq!(b.cycles() - c0, 2 * 4 + 0x7FFF * 2);
        // Switching back re-paces from the read cycle on (cc + 0).
        b.write(0xFF4D, 0x01);
        let c0 = b.cycles();
        assert!(b.stop(0x0000, false));
        assert_eq!(b.cycles() - c0, 0x8001 * 4);
    }

    /// DIV restarts from the STOP reset and TIMA keeps counting M-cycles
    /// through the pause: TAC=$04 (4096 Hz, +1 per 256 M-cycles) over
    /// 0x8001 M-cycles yields exactly 0x80 (gambatte speedchange_tima00_1a
    /// expects $80).
    #[test]
    fn speed_switch_pause_ticks_tima_from_div_reset() {
        let mut b = ic_cgb_mode();
        b.write(0xFF07, 0x04);
        b.write(0xFF4D, 0x01);
        assert!(b.stop(0x0000, false));
        assert_eq!(b.read_no_tick(0xFF05), 0x80);
    }

    /// The PPU keeps running through the pause: entering double speed
    /// costs 65542 dots = 143 lines + 334 dots (speedchange_ly44_m3_ly:
    /// LY 0x44 reads 0x39 = 0x44 + 143 mod 154 after the switch).
    #[test]
    fn speed_switch_pause_runs_the_ppu() {
        let mut b = ic_cgb_mode();
        b.write(0xFF40, 0x91);
        ticks(&mut b, 113); // glitched enable line is 452 dots: line 1 dot 0
        assert_eq!(b.read_no_tick(0xFF44), 1);
        b.write(0xFF4D, 0x01); // +4 dots (line 1 dot 4)
        assert!(b.stop(0x0000, false));
        // 65542 more dots: 143 full lines + 338 dots into line 144.
        assert_eq!(b.read_no_tick(0xFF44), 144);
    }

    /// IE & IF != 0 ends the pause early, exactly like halt mode
    /// (gambatte's pause is a halt: the halted intevent_interrupts path
    /// unhalts it).
    #[test]
    fn speed_switch_pause_cut_short_by_interrupt() {
        let mut b = ic_cgb_mode();
        b.write(0xFFFF, 0x04);
        b.write(0xFF07, 0x05); // 262144 Hz: +1 per 4 M-cycles
        b.write(0xFF05, 0xF0);
        b.write(0xFF4D, 0x01);
        let c0 = b.cycles();
        assert!(b.stop(0x0000, false));
        let elapsed_m = (b.cycles() - c0 - 8) / 2; // pause M-cycles
        assert!(elapsed_m < 0x100, "TIMA IRQ after ~64 M, got {elapsed_m}");
        assert_ne!(b.pending(), 0);
    }

    #[test]
    fn double_speed_halves_dots_per_m_cycle() {
        let mut b = ic_cgb_mode();
        b.write(0xFF4D, 0x01);
        b.stop(0x0000, true);
        let c0 = b.cycles();
        b.tick();
        assert_eq!(b.cycles() - c0, 2, "2 dots per M-cycle in double speed");
        // LY advances half as fast: a 456-dot line takes 228 M-cycles.
        b.write(0xFF40, 0x91);
        ticks(&mut b, 226); // glitched enable line is 452 dots
        assert_eq!(b.read(0xFF44), 1);
    }

    // ---- CGB VRAM DMA -----------------------------------------------------

    fn setup_gdma_regs(b: &mut Interconnect, src: u16, dst: u16) {
        b.write(0xFF51, (src >> 8) as u8);
        b.write(0xFF52, src as u8);
        b.write(0xFF53, (dst >> 8) as u8);
        b.write(0xFF54, dst as u8);
    }

    /// A GDMA write only *requests* the transfer; the copy steals the bus
    /// at the head of the CPU's next machine cycle — 8 M-cycles per block
    /// (2 bytes per M-cycle at normal speed) plus one teardown M-cycle
    /// (gambatte memory.cpp dma(): `cc += 2 + 2 * doubleSpeed` per byte,
    /// `cc += 4` at the end; see `service_vram_dma` for the seam).
    #[test]
    fn gdma_steals_the_next_machine_cycle_plus_teardown() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x40, 0x40);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        let before = b.cycles();
        b.write(0xFF55, 0x03); // 4 blocks = 64 bytes, requested
        assert_eq!(b.cycles() - before, 4, "the write cycle only flags");
        assert_eq!(b.peek(0x8000), 0x00, "nothing copied yet");
        let before = b.cycles();
        b.tick(); // the steal precedes this op's own cycle
        assert_eq!(b.cycles() - before, (4 * 8 + 1 + 1) * 4, "stall + teardown");
        assert_eq!(b.peek(0x8000), 0x40);
        assert_eq!(b.peek(0x803F), 0x7F);
        assert_eq!(b.read(0xFF55), 0xFF, "completed");
        // HDMA1-4 are write-only.
        assert_eq!(b.read(0xFF51), 0xFF);
        assert_eq!(b.read(0xFF54), 0xFF);
    }

    #[test]
    fn gdma_continues_from_incremented_addresses() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x00, 0x20);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        b.write(0xFF55, 0x00); // one block
        b.tick();
        b.write(0xFF55, 0x00); // next block continues at +0x10
        b.tick();
        assert_eq!(b.read(0x8010), 0x10);
        assert_eq!(b.read(0x801F), 0x1F);
    }

    /// FF51-FF54 write straight into the *live* DMA address counters
    /// (gambatte memory.cpp cases 0x51-0x54: `dmaSource_ = data << 8 |
    /// (dmaSource_ & 0xFF)` etc.; SameBoy's GB_IO_HDMA1-4 handlers agree):
    /// rewriting only FF51 after blocks have copied keeps the incremented
    /// low byte, so the next transfer reads from (new high byte | live low
    /// byte), not from a fresh xx00.
    #[test]
    fn hdma_partial_src_rewrite_blends_live_counter() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x00, 0x30);
        fill_wram(&mut b, 0xD030, 0xA0, 0x10);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        b.write(0xFF55, 0x02); // 3 blocks: src counter is then 0xC030
        b.tick();
        b.write(0xFF51, 0xD0); // rewrite the high byte only
        b.write(0xFF55, 0x00); // 1 block: src 0xD030.., dst continues at 0x30
        b.tick();
        assert_eq!(b.read(0x8030), 0xA0, "live low byte kept: src 0xD030");
        assert_eq!(b.read(0x803F), 0xAF);
    }

    /// VRAM and 0xE000+ are not valid VRAM-DMA sources (Pan Docs "CGB
    /// DMA"); the engine copies 0xFF instead of looping VRAM back into
    /// itself (SameBoy GB_hdma_run only drives the bus for ROM/SRAM/WRAM
    /// sources; everything else yields the idle data-bus value).
    #[test]
    fn gdma_invalid_sources_fill_destination_with_ff() {
        for src in [0x8000u16, 0x9000, 0xE000, 0xF000] {
            let mut b = ic_cgb_mode();
            // Distinct data at the would-be source and the destination.
            b.write(0x8000, 0x12);
            b.write(0x9000, 0x34);
            for i in 0..16 {
                b.write(0x9800 + i, 0x55);
            }
            setup_gdma_regs(&mut b, src, 0x1800);
            b.write(0xFF55, 0x00); // one block
            b.tick();
            for i in 0..16 {
                assert_eq!(b.read(0x9800 + i), 0xFF, "src {src:04X} byte {i}");
            }
        }
    }

    /// The destination is a full 16-bit counter: a transfer reaching
    /// 0x10000 terminates there with FF55 bit 7 latched — it does *not*
    /// wrap back into VRAM (gambatte memory.cpp dma(): `if (dmaDest +
    /// length >= 0x10000) { length = 0x10000 - dmaDest; ioamhram_[0x155]
    /// |= 0x80; }`, hardware-captured by gambatte dma/dma_dst_wrap_2;
    /// FF53 keeps the full high byte, masked only at the VRAM write).
    /// This replaces the earlier SameBoy-derived wrap-to-0x8000 model,
    /// which that capture contradicts.
    #[test]
    fn gdma_terminates_at_dest_0x10000_crossing() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x40, 0x20);
        setup_gdma_regs(&mut b, 0xC000, 0xFFF0);
        b.write(0xFF55, 0x01); // 2 blocks requested, only one fits
        b.tick();
        assert_eq!(b.peek(0x9FF0), 0x40, "dest 0xFFF0 masks to VRAM 0x1FF0");
        assert_eq!(b.peek(0x9FFF), 0x4F);
        assert_eq!(b.peek(0x8000), 0x00, "no wrap into a second block");
        // With the display off the truncated GDMA still retires its whole
        // length (gambatte dma(): `if (!(lcdc & en) && gdmaReqFlagged)
        // dmaLength = 0`), reading back $FF.
        assert_eq!(b.read(0xFF55), 0xFF);
    }

    #[test]
    fn hblank_dma_one_block_per_hblank() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x40, 0x20);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        b.write(0xFF40, 0x91); // LCD on: glitched line, hblank from ~dot 250
        b.write(0xFF55, 0x81); // hblank DMA, 2 blocks (PPU at dot 4)
        assert_eq!(b.read(0xFF55), 0x01, "2 blocks remaining reads 1");
        assert_eq!(b.peek(0x8000), 0x00, "nothing copied before hblank");
        // Run into the glitched line's hblank; the block transfer steals
        // 8 M-cycles + 1 teardown at the next boundary.
        ticks(&mut b, 90); // ~dot 400 incl. the stall
        assert_eq!(b.read(0xFF55), 0x00, "one block left");
        assert_eq!(b.peek(0x8000), 0x40);
        assert_eq!(b.peek(0x800F), 0x4F);
        assert_eq!(b.peek(0x8010), 0x00, "second block waits for next hblank");
        // Run well into line 1's hblank.
        ticks(&mut b, 100);
        assert_eq!(b.read(0xFF55), 0xFF, "done");
        assert_eq!(b.peek(0x8010), 0x50);
        assert_eq!(b.peek(0x801F), 0x5F);
    }

    /// Cancelling latches bit 7 plus the *written* length bits — the
    /// FF55 write replaces the live count before the cancel takes effect
    /// (gambatte memory.cpp case 0x55: `ioamhram_[0x155] = data & 0x7F`
    /// precedes the `|= 0x80`; SameBoy sets hdma_steps_left first, too).
    #[test]
    fn hblank_dma_cancel_sets_bit7_and_latches_written_length() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x40, 0x80);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        b.write(0xFF40, 0x91);
        b.write(0xFF55, 0x87); // 8 blocks
        ticks(&mut b, 90); // first hblank: one block done
        assert_eq!(b.read(0xFF55), 0x06);
        b.write(0xFF55, 0x02); // cancel, writing length bits 0x02
        assert_eq!(b.read(0xFF55), 0x82, "bit 7 + the written length bits");
        ticks(&mut b, 101); // into line 1's hblank
        assert_eq!(b.peek(0x8010), 0x00, "no further blocks after cancel");
    }

    /// Enabling HBlank DMA with the LCD off copies one block immediately
    /// and leaves the transfer armed (gambatte video.cpp enableHdma's
    /// LCD-off branch flags a request at once; SameBoy GB_IO_HDMA5:
    /// `(STAT & 3) == 0 && display_state != 7 → hdma_on = true`).
    #[test]
    fn hblank_enable_with_lcd_off_copies_one_block_immediately() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x40, 0x20);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        b.write(0xFF55, 0x81); // LCD is off
        b.tick();
        assert_eq!(b.peek(0x8000), 0x40);
        assert_eq!(b.peek(0x800F), 0x4F);
        assert_eq!(b.peek(0x8010), 0x00, "exactly one block");
        assert_eq!(b.read(0xFF55), 0x00, "armed, one block left");
        // The remaining block fires at the first mode-0 entry after the
        // display comes on.
        b.write(0xFF40, 0x91);
        ticks(&mut b, 90);
        assert_eq!(b.peek(0x8010), 0x50);
        assert_eq!(b.read(0xFF55), 0xFF, "completed");
    }

    /// Enabling HBlank DMA inside the hblank window fires the first block
    /// in that same hblank; within 3 dots of the line end it waits for
    /// the next one (gambatte video.cpp enableHdma →
    /// `isHdmaPeriod(...)`: `ly < 144 && cc + 3 + 3 * ds <
    /// lyCounter.time() && cc >= m0TimeOfCurrentLy`).
    #[test]
    fn hblank_enable_inside_window_fires_immediately() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x40, 0x20);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        b.write(0xFF40, 0x91);
        while !b.ppu.hblank_active() {
            b.tick();
        }
        b.write(0xFF55, 0x80); // 1 block, enabled mid-hblank
        b.tick();
        assert_eq!(b.peek(0x8000), 0x40);
        assert_eq!(b.read(0xFF55), 0xFF, "completed in the same hblank");
    }

    /// The window cutoff: in double speed (2-dot M-cycles) an enable
    /// landing 2 dots before the line end is outside the window and
    /// waits for the next hblank.
    #[test]
    fn hblank_enable_past_window_cutoff_waits() {
        let mut b = ic_cgb_mode();
        b.write(0xFF4D, 0x01);
        b.stop(0x0000, true); // double speed, instantly
        fill_wram(&mut b, 0xC000, 0x40, 0x10);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        b.write(0xFF40, 0x91);
        // Glitched enable line: 452 dots, hblank from ~dot 250. Park 2
        // dots before its end (dot 450 = 225 double-speed M-cycles).
        ticks(&mut b, 224);
        assert!(b.ppu.hblank_active(), "still in the glitch line's hblank");
        b.write(0xFF55, 0x80); // PPU at dot 450: 2 dots left < 3-dot margin
        b.tick();
        assert_eq!(b.peek(0x8000), 0x00, "no block this close to line end");
        assert_eq!(b.read(0xFF55), 0x00, "armed, nothing copied");
        // The next line's mode-0 entry fires it.
        ticks(&mut b, 250);
        assert_eq!(b.peek(0x8000), 0x40);
    }

    /// The block/CPU-access race has M-cycle granularity: a block flagged
    /// in an earlier M-cycle steals the bus at the head of the next bus
    /// operation (the racing access loses), while an access whose own
    /// tick contains the trigger still commits first (the gambatte
    /// hdma_late_destl/_wrambank/_length `_1`/`_2` adjacent-cycle pairs:
    /// shifting the same code by one cycle flips the winner).
    #[test]
    fn hblank_block_race_has_machine_cycle_granularity() {
        // Calibrate: machine cycles from arming to the trigger dot.
        let lead_ticks = {
            let mut b = ic_cgb_mode();
            fill_wram(&mut b, 0xC000, 0x40, 0x10);
            setup_gdma_regs(&mut b, 0xC000, 0x0000);
            b.write(0xFF40, 0x91);
            b.write(0xFF55, 0x80);
            let mut n = 0u32;
            while !b.ppu.hdma_trigger_level() {
                b.tick();
                n += 1;
            }
            n
        };
        // Trigger during tick N, dest write afterwards: the steal heads
        // the write — the block uses the old destination.
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x40, 0x10);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        b.write(0xFF40, 0x91);
        b.write(0xFF55, 0x80);
        ticks(&mut b, lead_ticks);
        b.write(0xFF53, 0x90);
        assert_eq!(b.peek(0x8000), 0x40, "block first: old dest");
        assert_eq!(b.peek(0x9000), 0x00);
        // Trigger inside the write's own tick: the write commits first
        // and the block uses the new destination.
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x40, 0x10);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        b.write(0xFF40, 0x91);
        b.write(0xFF55, 0x80);
        ticks(&mut b, lead_ticks - 1);
        b.write(0xFF53, 0x90); // this op's tick contains the trigger
        b.tick(); // the steal happens here
        assert_eq!(b.peek(0x9000), 0x40, "write first: new dest");
        assert_eq!(b.peek(0x8000), 0x00);
    }

    /// HBlank DMA never proceeds while the core clock is gated: a block
    /// flagged before HALT is deferred and re-flagged at wake, where it
    /// copies without the teardown M-cycle (gambatte Memory::halt →
    /// haltHdmaState_ = hdma_requested; video.h flagHdmaReq is suppressed
    /// while halted; Memory::event intevent_dma: `cc -= 4` for the
    /// deferred block).
    #[test]
    fn hblank_block_defers_while_core_clock_gated() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x40, 0x10);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        b.write(0xFF40, 0x91);
        b.write(0xFF55, 0x80);
        // Stop on the tick that flags the block (the trigger leads the
        // hblank by one dot) so the clock gate lands before any bus op
        // can service the request.
        while !b.ppu.hdma_trigger_level() {
            b.tick();
        }
        b.set_cpu_halted(true);
        ticks(&mut b, 300); // crosses further hblanks: nothing copies
        assert_eq!(b.peek(0x8000), 0x00);
        assert_eq!(b.read_no_tick(0xFF55), 0x00, "still armed");
        b.set_cpu_halted(false); // wake re-flags the deferred block
        let before = b.cycles();
        b.tick(); // the steal heads this op
        assert_eq!(b.cycles() - before, (8 + 1) * 4, "no teardown cycle");
        assert_eq!(b.peek(0x8000), 0x40);
        assert_eq!(b.read_no_tick(0xFF55), 0xFF);
    }

    /// A halt that begins *outside* the hblank window fires a block on a
    /// wake landing inside one; a halt that begins inside it does not
    /// retrigger the same hblank (gambatte haltHdmaState_ low vs high).
    #[test]
    fn halt_wake_inside_hblank_window_fires_block_once() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x40, 0x10);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        b.write(0xFF40, 0x91);
        b.write(0xFF55, 0x80);
        // Halt right after arming, before the first hblank (state Low).
        b.set_cpu_halted(true);
        while !b.ppu.hblank_active() {
            b.tick();
        }
        b.set_cpu_halted(false); // wake inside the window: block fires
        b.tick();
        assert_eq!(b.peek(0x8000), 0x40);
        // Re-arm inside the same hblank, halt, wake immediately: the halt
        // began inside the window (state High) — no retrigger.
        setup_gdma_regs(&mut b, 0xC000, 0x0010);
        assert!(b.ppu.hblank_active());
        b.write(0xFF55, 0x80);
        // (the enable itself fired a request: let it run, then re-halt)
        b.tick();
        assert_eq!(b.peek(0x8010), 0x40);
    }

    /// Disabling the display kills an armed HBlank transfer: FF55 keeps
    /// reading "active" but no further block ever copies, even after the
    /// display returns (gambatte video.cpp lcdcChange: the disable branch
    /// parks every memevent, and only an armed-while-off transfer is
    /// re-anchored by the enable branch).
    #[test]
    fn lcd_disable_kills_hblank_arming_but_not_ff55() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x40, 0x20);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        b.write(0xFF40, 0x91);
        b.write(0xFF55, 0x81); // armed with the LCD on, before any hblank
        b.write(0xFF40, 0x11); // display off
        ticks(&mut b, 300);
        assert_eq!(b.peek(0x8000), 0x00, "arming died with the display");
        assert_eq!(b.read(0xFF55), 0x01, "FF55 reads active (stale)");
        b.write(0xFF40, 0x91); // re-enabling does not revive it
        ticks(&mut b, 500);
        assert_eq!(b.peek(0x8000), 0x00);
    }

    /// The pending-block × speed-switch matrix (gambatte Memory::stop):
    /// entering double speed the request survives into the pause and the
    /// gated service aborts the transfer with the count latched; leaving
    /// double speed it is deferred and completes normally after the pause
    /// (hdma_transition_speedchange_hdmalen*_hdma5 = $80|len vs
    /// hdma_late_m3speedchange_hdma5_*_ds_1 = still active).
    #[test]
    fn speed_switch_aborts_pending_hblank_block_entering_double_speed() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x40, 0x20);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        b.write(0xFF40, 0x91);
        b.write(0xFF4D, 0x01); // arm first: any later bus op would
        b.write(0xFF55, 0x81); // service the request (2 blocks)
        while !b.ppu.hdma_trigger_level() {
            b.tick();
        }
        // The request flagged during the last tick is still pending when
        // STOP executes (gambatte: prefetched = hdmaReqFlagged).
        assert!(b.stop(0x0000, false));
        assert_eq!(b.peek(0x8000), 0x40, "the block still copied");
        assert_eq!(b.peek(0x800F), 0x4F);
        assert_eq!(b.read(0xFF55), 0x81, "aborted: bit 7 + armed count");
        ticks(&mut b, 300);
        assert_eq!(b.peek(0x8010), 0x00, "no further blocks");
    }

    #[test]
    fn speed_switch_defers_pending_hblank_block_leaving_double_speed() {
        let mut b = ic_cgb_mode();
        b.write(0xFF4D, 0x01);
        assert!(b.stop(0x0000, true)); // enter double speed instantly
        fill_wram(&mut b, 0xC000, 0x40, 0x20);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        b.write(0xFF40, 0x91);
        b.write(0xFF4D, 0x01); // arm first (see the abort test above)
        b.write(0xFF55, 0x81);
        while !b.ppu.hdma_trigger_level() {
            b.tick();
        }
        assert!(b.stop(0x0000, false)); // back to normal speed, with pause
        assert_eq!(b.read_no_tick(0xFF55), 0x01, "still active");
        assert_eq!(b.peek(0x8000), 0x00, "block deferred across the pause");
        b.tick();
        assert_eq!(b.peek(0x8000), 0x40);
        assert_eq!(b.read_no_tick(0xFF55), 0x00);
    }

    // ---- OAM DMA x VRAM DMA bus composition -------------------------------

    /// While a VRAM DMA owns the bus, a concurrently running OAM DMA keeps
    /// advancing one position per M-cycle but performs no source reads of
    /// its own: each advance latches the VRAM DMA's bus traffic instead,
    /// writing the stolen byte to OAM[hdma_src & 0xFF] — the *address* the
    /// VRAM DMA drove, not the OAM DMA's own position (gambatte-core
    /// memory.cpp `dma()`: `ioamhram_[src & 0xFF] = data` once per 4 cc,
    /// gated `cc - 3 > lOamDmaUpdate`, which at normal speed lands the
    /// advance on the *second* byte of each 2-byte stolen M-cycle —
    /// hardware-pinned by dma/hdma_transition_oamdma_1's 50 9E 52 9C and
    /// oamdma/oamdmasrcC000_hdmasrc0000's single 94 capture).
    #[test]
    fn vram_dma_steal_advances_oam_dma_capturing_the_bus() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x50, 0xA0);
        for i in 0..0xA0 {
            b.write(0xFE00 + i, 0xF0);
        }
        setup_gdma_regs(&mut b, 0x1000, 0x0000); // ROM pattern i ^ 0x5A
        b.write(0xFF46, 0xC0); // cycle W: OAM DMA from WRAM
        ticks(&mut b, 5); // W+2..W+5 copy idx 0..3
        b.write(0xFF55, 0x00); // W+6 copies idx 4, then flags 1 GDMA block
        b.tick(); // steal: 8 M-cycles (idx 5..12 advance) + teardown (idx 13)
        for _ in 0..160 {
            b.tick(); // let the transfer finish
        }
        let rom = |i: u8| i ^ 0x5A;
        // Positions copied normally before the steal, even slots: kept.
        assert_eq!(b.peek(0xFE00), 0x50);
        assert_eq!(b.peek(0xFE02), 0x52);
        assert_eq!(b.peek(0xFE04), 0x54);
        // Captures land at OAM[src & 0xFF] of the second stolen byte of
        // each M-cycle — the odd HDMA source offsets — overwriting the
        // earlier normal copies of idx 1/3.
        assert_eq!(b.peek(0xFE01), rom(0x01), "capture over earlier copy");
        assert_eq!(b.peek(0xFE03), rom(0x03), "capture over earlier copy");
        // Positions 5..12 advanced during the steal without copying their
        // own source: odd ones hold captures, even ones keep the prefill.
        assert_eq!(b.peek(0xFE05), rom(0x05));
        assert_eq!(b.peek(0xFE07), rom(0x07));
        assert_eq!(b.peek(0xFE09), rom(0x09));
        assert_eq!(b.peek(0xFE0B), rom(0x0B));
        for i in [0x06u16, 0x08, 0x0A, 0x0C] {
            assert_eq!(b.peek(0xFE00 + i), 0xF0, "idx {i:#x} skipped");
        }
        // Captures at offsets 0x0D/0x0F are overwritten again by the
        // normal copies resuming at idx 13 (teardown cycle onward).
        assert_eq!(b.peek(0xFE0D), 0x5D);
        assert_eq!(b.peek(0xFE0F), 0x5F);
        assert_eq!(b.peek(0xFE10), 0x60);
        assert_eq!(b.peek(0xFE9F), 0xEF);
    }

    /// A captured bus byte whose address low byte is ≥ 0xA0 lands in the
    /// CGB-C extra OAM RAM behind FEA0-FEFF, decoded with the same bits
    /// 3-4 alias (gambatte memory.cpp dma(): `ioamhram_[p & 0xE7] = data`
    /// for `p >= oam_size`, skipped on AGB).
    #[test]
    fn vram_dma_steal_capture_reaches_extra_oam_ram() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x50, 0xA0);
        setup_gdma_regs(&mut b, 0x10A0, 0x0000);
        b.write(0xFF46, 0xC0);
        ticks(&mut b, 5);
        b.write(0xFF55, 0x00);
        b.tick();
        for _ in 0..170 {
            b.tick(); // transfer done, OAM idle again
        }
        // Captures land at odd offsets 0xA1..0xAF; the bits-3/4 alias
        // folds 0xA9/0xAB onto the 0xA1/0xA3 cells, so the later capture
        // wins each cell.
        assert_eq!(b.read(0xFEA1), 0xA9 ^ 0x5A);
        assert_eq!(b.read(0xFEA3), 0xAB ^ 0x5A);
        assert_eq!(b.read(0xFEA9), 0xA9 ^ 0x5A, "bits 3-4 alias");
    }

    /// In double speed the VRAM DMA copies one byte per stolen M-cycle, so
    /// *every* stolen byte advances the OAM DMA and is captured (gambatte
    /// dma(): `cc += 2 + 2 * doubleSpeed` per byte vs the 4-cc advance
    /// period).
    #[test]
    fn vram_dma_steal_captures_every_byte_in_double_speed() {
        let mut b = ic_cgb_mode();
        b.write(0xFF4D, 0x01);
        assert!(b.stop(0x0000, true)); // enter double speed instantly
        fill_wram(&mut b, 0xC000, 0x50, 0xA0);
        for i in 0..0xA0 {
            b.write(0xFE00 + i, 0xF0);
        }
        setup_gdma_regs(&mut b, 0x1000, 0x0000);
        b.write(0xFF46, 0xC0);
        ticks(&mut b, 5);
        b.write(0xFF55, 0x00);
        b.tick(); // steal: 16 M-cycles, one advance + capture per byte
        for _ in 0..160 {
            b.tick();
        }
        // All 16 block offsets captured — including 0..=4, whose earlier
        // normal copies are overwritten; positions 5..=20 advanced during
        // the steal, so none of the captures is re-copied afterwards.
        for i in 0..16u16 {
            assert_eq!(b.peek(0xFE00 + i), (i as u8) ^ 0x5A, "offset {i:#x}");
        }
        // Positions 16..=20 advanced during the steal too: no capture
        // (the block only drove offsets 0..=15), no copy — prefill stays.
        for i in 16..21u16 {
            assert_eq!(b.peek(0xFE00 + i), 0xF0, "idx {i:#x} skipped");
        }
        assert_eq!(b.peek(0xFE15), 0x65, "normal copies resume at idx 21");
    }

    /// A block serviced while the core clock is gated (the speed-switch
    /// pause) advances nothing: the OAM DMA controller is frozen with the
    /// CPU (gambatte dma(): the advance is gated on `!halted()`).
    #[test]
    fn vram_dma_steal_does_not_advance_a_halt_frozen_oam_dma() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x50, 0xA0);
        for i in 0..0xA0 {
            b.write(0xFE00 + i, 0xF0);
        }
        setup_gdma_regs(&mut b, 0x1000, 0x0000);
        b.write(0xFF46, 0xC0);
        ticks(&mut b, 5); // idx 0..3 copied
        b.set_cpu_halted(true);
        b.vram_dma_req = Some(VramDmaReq::Gdma);
        b.run_vram_dma();
        assert_eq!(b.peek(0xFE01), 0x51, "no capture while frozen");
        assert_eq!(b.peek(0xFE05), 0xF0, "no position consumed");
        assert_eq!(b.dma_run.unwrap().idx, 4, "frozen position kept");
        b.set_cpu_halted(false);
        ticks(&mut b, 170);
        assert_eq!(b.peek(0xFE05), 0x55, "transfer resumed normally");
        assert_eq!(b.peek(0xFE9F), 0xEF);
    }

    /// The OAM DMA setup delay keeps counting during a steal: the start
    /// promotion happens on a stolen advance, which captures instead of
    /// copying byte 0 (gambatte dma(): `if (oamDmaPos_ == oamDmaStartPos_)
    /// startOamDma(...)` inside the steal loop).
    #[test]
    fn vram_dma_steal_counts_oam_dma_startup_delay() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x50, 0xA0);
        for i in 0..0xA0 {
            b.write(0xFE00 + i, 0xF0);
        }
        setup_gdma_regs(&mut b, 0x1000, 0x0000);
        b.write(0xFF46, 0xC0); // cycle W: delay = 1 at commit
        b.write(0xFF55, 0x00); // W+1 ticks delay to 0, then flags the GDMA
        b.tick(); // steal precedes this cycle: the start promotes inside it
        for _ in 0..170 {
            b.tick();
        }
        // Steal advance 1 (2nd stolen byte, offset 1): promote, idx 0
        // consumed by the capture at OAM[1]. Advances 2..8: idx 1..7
        // consumed, captures at offsets 3/5/7/9/B/D/F. Normal copies
        // resume at idx 8 (teardown cycle), overwriting captures 9/B/D/F.
        assert_eq!(b.peek(0xFE00), 0xF0, "byte 0's copy was stolen");
        assert_eq!(b.peek(0xFE01), 0x01 ^ 0x5A, "capture during promote");
        assert_eq!(b.peek(0xFE03), 0x03 ^ 0x5A);
        assert_eq!(b.peek(0xFE02), 0xF0, "idx 2 skipped (capture at 3)");
        assert_eq!(b.peek(0xFE07), 0x07 ^ 0x5A);
        assert_eq!(b.peek(0xFE08), 0x58, "normal copies resume at idx 8");
        assert_eq!(b.peek(0xFE09), 0x59, "capture at 9 re-copied");
    }

    // ---- peek (side-effect-free harness view) -----------------------------

    /// `peek` takes `&self`: it ticks nothing and observes raw memory —
    /// WRAM/echo, HRAM, OAM, IE — without advancing time.
    #[test]
    fn peek_reads_plain_memory_without_time() {
        let mut b = ic(Model::Dmg);
        b.write_no_tick(0xC123, 0x11);
        b.write_no_tick(0xFF80, 0x22);
        b.write_no_tick(0xFE05, 0x33);
        b.write_no_tick(0xFFFF, 0xE4);
        let cycles = b.cycles();
        assert_eq!(b.peek(0xC123), 0x11);
        assert_eq!(b.peek(0xE123), 0x11, "echo");
        assert_eq!(b.peek(0xFF80), 0x22);
        assert_eq!(b.peek(0xFE05), 0x33);
        assert_eq!(b.peek(0xFFFF), 0xE4);
        assert_eq!(b.cycles(), cycles, "no time passed");
    }

    /// `peek` is omniscient by design: it ignores the PPU's mode-based
    /// VRAM/OAM lockout that makes a real CPU read return $FF.
    #[test]
    fn peek_ignores_ppu_access_blocking() {
        let mut b = ic(Model::Dmg);
        b.write_no_tick(0x8500, 0x44);
        b.write_no_tick(0xFE00, 0x55);
        b.write(0xFF40, 0x91); // LCD on
        // Into mode 3 of the glitched first line: VRAM and OAM locked.
        ticks(&mut b, (452 + 120) / 4);
        assert_eq!(b.read(0x8500), 0xFF, "real VRAM read: locked");
        assert_eq!(b.read(0xFE00), 0xFF, "real OAM read: locked");
        assert_eq!(b.peek(0x8500), 0x44);
        assert_eq!(b.peek(0xFE00), 0x55);
    }

    /// IO registers are not peekable; the whole FF00-FF7F range (and the
    /// FEA0-FEFF prohibited area) reads $FF through `peek`.
    #[test]
    fn peek_io_reads_ff() {
        let mut b = ic(Model::Dmg);
        b.write(0xFF40, 0x91);
        assert_eq!(b.read(0xFF40), 0x91, "real IO read works");
        assert_eq!(b.peek(0xFF40), 0xFF, "peek does not");
        assert_eq!(b.peek(0xFF00), 0xFF);
        assert_eq!(b.peek(0xFF0F), 0xFF);
        assert_eq!(b.peek(0xFEA0), 0xFF);
    }

    /// `peek` follows the live VBK/SVBK banking on CGB.
    #[test]
    fn peek_follows_cgb_banking() {
        let mut b = ic_cgb_mode();
        b.write(0x8000, 0x11);
        b.write(0xFF4F, 0x01);
        b.write(0x8000, 0x22);
        assert_eq!(b.peek(0x8000), 0x22, "active VRAM bank");
        b.write(0xFF4F, 0x00);
        assert_eq!(b.peek(0x8000), 0x11);
        b.write(0xFF70, 0x03);
        b.write(0xD000, 0x33);
        b.write(0xFF70, 0x04);
        b.write(0xD000, 0x44);
        assert_eq!(b.peek(0xD000), 0x44, "active WRAM bank");
        assert_eq!(b.peek(0xF000), 0x44, "echo follows the bank");
        b.write(0xFF70, 0x03);
        assert_eq!(b.peek(0xD000), 0x33);
    }

    // ---- post-boot state ---------------------------------------------------

    fn booted(model: Model) -> Interconnect {
        let mut b = ic(model);
        b.apply_post_boot_state();
        b
    }

    /// The boot ROM leaves its logo graphics in VRAM at hand-off: the
    /// header logo decompressed into tiles $01-$18 (even bytes — one
    /// bitplane), the (R) trademark tile at $19, and on DMG-family models
    /// the two logo tile-map rows (gambatte initstate.cpp setInitialVram
    /// hardware dump; the expected bytes below are that dump's prefix for
    /// the standard Nintendo logo). mealybug m3_scx_low_3_bits renders
    /// the leftover (R) tile.
    #[test]
    fn post_boot_vram_boot_logo_leftovers() {
        // The fixed logo applies regardless of the cart header (the boot
        // ROM locks up on a mismatch, so hardware VRAM only ever holds
        // the canonical image; gambatte's test carts have no header logo
        // and their references still show it).
        for model in [Model::Dmg, Model::Cgb] {
            let mut b = ic(model);
            b.apply_post_boot_state();
            // $CE -> F0 F0 FC FC, $ED -> FC FC F3 F3 (even bytes).
            for (off, want) in [
                (0x00u16, 0xF0u8),
                (0x02, 0xF0),
                (0x04, 0xFC),
                (0x06, 0xFC),
                (0x08, 0xFC),
                (0x0A, 0xFC),
                (0x0C, 0xF3),
                (0x0E, 0xF3),
                // $66 -> 3C 3C 3C 3C twice.
                (0x10, 0x3C),
                (0x16, 0x3C),
                (0x18, 0x3C),
                (0x1E, 0x3C),
            ] {
                assert_eq!(
                    b.ppu().vram_read_raw(0x8010 + off),
                    want,
                    "{model:?} +{off:#x}"
                );
            }
            assert_eq!(b.ppu().vram_read_raw(0x8011), 0, "high bitplane untouched");
            // (R) trademark tile $19.
            assert_eq!(b.ppu().vram_read_raw(0x8190), 0x3C, "{model:?}");
            assert_eq!(b.ppu().vram_read_raw(0x8192), 0x42, "{model:?}");
            assert_eq!(b.ppu().vram_read_raw(0x8194), 0xB9, "{model:?}");
            assert_eq!(b.ppu().vram_read_raw(0x819E), 0x3C, "{model:?}");
            // The logo tile-map rows are deliberately not installed
            // (see install_boot_logo_vram): the pinned gambatte
            // reference PNGs encode a cleared map.
            assert_eq!(b.ppu().vram_read_raw(0x9904), 0x00, "{model:?}");
            assert_eq!(b.ppu().vram_read_raw(0x9910), 0x00, "{model:?}");
        }
    }

    /// Real DMG-family WRAM powers up in the $00/$FF half-page stripe
    /// pattern, mirrored into D000-DFFF (gambatte-core mem_dumps.h
    /// `setInitialDmgWram` base pattern; see `install_power_on_wram`).
    /// The $DE00 page reading $FF is what the gambatte oamdma_srcFE00_*
    /// expectations encode (OAM DMA from $FE00 reads the $DE00 echo).
    /// CGB WRAM stays zero-filled.
    #[test]
    fn post_boot_wram_power_on_pattern() {
        for model in [Model::Dmg0, Model::Dmg, Model::Mgb, Model::Sgb, Model::Sgb2] {
            let b = booted(model);
            for (addr, want) in [
                (0xC000u16, 0x00u8),
                (0xC0FF, 0x00),
                (0xC100, 0xFF),
                (0xC1FF, 0xFF),
                (0xC2A0, 0x00),
                (0xC700, 0xFF),
                // Polarity inverts across the 2 KiB half...
                (0xC800, 0xFF),
                (0xC900, 0x00),
                (0xCE42, 0xFF),
                (0xCF00, 0x00),
                // ...and D000-DFFF mirrors C000-CFFF.
                (0xD000, 0x00),
                (0xD100, 0xFF),
                (0xDE00, 0xFF),
                (0xDEFF, 0xFF),
                (0xDF00, 0x00),
            ] {
                assert_eq!(b.peek(addr), want, "{model:?} {addr:04X}");
            }
        }
        let b = booted(Model::Cgb);
        for addr in [0xC100u16, 0xC800, 0xDE00] {
            assert_eq!(b.peek(addr), 0x00, "CGB WRAM zero-filled at {addr:04X}");
        }
    }

    /// The CGB boot ROM hands a CGB-flagged cart off 0x7D8 T-cycles
    /// earlier than a DMG cart (the DMG-compat palette tail), shifting
    /// DIV and the LCD phase together: DIV $1E9C pinned by gambatte
    /// div/start_inc_1/2 (FF04 reads $1E at +96 T immediately before
    /// the increment to $1F00) and tima/tc00_start_1/2 (first TIMA
    /// increment, DIV bit-9 edge, exactly between rounds at +356), LY
    /// $90 by display_startstate ly/stat. The DMG-cart side keeps
    /// mooneye misc/boot_div-cgbABCDE's $2674 with the LCD 0x7D8 dots
    /// further on (line 148, still in the pandocs#426 LY window).
    #[test]
    fn post_boot_cgb_cart_hands_off_earlier_than_dmg_cart() {
        let mut dmg_cart = booted(Model::Cgb);
        assert_eq!(dmg_cart.timer.div_counter(), 0x2674);
        assert_eq!(dmg_cart.read(0xFF44), 148);

        let mut cgb_cart = ic_cgb_mode();
        cgb_cart.apply_post_boot_state();
        let div = cgb_cart.timer.div_counter();
        assert_eq!(div, 0x1E9C);
        assert_eq!(div, 0x2674 - 0x7D8);
        // div/start_inc oracle: the read 24 M-cycles in.
        assert_eq!((div + 96) >> 8, 0x1E, "round 1 high byte");
        assert!(
            (div + 96) & 0xFF >= 0xFC,
            "immediately before the increment"
        );
        assert_eq!((div + 100) >> 8, 0x1F, "round 2 high byte");
        // tc00_start oracle: bit-9 falling edge between the rounds.
        assert_eq!((div + 356) % 0x400, 0);
        assert_eq!(cgb_cart.read(0xFF44), 144);
    }

    #[test]
    fn post_boot_io_dmg() {
        let mut b = booted(Model::Dmg);
        assert_eq!(b.read(0xFF00), 0xCF);
        assert_eq!(b.read(0xFF02), 0x7E);
        assert_eq!(b.read(0xFF0F), 0xE1);
        assert_eq!(b.read(0xFF26), 0xF1, "channel 1 beep still on");
        assert_eq!(b.read(0xFF11), 0xBF);
        assert_eq!(b.read(0xFF12), 0xF3);
        assert_eq!(b.read(0xFF24), 0x77);
        assert_eq!(b.read(0xFF25), 0xF3);
        assert_eq!(b.read(0xFF40), 0x91);
        assert_eq!(b.read(0xFF47), 0xFC);
        assert_eq!(b.read(0xFF46), 0xFF);
        assert_eq!(b.read(0xFFFF), 0x00);
    }

    #[test]
    fn post_boot_io_sgb() {
        let mut b = booted(Model::Sgb);
        assert_eq!(b.read(0xFF00), 0xFF, "P1 columns deselected on SGB");
        assert_eq!(b.read(0xFF26), 0xF0, "no boot beep on SGB");
    }

    #[test]
    fn post_boot_io_cgb_dmg_cart() {
        let mut b = booted(Model::Cgb);
        assert_eq!(b.read(0xFF00), 0xFF);
        assert_eq!(b.read(0xFF02), 0x7E, "fast-clock bit absent in DMG mode");
        assert_eq!(b.read(0xFF26), 0xF1);
        assert_eq!(b.read(0xFF46), 0x00);
        assert_eq!(b.read(0xFF4D), 0xFF);
        assert_eq!(b.read(0xFF4F), 0xFE);
        assert_eq!(b.read(0xFF55), 0xFF);
        assert_eq!(b.read(0xFF68), 0xC8, "BCPS boot leftover");
        assert_eq!(b.read(0xFF69), 0xFF, "BCPD unreadable in DMG mode");
        assert_eq!(b.read(0xFF6A), 0xD0, "OCPS boot leftover");
        assert_eq!(b.read(0xFF6C), 0xFF, "OPRI = DMG-style priority");
        assert_eq!(b.read(0xFF70), 0xFF);
        assert_eq!(b.read(0xFF74), 0xFF);
        assert_eq!(b.read(0xFF75), 0x8F);
    }

    /// For DMG carts whose licensee is not Nintendo (no title-hash lookup),
    /// the CGB boot ROM installs the *default* compatibility palette
    /// combination — BG palette 0 = $7FFF/$1BEF/$6180/$0000, OBJ palettes 0
    /// and 1 = $7FFF/$421F/$1CF2/$0000 (Pan Docs "Compatibility palettes";
    /// SameBoy BootROMs/cgb_boot.asm default combination OBJ0=4, OBJ1=4,
    /// BG=29). Pins that the BG table differs from the OBJ table and that
    /// *both* OBJ slots receive it.
    #[test]
    fn post_boot_cgb_compat_palettes_are_boot_defaults() {
        fn le_bytes(table: [u16; 4]) -> [u8; 8] {
            let mut out = [0u8; 8];
            for (i, c) in table.into_iter().enumerate() {
                [out[2 * i], out[2 * i + 1]] = c.to_le_bytes();
            }
            out
        }
        for model in [Model::Cgb, Model::Agb] {
            let b = booted(model);
            let (bg, obj) = b.ppu.palette_ram();
            assert_eq!(
                bg[..8],
                le_bytes([0x7FFF, 0x1BEF, 0x6180, 0x0000]),
                "{model:?} BG palette 0"
            );
            let obj_table = le_bytes([0x7FFF, 0x421F, 0x1CF2, 0x0000]);
            assert_eq!(obj[..8], obj_table, "{model:?} OBJ palette 0");
            assert_eq!(obj[8..16], obj_table, "{model:?} OBJ palette 1");
        }
    }

    #[test]
    fn post_boot_io_cgb_mode_cart() {
        let mut rom = test_rom();
        rom[0x143] = 0x80;
        let mut b = Interconnect::new(Model::Cgb, Cartridge::from_bytes(rom).unwrap());
        b.apply_post_boot_state();
        assert_eq!(b.read(0xFF4D), 0x7E);
        assert_eq!(b.read(0xFF02), 0x7C, "CGB-mode SC has the fast-clock bit");
        assert_eq!(b.read(0xFF6C), 0xFE, "OPRI = OAM index priority");
        assert_eq!(b.read(0xFF70), 0xF8);
        assert_eq!(b.read(0xFF56), 0x3E, "RP idle, not receiving");
    }

    /// Replicate acceptance/boot_div-dmgABCmgb: DIV reads at M-cycles 14,
    /// 78, 141, 205, 269 and 334 after hand-off observe AC AD AD AE AF B1.
    #[test]
    fn post_boot_div_phase_dmg() {
        let mut b = booted(Model::Dmg);
        let mut cycle = 0u32;
        let mut read_at = |b: &mut Interconnect, m: u32| {
            while cycle + 1 < m {
                b.tick();
                cycle += 1;
            }
            cycle += 1;
            b.read(0xFF04)
        };
        let got = [14, 78, 141, 205, 269, 334].map(|m| read_at(&mut b, m));
        assert_eq!(got, [0xAC, 0xAD, 0xAD, 0xAE, 0xAF, 0xB1]);
    }

    /// SGB DIV depends on the header bits: an all-zero header yields 731
    /// zero bits in the transferred packets -> DIV base + 4*731.
    #[test]
    fn post_boot_div_sgb_header_dependence() {
        let mut b = booted(Model::Sgb);
        // test_rom() header region 0x104-0x14F is all zeros: payload zeros =
        // 6 * 15 * 8 = 720, command bytes F1/F3/F5/F7/F9/FB add 11.
        assert_eq!(sgb_header_zero_bits(b.cartridge()), 731);
        // div = 0xD170 + 4 * 731 = 0xDCDC; the first read observes +4.
        assert_eq!(b.read(0xFF04), 0xDC);
    }

    /// Replicate the LY/STAT bytes of boot_hwio-dmgABCmgb: STAT read at
    /// M-cycle 1139 is $80 (mode 0, line 9), LY read at 1190 is $0A.
    #[test]
    fn post_boot_lcd_phase_dmg() {
        let mut b = booted(Model::Dmg);
        ticks(&mut b, 1138);
        assert_eq!(b.read(0xFF41), 0x80);
        let mut b = booted(Model::Dmg);
        ticks(&mut b, 1189);
        assert_eq!(b.read(0xFF44), 0x0A);
    }

    /// boot_hwio-dmg0: STAT $83 (mode 3, line 1), LY $01.
    #[test]
    fn post_boot_lcd_phase_dmg0() {
        let mut b = booted(Model::Dmg0);
        ticks(&mut b, 1138);
        assert_eq!(b.read(0xFF41), 0x83);
        let mut b = booted(Model::Dmg0);
        ticks(&mut b, 1189);
        assert_eq!(b.read(0xFF44), 0x01);
    }

    /// The IF value survives until boot_hwio's read at M-cycle 285 (no
    /// stray STAT/vblank bits from the warmed-up PPU).
    #[test]
    fn post_boot_if_stable() {
        for model in [Model::Dmg0, Model::Dmg, Model::Sgb, Model::Cgb] {
            let mut b = booted(model);
            ticks(&mut b, 284);
            assert_eq!(b.read(0xFF0F), 0xE1, "{model:?}");
        }
    }

    // ---- DMG OAM corruption bug (Pan Docs "OAM Corruption Bug") ------

    /// Interconnect with the LCD freshly enabled (`ic` powers on with the
    /// LCD off; the enable glitch line passes before any scan window).
    fn ic_lcd_on(model: Model) -> Interconnect {
        let mut b = ic(model);
        b.write(0xFF40, 0x91);
        b
    }

    /// Distinct OAM fill through the DMA-engine path (ignores blocking,
    /// takes no machine time).
    fn fill_oam_distinct(b: &mut Interconnect) {
        for i in 0..0xA0u8 {
            b.ppu_mut().oam_dma_write(i, i ^ 0xA5);
        }
    }

    fn oam_snapshot(b: &Interconnect) -> [u8; 0xA0] {
        let mut snap = [0u8; 0xA0];
        for (i, byte) in snap.iter_mut().enumerate() {
            *byte = b.peek(0xFE00 + i as u16);
        }
        snap
    }

    /// Tick until the *next* M-cycle's access lands on scan row `row`
    /// (every access advances the machine one M-cycle first, so park one
    /// row short).
    fn park_before_oam_row(b: &mut Interconnect, row: u8) {
        assert!((0x10..=0x98).contains(&row) && row % 8 == 0);
        for _ in 0..200_000 {
            if b.ppu.oam_bug_row() == Some(row - 8) {
                return;
            }
            b.tick();
        }
        panic!("scan row {row:#04x} never reached");
    }

    #[test]
    fn oam_bug_read_in_mode2_corrupts_on_dmg_family_only() {
        for model in [Model::Dmg, Model::Dmg0, Model::Mgb, Model::Sgb, Model::Sgb2] {
            let mut b = ic_lcd_on(model);
            park_before_oam_row(&mut b, 0x20);
            fill_oam_distinct(&mut b);
            let before = oam_snapshot(&b);
            assert_eq!(b.read(0xFE00), 0xFF, "{model:?}: OAM still locked");
            let after = oam_snapshot(&b);
            // Read pattern at row 0x20: glitched word in rows 3 *and* 4,
            // row tail copied from row 3.
            let glitched = before[0x18] | (before[0x20] & before[0x1C]);
            assert_eq!(after[0x20], glitched, "{model:?}");
            assert_eq!(after[0x18], glitched, "{model:?}");
            assert_eq!(after[0x22..0x28], before[0x1A..0x20], "{model:?}");
            assert_eq!(after[..0x18], before[..0x18], "{model:?}: earlier rows");
        }
        for model in [Model::Cgb, Model::Agb] {
            let mut b = ic_lcd_on(model);
            park_before_oam_row(&mut b, 0x20);
            fill_oam_distinct(&mut b);
            let before = oam_snapshot(&b);
            b.read(0xFE00);
            assert_eq!(oam_snapshot(&b), before, "{model:?}: no bug on CGB");
        }
    }

    #[test]
    fn oam_bug_triggers_across_the_whole_fexx_page_only() {
        // The trigger keys on the address byte $FE on the bus: the
        // FEA0-FEFF prohibited area corrupts like OAM proper (blargg
        // oam_bug/8-instr_effect pops from $FEF0), neighbours do not.
        let mut b = ic_lcd_on(Model::Dmg);
        park_before_oam_row(&mut b, 0x20);
        fill_oam_distinct(&mut b);
        let before = oam_snapshot(&b);
        b.read(0xFEA0);
        assert_ne!(oam_snapshot(&b), before, "prohibited-area read corrupts");
        for addr in [0xFDFF, 0xFF00] {
            let mut b = ic_lcd_on(Model::Dmg);
            park_before_oam_row(&mut b, 0x20);
            fill_oam_distinct(&mut b);
            let before = oam_snapshot(&b);
            b.read(addr);
            assert_eq!(oam_snapshot(&b), before, "read {addr:#06x} is inert");
        }
    }

    #[test]
    fn oam_bug_write_corrupts_with_write_pattern_and_is_dropped() {
        let mut b = ic_lcd_on(Model::Dmg);
        park_before_oam_row(&mut b, 0x20);
        fill_oam_distinct(&mut b);
        let before = oam_snapshot(&b);
        b.write(0xFE21, 0x77);
        let after = oam_snapshot(&b);
        for i in 0..2 {
            let (a, p0, p2) = (before[0x20 + i], before[0x18 + i], before[0x1C + i]);
            assert_eq!(after[0x20 + i], ((a ^ p2) & (p0 ^ p2)) ^ p2, "byte {i}");
        }
        assert_eq!(after[0x22..0x28], before[0x1A..0x20], "row tail copied");
        assert!(
            !after.contains(&0x77),
            "the blocked CPU write must not land"
        );
    }

    #[test]
    fn oam_bug_internal_cycle_value_corrupts_via_tick_addr() {
        // INC rr's internal cycle carries no memory access; the register
        // value alone triggers the write pattern (blargg oam_bug/2-causes).
        let mut b = ic_lcd_on(Model::Dmg);
        park_before_oam_row(&mut b, 0x20);
        fill_oam_distinct(&mut b);
        let before = oam_snapshot(&b);
        Bus::tick_addr(&mut b, 0xFE00);
        let after = oam_snapshot(&b);
        for i in 0..2 {
            let (a, p0, p2) = (before[0x20 + i], before[0x18 + i], before[0x1C + i]);
            assert_eq!(after[0x20 + i], ((a ^ p2) & (p0 ^ p2)) ^ p2, "byte {i}");
        }
        assert_eq!(after[0x22..0x28], before[0x1A..0x20]);
        // Out-of-range values are inert (blargg oam_bug/3-non_causes).
        let mut b = ic_lcd_on(Model::Dmg);
        park_before_oam_row(&mut b, 0x20);
        fill_oam_distinct(&mut b);
        let before = oam_snapshot(&b);
        Bus::tick_addr(&mut b, 0xFDFF);
        Bus::tick_addr(&mut b, 0xFF00);
        assert_eq!(oam_snapshot(&b), before);
    }

    #[test]
    fn oam_bug_increase_read_uses_the_read_increase_pattern() {
        // POP/LD A,(HL+) style reads: special pattern at rows 4..=18
        // (SameBoy v0.12.1 GB_trigger_oam_bug_read_increase).
        let mut b = ic_lcd_on(Model::Dmg);
        park_before_oam_row(&mut b, 0x20);
        fill_oam_distinct(&mut b);
        let before = oam_snapshot(&b);
        assert_eq!(Bus::read_inc(&mut b, 0xFE05), 0xFF);
        let after = oam_snapshot(&b);
        let mut prev = [0u8; 8];
        prev.copy_from_slice(&before[0x18..0x20]);
        for i in 0..2 {
            let (a, p0, c, d) = (
                before[0x10 + i],
                before[0x18 + i],
                before[0x20 + i],
                before[0x1C + i],
            );
            prev[i] = (p0 & (a | c | d)) | (a & c & d);
        }
        for i in 0..8 {
            assert_eq!(after[0x10 + i], prev[i], "two rows back {i}");
            assert_eq!(after[0x18 + i], prev[i], "preceding row {i}");
            assert_eq!(after[0x20 + i], prev[i], "current row {i}");
        }
    }

    #[test]
    fn oam_bug_suppressed_while_the_core_clock_is_gated() {
        // The halted CPU performs no bus accesses on hardware; the
        // discarded halt prefetch (see cpu::Bus docs) must stay
        // side-effect-free even with PC in $FExx.
        let mut b = ic_lcd_on(Model::Dmg);
        park_before_oam_row(&mut b, 0x20);
        fill_oam_distinct(&mut b);
        let before = oam_snapshot(&b);
        b.set_cpu_halted(true);
        b.read(0xFE00);
        assert_eq!(oam_snapshot(&b), before, "halted: no corruption");
        b.set_cpu_halted(false);
        park_before_oam_row(&mut b, 0x20);
        b.read(0xFE00);
        assert_ne!(oam_snapshot(&b), before, "running again: corruption");
    }

    #[test]
    fn oam_bug_suppressed_while_oam_dma_copies() {
        // While the DMA engine owns OAM, CPU-side $FExx traffic does not
        // corrupt (the interplay is untested on hardware — SameBoy leaves
        // the same Todo — so the conservative gate wins). The DMA source
        // mirrors the OAM contents so the copy itself is invisible.
        let mut b = ic_lcd_on(Model::Dmg);
        for i in 0..0xA0u16 {
            b.write(0xC000 + i, (i as u8) ^ 0xA5);
        }
        park_before_oam_row(&mut b, 0x10);
        fill_oam_distinct(&mut b);
        let before = oam_snapshot(&b);
        b.write(0xFF46, 0xC0);
        b.tick(); // setup delay
        b.tick(); // first byte copies; the engine owns OAM from here
        b.read(0xFE00); // still inside the scan window (row 0x28)
        assert_eq!(oam_snapshot(&b), before);
    }

    #[test]
    fn oam_bug_inert_outside_the_scan_window() {
        // blargg oam_bug/6-timing_no_bug: accesses bracketing the per-line
        // window, hammering vblank, and with the LCD off are all clean.
        let access_all = |b: &mut Interconnect| {
            let keep = b.peek(0xFE00);
            b.read(0xFE00);
            Bus::tick_addr(b, 0xFE00);
            Bus::read_inc(b, 0xFE00);
            b.write(0xFE00, keep); // may land outside mode 2/3: same value
        };
        // VBlank.
        let mut b = ic_lcd_on(Model::Dmg);
        while b.ppu.mode_bits() != 1 {
            b.tick();
        }
        fill_oam_distinct(&mut b);
        let before = oam_snapshot(&b);
        access_all(&mut b);
        assert_eq!(oam_snapshot(&b), before, "vblank");
        // Mode 3 (entered fresh, lasts >= 43 M-cycles).
        let mut b = ic_lcd_on(Model::Dmg);
        while b.ppu.mode_bits() != 2 {
            b.tick();
        }
        while b.ppu.mode_bits() != 3 {
            b.tick();
        }
        fill_oam_distinct(&mut b);
        let before = oam_snapshot(&b);
        access_all(&mut b);
        assert_eq!(oam_snapshot(&b), before, "mode 3");
        // HBlank right after that mode 3.
        while b.ppu.mode_bits() != 0 {
            b.tick();
        }
        fill_oam_distinct(&mut b);
        let before = oam_snapshot(&b);
        access_all(&mut b);
        assert_eq!(oam_snapshot(&b), before, "hblank");
        // LCD off.
        let mut b = ic_lcd_on(Model::Dmg);
        b.write(0xFF40, 0x00);
        fill_oam_distinct(&mut b);
        let before = oam_snapshot(&b);
        access_all(&mut b);
        assert_eq!(oam_snapshot(&b), before, "LCD off");
    }
}

#[cfg(test)]
mod pcm_decay_probe {
    use super::*;
    use crate::cartridge::Cartridge;
    use crate::cpu::Bus;

    #[test]
    fn post_boot_beep_already_decayed_at_handoff() {
        // The CGB boot beep plays during the logo, ~0.7s before hand-off;
        // its NR12=$F3 envelope is at volume 0 by PC=0x100. NR52 keeps the
        // channel-1 status bit (enable != volume), but PCM12 reads $00
        // (oracle: misc/boot_hwio-C, misc/bits/unused_hwio-C).
        let mut rom = vec![0u8; 0x8000];
        rom[0x143] = 0x80;
        rom[0x147] = 0x00;
        let mut ic = Interconnect::new(Model::Cgb, Cartridge::from_bytes(rom).unwrap());
        ic.apply_post_boot_state();
        assert_eq!(
            ic.read_no_tick(0xFF76),
            0,
            "beep already silent at hand-off"
        );
        assert_eq!(ic.read_no_tick(0xFF26) & 0x01, 0x01, "ch1 still enabled");
        for _ in 0..1_048_576 {
            ic.tick();
        }
        assert_eq!(ic.read_no_tick(0xFF76), 0, "stays silent");
    }
}
