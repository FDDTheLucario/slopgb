//! Tests for the deferred-commit clock: each pins a worked example against
//! SameBoy 1.0.2 `sm83_cpu.c`.

use super::*;

/// `LDH A,(a8)` (`ld_a_da8`, `sm83_cpu.c:1284`).
/// Opcode fetch samples at C0, imm fetch at C0+4, the FF41 data read at
/// **C0+8** (the leading edge of M3), and the trailing flush ends at C0+12.
/// This 4-T-earlier sample than tick-then-access (cc+4 = C0+12) is the crux.
#[test]
fn ldh_a_da8_samples_stat_at_leading_edge() {
    let mut c = CycleClock::new();
    assert_eq!(c.read(), 0, "M1 opcode fetch samples at instruction entry");
    assert_eq!(c.read(), 4, "M2 immediate fetch samples at C0+4");
    assert_eq!(
        c.read(),
        8,
        "M3 FF41 read samples at C0+8 — leading edge of M3"
    );
    c.flush();
    assert_eq!(
        c.now(),
        12,
        "trailing flush advances M3's parked 4 to C0+12"
    );
}

/// `cycle_no_access` (`sm83_cpu.c:321`) parks +4 with no
/// advance, paid lazily by the next access — so a read after one internal
/// M-cycle samples a full M-cycle later.
#[test]
fn internal_cycle_defers_four() {
    let mut c = CycleClock::new();
    assert_eq!(c.read(), 0);
    c.internal();
    assert_eq!(
        c.read(),
        8,
        "the +4 internal debt is paid before the next sample"
    );
    assert_eq!(c.pending(), 4);
}

/// Every conflict class conserves the per-M-cycle total
/// of 4 T-cycles (the pre-commit split is reclaimed by the re-park), so overall
/// instruction timing is unchanged while the sub-M-cycle commit point varies.
#[test]
fn conflict_write_conserves_total() {
    for conflict in [
        Conflict::ReadOld,
        Conflict::ReadNew,
        Conflict::WriteCpu,
        Conflict::EarlyTwo,
        Conflict::WxHold,
    ] {
        let mut c = CycleClock::new();
        c.read(); // M-cycle 1: parks 4
        c.write(conflict); // M-cycle 2: conflict-staged commit
        c.flush();
        assert_eq!(c.now(), 8, "two M-cycles total 8 T for {conflict:?}");
    }
}

/// The commit point itself shifts per class — `ReadNew`
/// lands 1 T early, `WriteCpu` 1 T late, `ReadOld` at the leading edge,
/// `EarlyTwo` (PALETTE_CGB≥D / SCX) 2 T early, `WxHold` (WX_DMG / LCDC tile-sel
/// glitch) at the leading edge like `ReadOld` but with the M-cycle holding one
/// extra T after the commit.
#[test]
fn conflict_write_commit_point_shifts() {
    let commit = |conflict| {
        let mut c = CycleClock::new();
        c.read();
        c.write(conflict)
    };
    assert_eq!(
        commit(Conflict::ReadOld),
        4,
        "READ_OLD commits at the leading edge"
    );
    assert_eq!(commit(Conflict::ReadNew), 3, "READ_NEW commits 1 T early");
    assert_eq!(commit(Conflict::WriteCpu), 5, "WRITE_CPU commits 1 T late");
    assert_eq!(commit(Conflict::EarlyTwo), 2, "EARLY_TWO commits 2 T early");
    assert_eq!(
        commit(Conflict::WxHold),
        4,
        "WX_HOLD commits the value at the leading edge"
    );
}

/// `sm83_cpu.c:262` `GB_CONFLICT_WX_DMG` (and the LCDC tile-sel glitch,
/// `:271`): the value writes at the leading edge, then one extra T elapses
/// (the `wx_just_changed` / `tile_sel_glitch` window) before re-parking 3 —
/// so the running clock advances past the commit while only 3 T stay parked,
/// still conserving the per-M-cycle 4.
#[test]
fn wx_hold_advances_one_t_past_the_commit() {
    let mut c = CycleClock::new();
    c.read(); // parks 4
    let commit = c.write(Conflict::WxHold);
    assert_eq!(commit, 4, "value commits at the leading edge");
    assert_eq!(c.now(), 5, "the clock advances one T past the commit");
    assert_eq!(c.pending(), 3, "only 3 T stay parked for the next M-cycle");
}
