//! [`GameBoyView`] — read-only handle onto the live machine, and the
//! [`Registers`] snapshot it returns.

use crate::abi::{self, Reg};

/// Read-only window onto the running Game Boy, handed to
/// [`Plugin::on_frame`](crate::Plugin::on_frame) each frame and to
/// [`ToolPlugin::call`](crate::ToolPlugin::call) on demand. Reads never perturb
/// emulation.
///
/// The plain accessors ([`read`](Self::read) / [`reg`](Self::reg) /
/// [`registers`](Self::registers) / [`log`](Self::log)) are available to every
/// tier. The richer debug helpers ([`read_banked`](Self::read_banked),
/// [`disassemble`](Self::disassemble), [`vram`](Self::vram), …) are served only
/// on the tool-plugin host, so a tier-1 plugin that calls one fails to load (its
/// module would import a host function the per-frame host does not provide).
pub struct GameBoyView {
    _private: (),
}

impl GameBoyView {
    /// Constructed only by the generated export shim.
    #[doc(hidden)]
    #[must_use]
    pub fn __new() -> Self {
        Self { _private: () }
    }

    /// One byte of the CPU address space (`$0000..=$FFFF`, bank 0), no I/O
    /// side effects.
    #[must_use]
    pub fn read(&self, addr: u16) -> u8 {
        (abi::host_read(i32::from(addr)) & 0xFF) as u8
    }

    /// One register or I/O value.
    #[must_use]
    pub fn reg(&self, reg: Reg) -> u16 {
        (abi::host_reg(reg.index()) & 0xFFFF) as u16
    }

    /// All exposed registers at once.
    #[must_use]
    pub fn registers(&self) -> Registers {
        Registers {
            af: self.reg(Reg::Af),
            bc: self.reg(Reg::Bc),
            de: self.reg(Reg::De),
            hl: self.reg(Reg::Hl),
            sp: self.reg(Reg::Sp),
            pc: self.reg(Reg::Pc),
            lcdc: self.reg(Reg::Lcdc) as u8,
            stat: self.reg(Reg::Stat) as u8,
            ly: self.reg(Reg::Ly) as u8,
        }
    }

    /// Append a UTF-8 line to the host plugin log.
    pub fn log(&self, line: &str) {
        abi::host_log(line.as_ptr() as i32, line.len() as i32);
    }

    // --- Tool-plugin debug helpers (served only on the tool host) ---

    /// One byte of an explicit **bank** of the banked regions (ROMX
    /// `$4000-$7FFF`, VRAM `$8000-$9FFF`, SRAM `$A000-$BFFF`, WRAMX
    /// `$D000-$DFFF`); elsewhere `bank` is ignored (== [`read`](Self::read)).
    #[must_use]
    pub fn read_banked(&self, bank: u16, addr: u16) -> u8 {
        (abi::host_read_banked(i32::from(bank), i32::from(addr)) & 0xFF) as u8
    }

    /// The code/data-log access flags for a byte of an explicit bank (`r`=1,
    /// `w`=2, `x`=4; 0 when the log is off or the byte is unvisited).
    #[must_use]
    pub fn cdl_flag(&self, bank: u16, addr: u16) -> u8 {
        (abi::host_cdl_flag(i32::from(bank), i32::from(addr)) & 0xFF) as u8
    }

    /// Set a PC breakpoint in the host's breakpoint set (the one mutating
    /// helper; gated by the [`MUTATE`](crate::Capabilities::MUTATE) capability).
    pub fn set_breakpoint(&self, addr: u16) {
        let _ = abi::host_set_breakpoint(i32::from(addr));
    }

    /// The host's one-line CPU + LCD register readout.
    // The wrapping closure is required: on wasm32 the import is an `extern "C"`
    // fn item, which does not implement `FnMut` (only native's stub does, where
    // clippy would otherwise flag the closure as redundant).
    #[allow(clippy::redundant_closure)]
    #[must_use]
    pub fn registers_text(&self) -> String {
        bulk(|ptr, cap| abi::host_registers(ptr, cap))
    }

    /// The continuous address ranges the code/data log has recorded so far, one
    /// `AAAA-AAAA` / `BB:AAAA-BB:AAAA` per line (empty when off / nothing logged).
    #[allow(clippy::redundant_closure)]
    #[must_use]
    pub fn cdl_ranges(&self) -> String {
        bulk(|ptr, cap| abi::host_cdl_ranges(ptr, cap))
    }

    /// Disassemble `[from, to]` in `bank`, one instruction per line
    /// (`BB:AAAA\tlabel\tinstruction\tcycles`), symbol names substituted.
    #[must_use]
    pub fn disassemble(&self, bank: u16, from: u16, to: u16) -> String {
        bulk(|ptr, cap| abi::host_disasm(i32::from(bank), i32::from(from), i32::from(to), ptr, cap))
    }

    /// The current 160×144 screen as PNG bytes, nearest-neighbor magnified by
    /// `scale` (1 = native).
    #[must_use]
    pub fn screencap(&self, scale: u32) -> Vec<u8> {
        bulk_bytes(|ptr, cap| abi::host_screencap(scale as i32, ptr, cap))
    }

    /// A VRAM view (`bg`/`win`/`tile0`/`tile1`/`oam`/`palette`) as PNG bytes,
    /// magnified by `scale`. Empty on an unknown view name.
    #[must_use]
    pub fn vram(&self, view: &str, scale: u32) -> Vec<u8> {
        bulk_bytes(|ptr, cap| {
            abi::host_vram(
                view.as_ptr() as i32,
                view.len() as i32,
                scale as i32,
                ptr,
                cap,
            )
        })
    }

    /// Evaluate a bgb-style debugger expression against the live regs + memory.
    #[must_use]
    pub fn expr(&self, expression: &str) -> String {
        bulk(|ptr, cap| {
            abi::host_expr(
                expression.as_ptr() as i32,
                expression.len() as i32,
                ptr,
                cap,
            )
        })
    }
}

/// Run a host bulk-result import into a guest-owned scratch and return the
/// bytes. `call(out_ptr, out_cap) -> true_len`: the host writes up to `out_cap`
/// bytes into our buffer and reports the full length, so we grow + retry when it
/// overflows. No `unsafe`: the host writes through wasmi's bounds-checked
/// `Memory`, and we read our own buffer by safe indexing (`truncate`).
fn bulk_bytes(mut call: impl FnMut(i32, i32) -> i32) -> Vec<u8> {
    let mut buf = vec![0u8; 512];
    loop {
        let need = call(buf.as_ptr() as i32, buf.len() as i32).max(0) as usize;
        if need <= buf.len() {
            buf.truncate(need);
            return buf;
        }
        buf.resize(need, 0);
    }
}

/// [`bulk_bytes`] decoded as UTF-8 (lossy) — the text bulk results.
fn bulk(call: impl FnMut(i32, i32) -> i32) -> String {
    String::from_utf8_lossy(&bulk_bytes(call)).into_owned()
}

/// Frame-consistent CPU registers plus key LCD I/O bytes, from
/// [`GameBoyView::registers`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Registers {
    pub af: u16,
    pub bc: u16,
    pub de: u16,
    pub hl: u16,
    pub sp: u16,
    pub pc: u16,
    /// `LCDC` `$FF40`.
    pub lcdc: u8,
    /// `STAT` `$FF41`.
    pub stat: u8,
    /// `LY` `$FF44`.
    pub ly: u8,
}
