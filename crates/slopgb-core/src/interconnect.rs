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
    // Interconnect work package owns the rest: WRAM, HRAM, IF, IE, DMA
    // engines, CGB registers, double-speed state.
}

impl Interconnect {
    pub fn new(model: Model, cart: Cartridge) -> Self {
        Self {
            model,
            cart,
            ppu: Ppu::new(model),
            apu: Apu::new(model.is_cgb()),
            timer: Timer::new(),
            serial: Serial::new(model.is_cgb()),
            joypad: Joypad::new(),
            cycles: 0,
        }
    }

    /// Initialise hardware registers and DIV to the post-boot state of the
    /// model (called once from `GameBoy::new`).
    pub fn apply_post_boot_state(&mut self) {
        todo!("interconnect work package")
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
}

impl Bus for Interconnect {
    fn read(&mut self, addr: u16) -> u8 {
        let _ = addr;
        todo!("interconnect work package")
    }

    fn write(&mut self, addr: u16, value: u8) {
        let _ = (addr, value);
        todo!("interconnect work package")
    }

    fn tick(&mut self) {
        todo!("interconnect work package")
    }

    fn pending(&self) -> u8 {
        todo!("interconnect work package")
    }

    fn ack(&mut self, bit: u8) {
        let _ = bit;
        todo!("interconnect work package")
    }

    fn stop(&mut self) -> bool {
        todo!("interconnect work package")
    }
}
