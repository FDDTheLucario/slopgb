//! Clean-room SNES PPU (S-PPU) for the slopgb SGB SNES side, authored purely
//! from nocash *fullsnes* ("SNES Memory VRAM/CGRAM/OAM Access", "SNES PPU"
//! chapters) — never from emulator source. This crate is native-unit-testable
//! and zero-dep; the wasm coprocessor wrapper lives in
//! `slopgb-snes-ppu-plugin`.
//!
//! Current scope: the three memory-access port groups (VRAM `$2115-$2119` /
//! `$2139/$213A`, CGRAM `$2121/$2122/$213B`, OAM `$2102-$2104/$2138`) with
//! their address/latch/prefetch state machines. The BG/OBJ renderer layers on
//! next.
//!
//! Deliberate ceilings: the rendering-time OAM address destruction and the
//! PPU2 open-bus bit in CGRAM reads are not modeled (no renderer timing
//! exists to destroy against; the open-bus bit reads 0).

#![forbid(unsafe_code)]

/// VRAM: 32 K words installed (address bit 15 unconnected — `$8000-$FFFF`
/// word-addresses mirror `$0000-$7FFF`; fullsnes 2116h).
const VRAM_WORDS: usize = 0x8000;
/// OAM: 512 bytes of 4-byte OBJ entries + the 32-byte high table; byte
/// addresses `$220-$3FF` mirror `$200-$21F` (fullsnes 2104h).
const OAM_LEN: usize = 0x220;

/// The S-PPU memory ports: VRAM, CGRAM, and OAM behind their B-bus access
/// state machines. `write`/`read` take the B-bus port number (`$21xx` low
/// byte).
pub struct SnesPpu {
    vram: Box<[u16; VRAM_WORDS]>,
    cgram: [u16; 256],
    oam: [u8; OAM_LEN],

    /// VMAIN (`$2115`): bit 7 = increment on high-byte access, bits 3-2 =
    /// address translation, bits 1-0 = increment step.
    vmain: u8,
    /// VMADD (`$2116/17`): the VRAM word address (untranslated).
    vmadd: u16,
    /// The RDVRAM prefetch word (fullsnes 2139h).
    prefetch: u16,

    /// CGADD (`$2121`): the CGRAM word (color) address.
    cgadd: u8,
    /// The shared 1st/2nd-access flipflop for `$2122`/`$213B` (reset by a
    /// `$2121` write) + the memorized low byte.
    cg_second: bool,
    cg_lsb: u8,

    /// The 9-bit OAMADD reload value + priority-rotation flag (`$2102/03`).
    oam_reload: u16,
    oam_priority: bool,
    /// The live 10-bit OAM byte address.
    oam_addr: u16,
    /// The memorized low byte for low-table word writes.
    oam_lsb: u8,

    /// BGMODE (`$2105`): mode bits 2-0, BG3-priority bit 3, per-BG tile
    /// size bits 4-7.
    bgmode: u8,
    /// BGnSC (`$2107-$210A`): map base (bits 7-2, 1K-word steps) + screen
    /// size (bits 1-0).
    bgsc: [u8; 4],
    /// BG12NBA/BG34NBA (`$210B/0C`): per-BG tile base nibbles (4K-word
    /// steps).
    nba: [u8; 2],
    /// The effective 10-bit BGnHOFS/BGnVOFS scroll values (`$210D-$2114`).
    hofs: [u16; 4],
    vofs: [u16; 4],
    /// The shared write-twice scroll latch ("BG_old" — fullsnes 210Dh; one
    /// latch across all eight scroll registers).
    bg_old: u8,
    /// TM (`$212C`): main-screen layer enables (bit 0-3 BG1-4, bit 4 OBJ).
    tm: u8,
    /// OBSEL (`$2101`): OBJ size selection (bits 7-5), name gap (4-3),
    /// tile base (2-0).
    obsel: u8,
    /// INIDISP (`$2100`): forced blank (bit 7) + master brightness (3-0).
    inidisp: u8,
}

impl SnesPpu {
    pub fn new() -> Self {
        SnesPpu {
            vram: vec![0u16; VRAM_WORDS]
                .into_boxed_slice()
                .try_into()
                .unwrap_or_else(|_| unreachable!()),
            cgram: [0; 256],
            oam: [0; OAM_LEN],
            vmain: 0,
            vmadd: 0,
            prefetch: 0,
            cgadd: 0,
            cg_second: false,
            cg_lsb: 0,
            oam_reload: 0,
            oam_priority: false,
            oam_addr: 0,
            oam_lsb: 0,
            bgmode: 0,
            bgsc: [0; 4],
            nba: [0; 2],
            hofs: [0; 4],
            vofs: [0; 4],
            bg_old: 0,
            tm: 0,
            obsel: 0,
            inidisp: 0,
        }
    }

    /// A B-bus write to `$2100 + port`. Ports outside this chip are ignored.
    pub fn write(&mut self, port: u8, val: u8) {
        match port {
            0x02 => {
                // OAMADDL: low 8 reload bits; either OAMADD write copies the
                // whole 9-bit reload to the address with bit 0 = 0
                // (fullsnes 2102h).
                self.oam_reload = self.oam_reload & 0x100 | u16::from(val);
                self.oam_addr = self.oam_reload << 1;
            }
            0x03 => {
                // OAMADDH: reload bit 8 + the priority-rotation flag.
                self.oam_reload = self.oam_reload & 0xFF | u16::from(val & 1) << 8;
                self.oam_priority = val & 0x80 != 0;
                self.oam_addr = self.oam_reload << 1;
            }
            0x04 => {
                // OAMDATA (fullsnes 2104h): low table latches even bytes and
                // lands the word on the odd byte; the high table takes the
                // byte directly.
                let a = self.oam_addr & 0x3FF;
                if a >= 0x200 {
                    self.oam[Self::oam_index(a)] = val;
                } else if a & 1 == 0 {
                    self.oam_lsb = val;
                } else {
                    self.oam[(a - 1) as usize] = self.oam_lsb;
                    self.oam[a as usize] = val;
                }
                self.oam_addr = (self.oam_addr + 1) & 0x3FF;
            }
            0x00 => self.inidisp = val,
            0x01 => self.obsel = val,
            0x05 => self.bgmode = val,
            0x07..=0x0A => self.bgsc[usize::from(port - 0x07)] = val,
            0x0B | 0x0C => self.nba[usize::from(port - 0x0B)] = val,
            0x0D..=0x14 => {
                // The write-twice scroll mechanism, verbatim from fullsnes
                // 210Dh ("BG_old"): one shared previous-byte latch across
                // all eight registers. The 210Dh/210Eh M7 twins are not
                // modeled (mode 7 unsupported).
                // The registers stay full-width here (the renderer masks to
                // 10 bits at use): masking on write would corrupt the
                // formula's `Reg>>8` term on the next write of the pair.
                let i = usize::from(port - 0x0D) / 2;
                let cur = u16::from(val);
                let prev = u16::from(self.bg_old);
                if (port - 0x0D) & 1 == 0 {
                    self.hofs[i] = cur << 8 | prev & !7 | self.hofs[i] >> 8 & 7;
                } else {
                    self.vofs[i] = cur << 8 | prev;
                }
                self.bg_old = val;
            }
            0x15 => self.vmain = val,
            0x16 => {
                self.vmadd = self.vmadd & 0xFF00 | u16::from(val);
                // Prefetch fills after an address change (fullsnes 2139h).
                self.prefetch = self.vram[self.vram_index()];
            }
            0x17 => {
                self.vmadd = self.vmadd & 0x00FF | u16::from(val) << 8;
                self.prefetch = self.vram[self.vram_index()];
            }
            0x18 => {
                let i = self.vram_index();
                self.vram[i] = self.vram[i] & 0xFF00 | u16::from(val);
                // Increment on the byte VMAIN bit 7 selects; writes never
                // touch the prefetch register (fullsnes 2139h).
                if self.vmain & 0x80 == 0 {
                    self.step_vmadd();
                }
            }
            0x19 => {
                let i = self.vram_index();
                self.vram[i] = self.vram[i] & 0x00FF | u16::from(val) << 8;
                if self.vmain & 0x80 != 0 {
                    self.step_vmadd();
                }
            }
            0x2C => self.tm = val,
            0x21 => {
                // CGADD resets the shared 1st/2nd-access flipflop
                // (fullsnes 2121h).
                self.cgadd = val;
                self.cg_second = false;
            }
            0x22 => {
                if self.cg_second {
                    self.cgram[usize::from(self.cgadd)] =
                        u16::from(val) << 8 | u16::from(self.cg_lsb);
                    self.cgadd = self.cgadd.wrapping_add(1);
                } else {
                    self.cg_lsb = val;
                }
                self.cg_second = !self.cg_second;
            }
            _ => {}
        }
    }

    /// A B-bus read from `$2100 + port`. Unhandled ports read 0.
    pub fn read(&mut self, port: u8) -> u8 {
        match port {
            0x38 => {
                let v = self.oam[Self::oam_index(self.oam_addr & 0x3FF)];
                self.oam_addr = (self.oam_addr + 1) & 0x3FF;
                v
            }
            0x39 => {
                let v = self.prefetch as u8;
                // Prefetch BEFORE increment — the hardware glitch that makes
                // the first word after an address load appear twice
                // (fullsnes 2139h).
                if self.vmain & 0x80 == 0 {
                    self.prefetch = self.vram[self.vram_index()];
                    self.step_vmadd();
                }
                v
            }
            0x3A => {
                let v = (self.prefetch >> 8) as u8;
                if self.vmain & 0x80 != 0 {
                    self.prefetch = self.vram[self.vram_index()];
                    self.step_vmadd();
                }
                v
            }
            0x3B => {
                let w = self.cgram[usize::from(self.cgadd)];
                let v = if self.cg_second {
                    self.cgadd = self.cgadd.wrapping_add(1);
                    // Upper 7 bits; bit 7 is PPU2 open bus, modeled as 0.
                    (w >> 8) as u8 & 0x7F
                } else {
                    w as u8
                };
                self.cg_second = !self.cg_second;
                v
            }
            _ => 0,
        }
    }

    /// The effective VRAM word index for the current VMADD: the VMAIN
    /// address translation thrice left-rotates the low 8/9/10 bits on
    /// access only (VMADD itself keeps counting untranslated), and bit 15
    /// is unconnected (fullsnes 2115h/2116h).
    fn vram_index(&self) -> usize {
        let addr = self.vmadd;
        let translated = match self.vmain >> 2 & 3 {
            0 => addr,
            n => {
                let bits = 7 + u16::from(n);
                let mask = (1u16 << bits) - 1;
                let low = addr & mask;
                addr & !mask | (low << 3 | low >> (bits - 3)) & mask
            }
        };
        usize::from(translated & 0x7FFF)
    }

    /// Advance VMADD by the VMAIN step (1/32/128/128 — fullsnes 2115h).
    fn step_vmadd(&mut self) {
        let step = match self.vmain & 3 {
            0 => 1,
            1 => 32,
            _ => 128,
        };
        self.vmadd = self.vmadd.wrapping_add(step);
    }

    /// The backing index for a 10-bit OAM byte address: `$220-$3FF` mirror
    /// the 32-byte high table (fullsnes 2104h).
    fn oam_index(addr: u16) -> usize {
        usize::from(if addr < 0x200 {
            addr
        } else {
            0x200 + (addr & 0x1F)
        })
    }

    /// The installed VRAM words (debug/renderer access).
    pub fn vram(&self) -> &[u16] {
        &self.vram[..]
    }

    /// The 256 CGRAM color words (debug/renderer access).
    pub fn cgram(&self) -> &[u16] {
        &self.cgram
    }

    /// The 544 OAM bytes (debug/renderer access).
    pub fn oam(&self) -> &[u8] {
        &self.oam
    }
}

impl Default for SnesPpu {
    fn default() -> Self {
        Self::new()
    }
}

mod frame;
mod obj;
mod render;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
