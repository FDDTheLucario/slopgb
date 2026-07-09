//! Addressing-mode operand resolution + the multi-step `MOV` handlers.
//!
//! `am_*` fetch operand bytes and return the source **value**; `ea_*` fetch
//! operand bytes and return the effective **address** (for stores / RMW). Each
//! consumes exactly the operand bytes its mode encodes.
//!
//! Direct-page indexing (`dp+X`, `dp+Y`) wraps within the 256-byte page; the
//! indirect pointer bytes wrap within the page too (see [`Spc700::read_word_dp`]).
//! `!abs+X`/`!abs+Y` index across the full 16-bit space. (fullsnes, "SPC700
//! Addressing Modes".)

use super::*;

impl Spc700 {
    // -- value readers (for ALU `A,<mode>` and `MOV reg,<mode>`) -----------

    /// `#imm`
    pub(super) fn am_imm(&mut self) -> u8 {
        self.fetch()
    }
    /// `dp`
    pub(super) fn am_dp(&mut self) -> u8 {
        let a = self.ea_dp();
        self.read8(a)
    }
    /// `dp+X`
    pub(super) fn am_dpx(&mut self) -> u8 {
        let a = self.ea_dpx();
        self.read8(a)
    }
    /// `dp+Y` (only `MOV X, dp+Y`)
    pub(super) fn am_dpy(&mut self) -> u8 {
        let a = self.ea_dpy();
        self.read8(a)
    }
    /// `!abs`
    pub(super) fn am_abs(&mut self) -> u8 {
        let a = self.ea_abs();
        self.read8(a)
    }
    /// `!abs+X`
    pub(super) fn am_absx(&mut self) -> u8 {
        let a = self.ea_absx();
        self.read8(a)
    }
    /// `!abs+Y`
    pub(super) fn am_absy(&mut self) -> u8 {
        let a = self.ea_absy();
        self.read8(a)
    }
    /// `(X)` — direct-page indirect via X, no post-increment.
    pub(super) fn am_xind(&mut self) -> u8 {
        let a = self.dp(self.x);
        self.read8(a)
    }
    /// `(X)+` — read via X, then X++ (`MOV A,(X)+`).
    pub(super) fn am_xind_inc(&mut self) -> u8 {
        let a = self.dp(self.x);
        let v = self.read8(a);
        self.x = self.x.wrapping_add(1);
        v
    }
    /// `[dp+X]` — indexed indirect (pre-index X into the pointer table).
    pub(super) fn am_idx(&mut self) -> u8 {
        let a = self.ea_idx();
        self.read8(a)
    }
    /// `[dp]+Y` — indirect indexed (post-index Y onto the pointer).
    pub(super) fn am_idy(&mut self) -> u8 {
        let a = self.ea_idy();
        self.read8(a)
    }

    // -- effective-address computers (for stores / RMW) --------------------

    /// `dp`
    pub(super) fn ea_dp(&mut self) -> u16 {
        let off = self.fetch();
        self.dp(off)
    }
    /// `dp+X`
    pub(super) fn ea_dpx(&mut self) -> u16 {
        let off = self.fetch();
        self.dp(off.wrapping_add(self.x))
    }
    /// `dp+Y`
    pub(super) fn ea_dpy(&mut self) -> u16 {
        let off = self.fetch();
        self.dp(off.wrapping_add(self.y))
    }
    /// `!abs`
    pub(super) fn ea_abs(&mut self) -> u16 {
        self.fetch16()
    }
    /// `!abs+X`
    pub(super) fn ea_absx(&mut self) -> u16 {
        self.fetch16().wrapping_add(self.x as u16)
    }
    /// `!abs+Y`
    pub(super) fn ea_absy(&mut self) -> u16 {
        self.fetch16().wrapping_add(self.y as u16)
    }
    /// `[dp+X]` — pointer read from `dp+X` (page-wrapped), no further indexing.
    pub(super) fn ea_idx(&mut self) -> u16 {
        let off = self.fetch();
        self.read_word_dp(off.wrapping_add(self.x))
    }
    /// `[dp]+Y` — pointer read from `dp`, then `+Y` across the full space.
    pub(super) fn ea_idy(&mut self) -> u16 {
        let off = self.fetch();
        self.read_word_dp(off).wrapping_add(self.y as u16)
    }

    // -- multi-step MOV handlers -------------------------------------------

    /// `MOV (X)+, A` (`AF`): store A via X, then X++. No flags.
    pub(super) fn mov_xinc_store(&mut self) {
        let a = self.dp(self.x);
        let v = self.a;
        self.write8(a, v);
        self.x = self.x.wrapping_add(1);
    }

    /// `MOV dp, dp` (`FA nn mm`): `[mm] = [nn]`, first byte = source, second =
    /// dest. No flags. (fullsnes: `FA nn mm  MOV (mm),(nn)`.)
    pub(super) fn mov_dp_dp(&mut self) {
        let src = self.fetch();
        let dst = self.fetch();
        let sa = self.dp(src);
        let v = self.read8(sa);
        let da = self.dp(dst);
        self.write8(da, v);
    }

    /// `MOV dp, #imm` (`8F nn mm`): `[mm] = nn`, first byte = immediate, second =
    /// dest. No flags.
    pub(super) fn mov_dp_imm(&mut self) {
        let imm = self.fetch();
        let dst = self.fetch();
        let da = self.dp(dst);
        self.write8(da, imm);
    }
}
