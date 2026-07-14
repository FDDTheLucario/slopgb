//! Guest↔host wire contract: ABI version, register index map, wasm imports.
//! Host counterpart is `slopgb-plugin-host`; both must agree on the items here.

/// Incremented on any incompatible import/export change. Host reads the guest's
/// `slopgb_abi_version()` export and refuses a mismatch.
pub const ABI_VERSION: i32 = 1;

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

// Imports are `safe fn`, so call sites in `view` need no `unsafe` block and pass
// no raw pointer (host→guest is one scalar per call; guest→host strings pass the
// guest's own `as_ptr`/`len`, read back via wasmi's bounds-checked `Memory`). The
// `unsafe extern` block header is a linkage marker, the sole reason for `allow`.
#[cfg(target_arch = "wasm32")]
#[allow(unsafe_code)]
mod raw {
    #[link(wasm_import_module = "slopgb")]
    unsafe extern "C" {
        pub safe fn host_read(addr: i32) -> i32;
        pub safe fn host_reg(which: i32) -> i32;
        pub safe fn host_log(ptr: i32, len: i32);
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
}

pub(crate) use raw::{host_log, host_read, host_reg};
