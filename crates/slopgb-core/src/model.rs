//! Hardware model selection and per-model post-boot state.

/// Which physical Game Boy we are emulating.
///
/// Models differ in initial (post-boot-ROM) CPU/hardware-register state, in
/// a handful of timing details, and (CGB/AGB) in the whole color subsystem.
/// Mooneye test ROM filename suffixes map onto these:
/// `-dmg0`→[`Model::Dmg0`], `-dmgABC`/`-dmgABCmgb`→[`Model::Dmg`],
/// `-mgb`→[`Model::Mgb`], `-S`/`-sgb`→[`Model::Sgb`], `-sgb2`→[`Model::Sgb2`],
/// `-GS`→DMG+SGB, `-C`/`-cgb`/`-cgbABCDE`→[`Model::Cgb`], `-A`→[`Model::Agb`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Model {
    /// Original DMG, early revision 0 boot ROM.
    Dmg0,
    /// DMG revisions A/B/C — "the" Game Boy.
    Dmg,
    /// Game Boy Pocket.
    Mgb,
    /// Super Game Boy.
    Sgb,
    /// Super Game Boy 2.
    Sgb2,
    /// Game Boy Color (revisions A-E).
    Cgb,
    /// Game Boy Advance running in CGB mode.
    Agb,
}

impl Model {
    /// True for models with the color PPU and CGB-only hardware
    /// (VRAM/WRAM banking, palettes, HDMA, double speed).
    pub fn is_cgb(self) -> bool {
        matches!(self, Model::Cgb | Model::Agb)
    }
}

/// Exact machine state at the moment the boot ROM hands control to the
/// cartridge (PC=0x100). One table entry per [`Model`].
///
/// `div_counter` is the internal 16-bit DIV counter value, which encodes how
/// long the boot ROM ran — several mooneye `boot_div*` tests measure it.
#[derive(Debug, Clone, Copy)]
pub struct PostBootState {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
    pub div_counter: u16,
    /// Initial values for FF00..=FF7F and FFFF where they differ from the
    /// peripheral reset defaults: (address, value) pairs applied in order.
    pub hwio: &'static [(u16, u8)],
}

impl Model {
    /// Post-boot state table for this model.
    ///
    /// Implemented (with values verified against mooneye `boot_regs-*`,
    /// `boot_div*` and `boot_hwio-*` ROMs) by the interconnect work package.
    pub fn post_boot_state(self) -> PostBootState {
        todo!("post-boot state tables: interconnect work package")
    }
}
