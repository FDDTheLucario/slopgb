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
use crate::ppu::Ppu;
use crate::serial::Serial;
use crate::timer::Timer;

/// The buses OAM DMA can occupy. While the DMA engine reads a byte from one
/// of these, a CPU read of any address on the *same* bus returns the DMA's
/// byte instead (gbctr "OAM DMA": the DMA controller drives the bus).
/// On DMG, WRAM sits on the external bus; on CGB it has its own bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DmaBus {
    External,
    Vram,
    Wram,
}

/// An OAM DMA transfer in progress: `idx` is the next byte to copy (one per
/// M-cycle).
struct OamDmaRun {
    src: u16,
    idx: u8,
}

/// A freshly written FF46 value waiting out its 1 M-cycle setup delay
/// (acceptance/oam_dma_start: the cycle after the write still reads OAM).
struct OamDmaStart {
    src: u16,
    delay: u8,
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

    /// FF46 readback is simply the last written value
    /// (acceptance/oam_dma/reg_read).
    dma_reg: u8,
    dma_run: Option<OamDmaRun>,
    dma_start: Option<OamDmaStart>,
    /// Bus occupied + byte transferred during the current M-cycle, if a DMA
    /// byte was copied this cycle.
    dma_conflict: Option<(DmaBus, u8)>,

    // CGB VRAM DMA (FF51-FF55).
    /// Source address as assembled from HDMA1/2 (low 4 bits always 0).
    hdma_src: u16,
    /// Destination offset into VRAM (13 bits).
    hdma_dst: u16,
    /// HBlank DMA in progress.
    hdma_active: bool,
    hdma_blocks_left: u8,
    /// FF55 readback when no HBlank DMA is active ($FF = completed/never,
    /// $80|n = cancelled with n blocks remaining).
    hdma_latch: u8,
    /// Previous `hblank_active` level for the HBlank DMA edge detector.
    hdma_prev_hblank: bool,
    /// Re-entrancy guard: a VRAM DMA block is stalling the CPU and ticking
    /// the machine internally.
    vram_dma_stall: bool,

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

/// DMG-compat palette installed by the CGB boot ROM for DMG carts. We use a
/// neutral grayscale (RGB555 white/light/dark/black); the hardware default
/// depends on a title-hash lookup that is not modelled.
const CGB_COMPAT_PALETTE: [u16; 4] = [0x7FFF, 0x5294, 0x294A, 0x0000];

impl Interconnect {
    pub fn new(model: Model, cart: Cartridge) -> Self {
        // CGB mode iff the hardware is a CGB/AGB *and* the cart opts in via
        // header byte 0x143 bit 7 (same flag `GameBoy::auto_model` uses).
        let cgb_mode = model.is_cgb() && cart.read_rom(0x143) & 0x80 != 0;
        let mut ppu = Ppu::new(model);
        ppu.set_dmg_compat(model.is_cgb() && !cgb_mode);
        Self {
            model,
            ppu,
            apu: Apu::new(model.is_cgb()),
            timer: Timer::new(),
            // The serial fast-clock bit (SC bit 1) exists in CGB mode only;
            // in DMG compatibility mode SC reads $7E (misc/boot_hwio-C).
            serial: Serial::new(cgb_mode),
            joypad: Joypad::new(),
            cycles: 0,
            cgb_mode,
            double_speed: false,
            key1_armed: false,
            wram: vec![0; if model.is_cgb() { 0x8000 } else { 0x2000 }],
            svbk: 0,
            hram: [0; 0x7F],
            intf: 0,
            ie: 0,
            dma_reg: 0,
            dma_run: None,
            dma_start: None,
            dma_conflict: None,
            hdma_src: 0,
            hdma_dst: 0,
            hdma_active: false,
            hdma_blocks_left: 0,
            hdma_latch: 0xFF,
            hdma_prev_hblank: false,
            vram_dma_stall: false,
            rp: 0,
            ff72: 0,
            ff73: 0,
            ff74: 0,
            ff75: 0,
            cart,
        }
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

        // LCD warmup: glitched enable line (452 dots) + 153 normal lines
        // brings the PPU to line 0 dot 0; then advance to the hand-off
        // phase.
        self.ppu.write(0xFF40, 0x91);
        for _ in 0..(70224 - 4 + s.lcd_phase_dots) {
            self.ppu.tick();
        }
        self.ppu.consume_pending_irq();

        if self.model.is_cgb() {
            // Compat palette: BG palette 0 (8 bytes) leaves BCPS = $88,
            // OBJ palettes 0+1 (16 bytes) leave OCPS = $90 — boot_hwio-C
            // reads $C8/$D0.
            self.ppu.write(0xFF68, 0x80);
            for c in CGB_COMPAT_PALETTE {
                self.ppu.write(0xFF69, c as u8);
                self.ppu.write(0xFF69, (c >> 8) as u8);
            }
            self.ppu.write(0xFF6A, 0x80);
            for _ in 0..2 {
                for c in CGB_COMPAT_PALETTE {
                    self.ppu.write(0xFF6B, c as u8);
                    self.ppu.write(0xFF6B, (c >> 8) as u8);
                }
            }
            // OPRI: DMG-compat mode uses DMG-style X priority (FF6C reads
            // $FF), CGB mode uses OAM-index priority ($FE).
            self.ppu.write(0xFF6C, u8::from(!self.cgb_mode));
            self.ppu.consume_pending_irq();
        }

        for &(addr, value) in s.hwio {
            if addr == 0xFF46 {
                self.dma_reg = value;
            } else {
                self.write_no_tick(addr, value);
            }
        }

        // SGB boot duration depends on the cartridge header: the boot ROM
        // sends it to the SNES bit by bit, and a zero bit costs one M-cycle
        // more than a one bit (boot_div-S vs boot_div2-S).
        let div = if matches!(self.model, Model::Sgb | Model::Sgb2) {
            s.div_counter
                .wrapping_add((4 * sgb_header_zero_bits(&self.cart)) as u16)
        } else {
            s.div_counter
        };
        self.timer.set_div(div);
        self.serial.tick(div);
        self.apu.tick(div, false);
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

    /// Advance the whole machine by one CPU M-cycle (docs/ARCHITECTURE.md
    /// §Timing: timer, OAM DMA engine, PPU dots, VRAM DMA, APU, serial,
    /// joypad; IF bits OR-ed in as produced).
    fn tick_machine(&mut self) {
        let dots: u64 = if self.double_speed { 2 } else { 4 };
        self.cycles += dots;
        self.intf |= self.timer.tick() & 0x1F;
        self.oam_dma_tick();
        for _ in 0..dots {
            self.intf |= self.ppu.tick() & 0x1F;
        }
        self.hblank_dma_check();
        let div = self.timer.div_counter();
        self.apu.tick(div, self.double_speed);
        self.intf |= self.serial.tick(div) & 0x1F;
        self.intf |= self.joypad.take_irq() & 0x1F;
        // RTC wall time is dot time (2 dots per M-cycle in double speed).
        self.cart.tick_rtc(dots as u32);
    }

    // ---- OAM DMA engine ------------------------------------------------

    fn oam_dma_tick(&mut self) {
        self.dma_conflict = None;
        // Promote a pending start whose setup delay has elapsed. The old
        // transfer (if any) keeps copying during the delay cycle
        // (acceptance/oam_dma_restart) and is replaced exactly when the new
        // one copies its first byte.
        match &mut self.dma_start {
            Some(s) if s.delay == 0 => {
                let src = s.src;
                self.dma_start = None;
                self.dma_run = Some(OamDmaRun { src, idx: 0 });
            }
            Some(s) => s.delay -= 1,
            None => {}
        }
        if let Some(run) = &self.dma_run {
            let (bus, byte) = self.oam_dma_source_read(run.src.wrapping_add(run.idx.into()));
            let idx = run.idx;
            self.ppu.oam_dma_write(idx, byte);
            self.dma_conflict = Some((bus, byte));
            match &mut self.dma_run {
                Some(run) if run.idx == 159 => self.dma_run = None,
                Some(run) => run.idx += 1,
                None => unreachable!(),
            }
        }
    }

    /// What the OAM DMA engine reads from `addr`, and the bus it occupies
    /// doing so. Mode-based PPU blocking does not apply.
    fn oam_dma_source_read(&self, addr: u16) -> (DmaBus, u8) {
        // 0xE000-0xFFFF: incomplete address decoding re-reads WRAM. This
        // covers the whole range including 0xFE/0xFF pages
        // (acceptance/oam_dma/sources-GS: sources $FE/$FF read $DE00/$DF00).
        let addr = if addr >= 0xE000 { addr - 0x2000 } else { addr };
        match addr {
            0x0000..=0x7FFF => (DmaBus::External, self.cart.read_rom(addr)),
            0x8000..=0x9FFF => (DmaBus::Vram, self.ppu.vram_read_raw(addr)),
            0xA000..=0xBFFF => (DmaBus::External, self.cart.read_ram(addr)),
            _ => (self.wram_bus(), self.wram[self.wram_index(addr)]),
        }
    }

    /// The bus WRAM lives on: shared with the cartridge on DMG, its own bus
    /// on CGB (gbctr: CGB OAM DMA from WRAM does not conflict with ROM).
    fn wram_bus(&self) -> DmaBus {
        if self.model.is_cgb() {
            DmaBus::Wram
        } else {
            DmaBus::External
        }
    }

    /// Which DMA bus a CPU access to `addr` would occupy (None: OAM, IO,
    /// HRAM — never in conflict).
    fn bus_of(&self, addr: u16) -> Option<DmaBus> {
        match addr {
            0x0000..=0x7FFF | 0xA000..=0xBFFF => Some(DmaBus::External),
            0x8000..=0x9FFF => Some(DmaBus::Vram),
            0xC000..=0xFDFF => Some(self.wram_bus()),
            _ => None,
        }
    }

    // ---- CGB VRAM DMA ---------------------------------------------------

    fn hblank_dma_check(&mut self) {
        let hb = self.ppu.hblank_active();
        let edge = hb && !self.hdma_prev_hblank;
        self.hdma_prev_hblank = hb;
        if edge && self.hdma_active && !self.vram_dma_stall {
            self.vram_dma_stall = true;
            self.copy_vram_dma_block();
            self.vram_dma_stall = false;
            self.hdma_blocks_left -= 1;
            if self.hdma_blocks_left == 0 {
                self.hdma_active = false;
                self.hdma_latch = 0xFF;
            }
        }
    }

    /// Copy one 16-byte block, stalling the CPU: 8 M-cycles at normal speed
    /// (2 bytes per M-cycle), 16 in double speed (gbctr CGB DMA timing
    /// table). The machine keeps running during the stall.
    fn copy_vram_dma_block(&mut self) {
        let cycles = if self.double_speed { 16 } else { 8 };
        for _ in 0..cycles {
            self.tick_machine();
            for _ in 0..(16 / cycles) {
                let byte = self.vram_dma_source_read(self.hdma_src);
                self.ppu
                    .vram_write_raw(0x8000 | (self.hdma_dst & 0x1FFF), byte);
                self.hdma_src = self.hdma_src.wrapping_add(1);
                self.hdma_dst = (self.hdma_dst + 1) & 0x1FFF;
            }
        }
    }

    /// VRAM DMA source read. VRAM itself and the 0xE000+ region are not
    /// valid sources (Pan Docs); they read as 0xFF here.
    fn vram_dma_source_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cart.read_rom(addr),
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xDFFF => self.wram[self.wram_index(addr)],
            _ => 0xFF,
        }
    }

    fn hdma5_write(&mut self, value: u8) {
        if self.hdma_active && value & 0x80 == 0 {
            // Cancel mid-transfer: FF55 reads back the remaining count with
            // bit 7 set (Pan Docs "FF55 — HDMA5").
            self.hdma_active = false;
            self.hdma_latch = 0x80 | (self.hdma_blocks_left - 1);
            return;
        }
        let blocks = (value & 0x7F) + 1;
        if value & 0x80 != 0 {
            // HBlank DMA: 16 bytes per hblank entered. Clearing the edge
            // detector lets a transfer started during hblank copy its first
            // block in that same hblank.
            self.hdma_active = true;
            self.hdma_blocks_left = blocks;
            self.hdma_prev_hblank = false;
        } else {
            // General-purpose DMA: everything at once, CPU stalled.
            self.vram_dma_stall = true;
            for _ in 0..blocks {
                self.copy_vram_dma_block();
            }
            self.vram_dma_stall = false;
            self.hdma_latch = 0xFF;
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

    /// FEA0-FEFF "prohibited" reads. DMG family: $00 while OAM is idle, $FF
    /// while the PPU has OAM locked (the mode-2 corruption itself is not
    /// modelled). CGB: the high nibble of the low address byte twice
    /// (Pan Docs "FEA0-FEFF range", revision E behavior).
    fn prohibited_read(&self, addr: u16) -> u8 {
        if self.model.is_cgb() {
            let lo = addr as u8;
            (lo & 0xF0) | (lo >> 4)
        } else if self.ppu.read(0xFF41) & 0x03 >= 2 {
            0xFF
        } else {
            0x00
        }
    }

    fn read_no_tick(&self, addr: u16) -> u8 {
        if let Some((bus, byte)) = self.dma_conflict {
            // OAM (and the prohibited area behind it) reads $FF while a DMA
            // byte is in flight; reads on the bus the DMA occupies see the
            // DMA's byte (gbctr OAM DMA bus conflicts).
            if (0xFE00..=0xFEFF).contains(&addr) {
                return 0xFF;
            }
            if self.bus_of(addr) == Some(bus) {
                return byte;
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
        match addr {
            0x0000..=0x7FFF => self.cart.write_rom(addr, value),
            0x8000..=0x9FFF => self.ppu.write(addr, value),
            0xA000..=0xBFFF => self.cart.write_ram(addr, value),
            0xC000..=0xFDFF => {
                let i = self.wram_index(addr);
                self.wram[i] = value;
            }
            0xFE00..=0xFE9F => {
                // CPU OAM writes are dropped while DMA owns OAM.
                if self.dma_conflict.is_none() {
                    self.ppu.write(addr, value);
                }
            }
            0xFEA0..=0xFEFF => {}
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
            0xFF55 if self.cgb_mode => {
                if self.hdma_active {
                    self.hdma_blocks_left - 1
                } else {
                    self.hdma_latch
                }
            }
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
            // FF76/FF77: read-only APU digital outputs (stubbed silent).
            0xFF76 | 0xFF77 if self.model.is_cgb() => 0x00,
            // FF50 (boot ROM disable) and everything unmapped: $FF.
            _ => 0xFF,
        }
    }

    fn io_write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF00 => self.joypad.write(value),
            0xFF01 | 0xFF02 => self.serial.write(addr, value),
            0xFF04..=0xFF07 => self.intf |= self.timer.write(addr, value) & 0x1F,
            0xFF0F => self.intf = value & 0x1F,
            0xFF10..=0xFF3F => self.apu.write(addr, value),
            0xFF46 => {
                self.dma_reg = value;
                self.dma_start = Some(OamDmaStart {
                    src: u16::from(value) << 8,
                    delay: 1,
                });
            }
            0xFF40..=0xFF45 | 0xFF47..=0xFF4B => {
                self.ppu.write(addr, value);
                // Register writes can raise the STAT line in this very
                // cycle (stat_lyc_onoff round 4).
                self.intf |= self.ppu.consume_pending_irq() & 0x1F;
            }
            0xFF4D if self.cgb_mode => self.key1_armed = value & 1 != 0,
            0xFF4F if self.cgb_mode => self.ppu.write(addr, value),
            0xFF51 if self.cgb_mode => {
                self.hdma_src = (self.hdma_src & 0x00F0) | (u16::from(value) << 8)
            }
            0xFF52 if self.cgb_mode => {
                self.hdma_src = (self.hdma_src & 0xFF00) | u16::from(value & 0xF0)
            }
            0xFF53 if self.cgb_mode => {
                self.hdma_dst = (self.hdma_dst & 0x00F0) | (u16::from(value & 0x1F) << 8)
            }
            0xFF54 if self.cgb_mode => {
                self.hdma_dst = (self.hdma_dst & 0x1F00) | u16::from(value & 0xF0)
            }
            0xFF55 if self.cgb_mode => self.hdma5_write(value),
            0xFF56 if self.cgb_mode => self.rp = value & 0xC1,
            0xFF68 | 0xFF6A => {
                self.ppu.write(addr, value);
                self.intf |= self.ppu.consume_pending_irq() & 0x1F;
            }
            0xFF69 | 0xFF6B if self.cgb_mode => {
                self.ppu.write(addr, value);
                self.intf |= self.ppu.consume_pending_irq() & 0x1F;
            }
            // OPRI is set up by the boot ROM and locked outside CGB mode.
            0xFF6C if self.cgb_mode => self.ppu.write(addr, value),
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
/// (addresses ≥ $0150 read as $00). Each zero bit costs one extra M-cycle
/// of boot time relative to a one bit — calibrated against
/// acceptance/boot_div-S and boot_div2-S, which differ only in the global
/// checksum bytes.
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
        self.tick_machine();
        self.read_no_tick(addr)
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.tick_machine();
        self.write_no_tick(addr, value);
    }

    fn tick(&mut self) {
        self.tick_machine();
    }

    fn pending(&self) -> u8 {
        self.intf & self.ie & 0x1F
    }

    fn ack(&mut self, bit: u8) {
        self.intf &= !(1 << bit);
    }

    fn stop(&mut self) -> bool {
        // STOP resets DIV on every model (Pan Docs "FF04 — DIV"). Model it
        // as a DIV write so the TIMA falling-edge effects apply.
        self.intf |= self.timer.write(0xFF04, 0) & 0x1F;
        if self.cgb_mode && self.key1_armed {
            self.double_speed = !self.double_speed;
            self.key1_armed = false;
            true
        } else {
            false
        }
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

    /// acceptance/oam_dma_restart: the old transfer keeps running during
    /// the new one's setup delay, then the new one starts from byte 0.
    #[test]
    fn oam_dma_restart_old_transfer_runs_through_setup() {
        let mut b = ic(Model::Dmg);
        fill_wram(&mut b, 0xC000, 0x80, 160); // old source
        fill_wram(&mut b, 0xD000, 0x10, 160); // new source
        b.write(0xFF46, 0xC0); // cycle W
        b.tick(); // W+1 setup
        b.tick(); // W+2 old byte 0
        b.write(0xFF46, 0xD0); // cycle W+3: old byte 1 copied, then write
                               // Cycle W+4 (new setup): the old transfer copies byte 2. Observe it
                               // through the external-bus conflict (WRAM shares the bus on DMG).
        assert_eq!(b.read(0x0000), 0x82);
        // Cycle W+5: new transfer byte 0.
        assert_eq!(b.read(0x0000), 0x10);
        ticks(&mut b, 161);
        assert_eq!(b.read(0xFE00), 0x10);
        assert_eq!(b.read(0xFE05), 0x15);
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

    #[test]
    fn cgb_wram_is_a_separate_bus() {
        let mut b = ic(Model::Cgb);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        b.write(0xFF46, 0x10); // ROM source
        b.tick();
        b.tick();
        assert_eq!(b.read(0xC000), 0x80, "CGB WRAM does not conflict with ROM");
        let mut b = ic(Model::Cgb);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        b.write(0xFF46, 0xC0); // WRAM source
        b.tick();
        b.tick();
        assert_eq!(b.read(0x1000), 0x5A, "ROM does not conflict with CGB WRAM");
        assert_eq!(b.read(0xC050), 0x82, "WRAM read sees DMA byte 2");
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

    #[test]
    fn prohibited_area_cgb_echoes_high_nibble() {
        let mut b = ic(Model::Cgb);
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
        let mut b = ic_cgb_mode();
        assert_eq!(b.read(0xFF4D), 0x7E);
        assert!(!b.stop(), "not armed: deep stop");
        b.write(0xFF4D, 0xFF);
        assert_eq!(b.read(0xFF4D), 0x7F);
        ticks(&mut b, 100);
        assert!(b.stop(), "armed: switch performed");
        assert_eq!(b.read(0xFF4D), 0xFE, "double speed, no longer armed");
        assert_eq!(b.read(0xFF04), 0x00, "STOP reset DIV");
        // Switch back.
        b.write(0xFF4D, 0x01);
        assert!(b.stop());
        assert_eq!(b.read(0xFF4D), 0x7E);
    }

    #[test]
    fn stop_resets_div_on_dmg() {
        let mut b = ic(Model::Dmg);
        ticks(&mut b, 100);
        assert_ne!(b.read(0xFF04), 0);
        assert!(!b.stop());
        assert_eq!(b.read(0xFF04), 0);
    }

    #[test]
    fn double_speed_halves_dots_per_m_cycle() {
        let mut b = ic_cgb_mode();
        b.write(0xFF4D, 0x01);
        b.stop();
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

    #[test]
    fn gdma_copies_blocks_and_stalls() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x00, 0x40);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        let before = b.cycles();
        b.write(0xFF55, 0x03); // 4 blocks = 64 bytes
                               // Write cycle (4 dots) + 4 blocks x 8 M-cycles (32 dots each... 8*4).
        assert_eq!(b.cycles() - before, 4 + 4 * 8 * 4);
        assert_eq!(b.read(0xFF55), 0xFF, "completed");
        assert_eq!(b.read(0x8000), 0x00);
        assert_eq!(b.read(0x803F), 0x3F);
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
        b.write(0xFF55, 0x00); // next block continues at +0x10
        assert_eq!(b.read(0x8010), 0x10);
        assert_eq!(b.read(0x801F), 0x1F);
    }

    #[test]
    fn hblank_dma_one_block_per_hblank() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x40, 0x20);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        b.write(0xFF40, 0x91); // LCD on: glitched line, hblank from ~dot 250
        b.write(0xFF55, 0x81); // hblank DMA, 2 blocks (PPU at dot 4)
        assert_eq!(b.read(0xFF55), 0x01, "2 blocks remaining reads 1");
        assert_eq!(b.read(0x8000), 0x00, "nothing copied before hblank");
        // PPU at dot 12. Run into the glitched line's hblank; the block
        // transfer itself stalls 8 M-cycles (32 more dots).
        ticks(&mut b, 87); // ~dot 392 incl. the stall
        assert_eq!(b.read(0xFF55), 0x00, "one block left");
        assert_eq!(b.read(0x8000), 0x40);
        assert_eq!(b.read(0x800F), 0x4F);
        assert_eq!(b.read(0x8010), 0x00, "second block waits for next hblank");
        // Run well into line 1's hblank (~dot 702-908 from enable).
        ticks(&mut b, 98);
        assert_eq!(b.read(0xFF55), 0xFF, "done");
        assert_eq!(b.read(0x8010), 0x50);
        assert_eq!(b.read(0x801F), 0x5F);
    }

    #[test]
    fn hblank_dma_cancel_sets_bit7() {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x40, 0x80);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        b.write(0xFF40, 0x91);
        b.write(0xFF55, 0x87); // 8 blocks
        ticks(&mut b, 87); // through the first hblank entry: one block done
        assert_eq!(b.read(0xFF55), 0x06);
        b.write(0xFF55, 0x00); // cancel
        assert_eq!(b.read(0xFF55), 0x86, "cancelled: bit 7 + remaining");
        ticks(&mut b, 101); // into line 1's hblank (~dot 800, VRAM readable)
        assert_eq!(b.read(0x8010), 0x00, "no further blocks after cancel");
    }

    // ---- post-boot state ---------------------------------------------------

    fn booted(model: Model) -> Interconnect {
        let mut b = ic(model);
        b.apply_post_boot_state();
        b
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
}
