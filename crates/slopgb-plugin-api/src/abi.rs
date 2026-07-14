//! Guest↔host wire contract: ABI version, register index map, wasm imports.
//! Host counterpart is `slopgb-plugin-host`; both must agree on the items here.

/// Incremented on any incompatible import/export change. Host reads the guest's
/// `slopgb_abi_version()` export and refuses a mismatch.
pub const ABI_VERSION: i32 = 2;

/// A readable register or I/O byte. Discriminant is the `host_reg` wire index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Reg {
    Af = 0,
    Bc = 1,
    De = 2,
    Hl = 3,
    Sp = 4,
    Pc = 5,
    /// `LCDC` `$FF40`.
    Lcdc = 6,
    /// `STAT` `$FF41`.
    Stat = 7,
    /// `LY` `$FF44`.
    Ly = 8,
}

impl Reg {
    pub const ALL: [Reg; 9] = [
        Reg::Af,
        Reg::Bc,
        Reg::De,
        Reg::Hl,
        Reg::Sp,
        Reg::Pc,
        Reg::Lcdc,
        Reg::Stat,
        Reg::Ly,
    ];

    #[must_use]
    pub const fn index(self) -> i32 {
        self as i32
    }
}

/// The per-tool metadata fields the host reads at load, one at a time, via the
/// `slopgb_tool_meta(idx, field)` export (each emitted as a text result). A tool
/// module may expose several tools (`slopgb_tool_count()`), so the host loops
/// `idx` and reads all three fields per tool. Kept in sync with the host reader.
pub const META_NAME: i32 = 0;
pub const META_DESCRIPTION: i32 = 1;
pub const META_SCHEMA: i32 = 2;

// Imports are `safe fn`, so call sites in `view` need no `unsafe` block and pass
// no raw pointer for scalars (host→guest is one scalar per call). Two byte-carrying
// shapes cross, both bounds-checked by the host through wasmi's `Memory`:
// guest→host passes the guest's own `as_ptr`/`len` (a log line, a string argument);
// host→guest fills a guest-owned scratch the guest hands over as `as_ptr`/`len`
// (`out_ptr`/`out_cap`) and reads back by safe indexing — the bulk-result imports
// return the true byte length so the guest can grow + retry a short buffer. The
// `unsafe extern` block header is a linkage marker, the sole reason for `allow`.
#[cfg(target_arch = "wasm32")]
#[allow(unsafe_code)]
mod raw {
    #[link(wasm_import_module = "slopgb")]
    unsafe extern "C" {
        pub safe fn host_read(addr: i32) -> i32;
        pub safe fn host_reg(which: i32) -> i32;
        pub safe fn host_log(ptr: i32, len: i32);
        pub safe fn host_emit(kind: i32, ptr: i32, len: i32);
        // Tool-plugin imports. A tier-1 plugin references none of these, so its
        // module declares no import for them and the host need not provide them.
        // Scalars:
        pub safe fn host_read_banked(bank: i32, addr: i32) -> i32;
        pub safe fn host_cdl_flag(bank: i32, addr: i32) -> i32;
        pub safe fn host_set_breakpoint(addr: i32) -> i32;
        // Bulk results: the host writes up to `out_cap` bytes into the guest
        // scratch at `out_ptr` and returns the true byte length.
        pub safe fn host_registers(out_ptr: i32, out_cap: i32) -> i32;
        pub safe fn host_cdl_ranges(out_ptr: i32, out_cap: i32) -> i32;
        pub safe fn host_disasm(bank: i32, from: i32, to: i32, out_ptr: i32, out_cap: i32) -> i32;
        pub safe fn host_screencap(scale: i32, out_ptr: i32, out_cap: i32) -> i32;
        pub safe fn host_vram(
            view_ptr: i32,
            view_len: i32,
            scale: i32,
            out_ptr: i32,
            out_cap: i32,
        ) -> i32;
        pub safe fn host_expr(in_ptr: i32, in_len: i32, out_ptr: i32, out_cap: i32) -> i32;
    }
}

// Off-wasm stubs so the crate also builds natively to share the constants above.
#[cfg(not(target_arch = "wasm32"))]
mod raw {
    pub fn host_read(_addr: i32) -> i32 {
        unreachable!()
    }
    pub fn host_reg(_which: i32) -> i32 {
        unreachable!()
    }
    pub fn host_log(_ptr: i32, _len: i32) {
        unreachable!()
    }
    pub fn host_emit(_kind: i32, _ptr: i32, _len: i32) {
        unreachable!()
    }
    pub fn host_read_banked(_bank: i32, _addr: i32) -> i32 {
        unreachable!()
    }
    pub fn host_cdl_flag(_bank: i32, _addr: i32) -> i32 {
        unreachable!()
    }
    pub fn host_set_breakpoint(_addr: i32) -> i32 {
        unreachable!()
    }
    pub fn host_registers(_out_ptr: i32, _out_cap: i32) -> i32 {
        unreachable!()
    }
    pub fn host_cdl_ranges(_out_ptr: i32, _out_cap: i32) -> i32 {
        unreachable!()
    }
    pub fn host_disasm(_bank: i32, _from: i32, _to: i32, _out_ptr: i32, _out_cap: i32) -> i32 {
        unreachable!()
    }
    pub fn host_screencap(_scale: i32, _out_ptr: i32, _out_cap: i32) -> i32 {
        unreachable!()
    }
    pub fn host_vram(
        _view_ptr: i32,
        _view_len: i32,
        _scale: i32,
        _out_ptr: i32,
        _out_cap: i32,
    ) -> i32 {
        unreachable!()
    }
    pub fn host_expr(_in_ptr: i32, _in_len: i32, _out_ptr: i32, _out_cap: i32) -> i32 {
        unreachable!()
    }
}

pub(crate) use raw::{
    host_cdl_flag, host_cdl_ranges, host_disasm, host_emit, host_expr, host_log, host_read,
    host_read_banked, host_reg, host_registers, host_screencap, host_set_breakpoint, host_vram,
};
