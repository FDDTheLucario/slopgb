//! Tests for the deferred-commit clock: each pins a worked example from
//! `docs/sameboy-port/cpu-timing-map.md` against SameBoy 1.0.2 `sm83_cpu.c`.

use super::*;

/// `cpu-timing-map.md` §2.1: `LDH A,(a8)` (`ld_a_da8`, `sm83_cpu.c:1284`).
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

/// `cpu-timing-map.md` §4: `cycle_no_access` (`sm83_cpu.c:321`) parks +4 with no
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

/// `cpu-timing-map.md` §3: every conflict class conserves the per-M-cycle total
/// of 4 T-cycles (the pre-commit split is reclaimed by the re-park), so overall
/// instruction timing is unchanged while the sub-M-cycle commit point varies.
#[test]
fn conflict_write_conserves_total() {
    for conflict in [Conflict::ReadOld, Conflict::ReadNew, Conflict::WriteCpu] {
        let mut c = CycleClock::new();
        c.read(); // M-cycle 1: parks 4
        c.write(conflict); // M-cycle 2: conflict-staged commit
        c.flush();
        assert_eq!(c.now(), 8, "two M-cycles total 8 T for {conflict:?}");
    }
}

/// `cpu-timing-map.md` §3: the commit point itself shifts per class — `ReadNew`
/// lands 1 T early, `WriteCpu` 1 T late, `ReadOld` at the leading edge.
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
}

/// `cpu-timing-map.md` §6: the interrupt-dispatch vector retime
/// (`sm83_cpu.c:1690-1692`) latches the IF-ack / vector 2 T before the final
/// push M-cycle completes.
#[test]
fn dispatch_vector_retime_latches_two_t_early() {
    let mut c = CycleClock::new();
    c.read(); // parks 4 (the push M-cycle's debt)
    let latch = c.dispatch_vector_retime();
    assert_eq!(
        latch, 2,
        "vector latched 2 T before the M-cycle's 4 would complete"
    );
    assert_eq!(c.pending(), 2, "the final 2 T stay parked");
}
