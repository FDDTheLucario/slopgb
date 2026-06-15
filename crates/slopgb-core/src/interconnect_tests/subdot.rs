//! `interconnect_tests` — subdot tests (split for file size).

use super::*;

/// The OAM accessibility unblock is read at the cc+2 MID phase (sub-dot
/// event-phase model, increment 1): when the mode-3→mode-0 unblock
/// lands in the M-cycle's second half (SCX=1 → rise dot ≡ 3) a CPU OAM
/// read in that same cycle still sees mode 3 ($FF), even though the m0
/// IRQ is already dispatch-visible and `line_render_done` is set; a
/// first-half unblock (SCX=0 → dot ≡ 2) is already accessible. One
/// M-cycle later the OAM read returns the unblocked value on both.
/// Pins gambatte `oam_access/postread_*` (out3 vs out0 round pair).
#[test]
fn oam_read_holds_blocked_at_cc2_through_a_second_half_unblock() {
    for (scx, second_half) in [(0u8, false), (1, true)] {
        let mut b = ic(Model::Dmg);
        b.write(0xFF43, scx);
        b.write(0xFF41, 0x08); // hblank STAT source
        b.write(0xFF40, 0x91);
        // Same geometry as `m0_rise_second_half_commit_is_halt_late`:
        // line-1 mode-0 flip at 452 + 254 + SCX%8.
        let rise = 452 + 254 + u32::from(scx);
        ticks(&mut b, rise.div_ceil(4) - 1);
        b.tick(); // the M-cycle whose flip lands the unblock
        assert_eq!(b.intf & 0x02, 0x02, "scx {scx}: m0 IRQ dispatch-visible");
        assert!(!b.ppu.oam_read_blocked(), "scx {scx}: end view unblocked");
        assert_eq!(
            b.read_no_tick(0xFE00),
            if second_half { 0xFF } else { 0x00 },
            "scx {scx}: cc+2 MID OAM read"
        );
        b.tick();
        assert_eq!(
            b.read_no_tick(0xFE00),
            0x00,
            "scx {scx}: unblocked next cycle"
        );
    }
}

/// VRAM unblocks on the same mode-3→mode-0 edge as OAM, so a CPU VRAM
/// read is held at the cc+2 MID phase the same way (sub-dot event-phase
/// model, increment 2): a second-half unblock reads $FF even though the
/// end view is already accessible. Pins gambatte `vram_m3/postread_*`.
#[test]
fn vram_read_holds_blocked_at_cc2_through_a_second_half_unblock() {
    for (scx, second_half) in [(0u8, false), (1, true)] {
        let mut b = ic(Model::Dmg);
        b.write(0xFF43, scx);
        b.write(0xFF41, 0x08);
        b.write(0xFF40, 0x91);
        let rise = 452 + 254 + u32::from(scx);
        ticks(&mut b, rise.div_ceil(4) - 1);
        b.tick();
        assert!(!b.ppu.vram_read_blocked(), "scx {scx}: end view unblocked");
        let direct = b.ppu.read(0x8000); // bypasses the MID override
        let via = b.read_no_tick(0x8000);
        if second_half {
            assert_eq!(via, 0xFF, "scx {scx}: cc+2 MID VRAM read still blocked");
        } else {
            assert_eq!(via, direct, "scx {scx}: first-half unblock is accessible");
        }
    }
}

/// A CPU VRAM write is locked out at the cc+2 MID phase the same way
/// the read is (sub-dot event-phase model): a second-half mode-3→mode-0
/// unblock drops the write (still mode 3 at cc+2), while a first-half
/// unblock lets it land. Pins gambatte `vramw_m3end_*`.
#[test]
fn vram_write_dropped_at_cc2_through_a_second_half_unblock() {
    for (scx, second_half) in [(0u8, false), (1, true)] {
        let mut b = ic(Model::Dmg);
        b.write(0xFF43, scx);
        b.write(0xFF41, 0x08);
        b.write(0xFF40, 0x91);
        let rise = 452 + 254 + u32::from(scx);
        ticks(&mut b, rise.div_ceil(4) - 1);
        b.tick();
        let before = b.ppu.vram_read_raw(0x8000);
        let probe = before ^ 0xFF; // a value distinct from the current byte
        b.write_no_tick(0x8000, probe);
        let after = b.ppu.vram_read_raw(0x8000);
        if second_half {
            assert_eq!(after, before, "scx {scx}: cc+2 MID write dropped");
        } else {
            assert_eq!(after, probe, "scx {scx}: first-half write landed");
        }
    }
}

/// The CGB FF69/FF6B palette read is held at the cc+2 MID phase: the
/// pipe-end palette unblock commits at the M-cycle END (`PalAccess` =
/// phase 8, the whole-M-cycle block — INC-G3 task 5), so the CPU read still
/// returns $FF for the entire straddle M-cycle even though the PPU's own
/// (end-view) palette RAM has unlocked. Pins gambatte `cgbpal_m3` (the
/// pipe-end-anchored read; population + end-to-end correctness validated by
/// that suite). Anchored at `render_finished`, one dot after the m0 flip —
/// see `Ppu::pal_access_flip`.
#[test]
fn cgb_palette_read_mid_override_returns_ff() {
    let mut b = ic(Model::Cgb);
    // The palette unblock commits at the M-cycle end (`END_PHASE` =
    // `event_phase(PalAccess, ..)`), so a MID (cc+2) observer is blocked.
    b.pal_access_edge = Some(END_PHASE);
    assert_eq!(
        b.read_no_tick(0xFF69),
        0xFF,
        "BG palette read locked at cc+2 MID"
    );
    assert_eq!(
        b.read_no_tick(0xFF6B),
        0xFF,
        "OBJ palette read locked at cc+2 MID"
    );
    // The stamp is reset every machine tick (only the straddle M-cycle
    // carries it); a normal read goes to the PPU.
    b.tick();
    assert_eq!(b.pal_access_edge, None, "stamp cleared by the next tick");
}

/// In double speed the FF41 mode bits read at the cc+2 MID phase: when a
/// sprite-line mode-3→mode-0 flip fired anywhere in the straddle M-cycle the
/// STAT read still shows the old mode 3, even though the PPU's whole-dot end
/// view has flipped to mode 0 (gambatte sprites m3stat_ds). INC-G3 task 6
/// promotes the block to the WHOLE M-cycle (`event_phase(StatMode)=END_PHASE`,
/// like the palette block): a flip on the M-cycle's FIRST dot — which the
/// INC-DS-1 dot-END half-split left readable as mode 0 — now also holds
/// mode 3 (the +84 residual `m3stat_ds_1` rows whose flip lands in the first
/// half). Single speed keeps the end view (the parked multi-chain STAT-mode
/// read; cross-oracle, see `stat_mode_override_requires_double_speed`).
#[test]
fn stat_mode_read_forces_mode3_whole_mcycle_in_double_speed() {
    let mut b = ic(Model::Cgb);
    // LCD on (the override is LCD-gated) but still in the post-enable
    // glitch line's early dots, so the end-view mode bits are 0 and the
    // override is the only source of the mode bits.
    b.write(0xFF40, 0x80);
    assert!(b.ppu.lcd_enabled());
    let base = b.ppu.read(0xFF41) & 0x03;
    assert_eq!(base, 0, "end-view mode bits are 0");
    // A flip on the M-cycle's FIRST double-speed dot (cc=2): the INC-DS-1
    // dot-END eighth was `cc_eighth(2)=4` = the M-cycle midpoint, which the
    // cc+2 MID observer does NOT precede (the half-split left it readable as
    // mode 0). `event_phase(StatMode)` now returns END_PHASE, so the whole
    // straddle M-cycle blocks regardless of which cc the flip lands on.
    b.stat_mode_edge = Some(event_phase(EdgeKind::StatMode, 2));
    assert_eq!(
        b.stat_mode_edge,
        Some(END_PHASE),
        "StatMode stamps the whole-M-cycle END phase"
    );
    // Single speed: the override is gated off, the end view (mode 0)
    // shows through.
    b.double_speed = false;
    assert_eq!(b.read_no_tick(0xFF41) & 0x03, 0, "SS keeps the end view");
    // Double speed: even a first-dot flip now reads the old mode 3.
    b.double_speed = true;
    assert_eq!(
        b.read_no_tick(0xFF41) & 0x03,
        3,
        "DS whole-M-cycle reads mode 3"
    );
    // Only the low mode bits move — the rest of STAT keeps the end view.
    assert_eq!(b.read_no_tick(0xFF41) & !0x03, b.ppu.read(0xFF41) & !0x03);
    // The stamp is reset every machine tick (only the straddle carries it).
    b.tick();
    assert_eq!(b.stat_mode_edge, None, "stamp cleared by the next tick");
}

/// INC-G3 task 6/7 ceiling pin: the `double_speed` gate on the FF41
/// STAT-mode override is load-bearing, NOT belt-and-braces. The same
/// `stat_mode_edge` stamp serves single- and double-speed reads (the
/// dot-loop is speed-agnostic past `dots`), but they want OPPOSITE results:
/// the double-speed sprite `m3stat_ds` reads want the held mode 3 (lifted),
/// while the single-speed `m3stat` direct-poll reads (enable_display,
/// sprite-count, m0int/ine_m3stat) and the m2-dispatch FF41/FF0F chains want
/// the mode-0 end view. Relaxing the gate measured LIFT 0 / REGRESS 30
/// (task 6 single-speed probe). The single discriminator that would separate
/// them is a per-read-chain CPU↔bus sub-cc phase, and the IF-flop set/read
/// race that implements it (task 7, intr_2_mode0_nops / m2int_m0irq) is
/// cross-oracle irreducible — gbmicrotest hblank_int_scx*_if pins the very
/// dots gambatte's m2int reads contradict, and the G2c ack-countdown tag
/// that lifted the gambatte side broke the canonical `intr_2_mode0_timing`
/// mooneye test. So the gate stays speed-conditioned; this test fails the
/// moment someone drops `&& self.double_speed`.
#[test]
fn stat_mode_override_requires_double_speed() {
    let mut b = ic(Model::Cgb);
    b.write(0xFF40, 0x80);
    assert!(b.ppu.lcd_enabled());
    let end_view = b.ppu.read(0xFF41) & 0x03;
    assert_eq!(end_view, 0, "end-view mode bits are 0");
    // A live sprite-line whole-M-cycle stamp (set identically for both
    // speeds — the same value the cc-loop stamps).
    b.stat_mode_edge = Some(event_phase(EdgeKind::StatMode, 2));
    // Single speed: the override is gated off — the read shows the unforced
    // PPU end view, matching the green single-speed m3stat direct polls.
    b.double_speed = false;
    assert_eq!(
        b.read_no_tick(0xFF41) & 0x03,
        end_view,
        "SS read keeps the end view (the override must not fire)"
    );
    // Double speed: the same stamp forces the held mode 3.
    b.double_speed = true;
    assert_eq!(
        b.read_no_tick(0xFF41) & 0x03,
        3,
        "DS read holds mode 3 (the override fires)"
    );
}

/// The cc-granular reclock's `dot_phase` starts at 0 — the fixed even-cc
/// {2,4} double-speed alignment the old dot loop baked in — so a fresh
/// interconnect (no speed switch yet) is bit-identical to the dot loop. A
/// speed switch is what sets it to the half-dot offset (the next increment).
#[test]
fn dot_phase_defaults_zero() {
    for model in [Model::Dmg, Model::Cgb] {
        assert_eq!(ic(model).dot_phase, 0, "{model:?}: dot_phase starts at 0");
    }
}

/// The eighth-grid sub-cc phase comparator (`obs_pre_edge` /
/// `edge_eighth`) is a bit-exact reframe of the old `2 * (i + 1) > dots`
/// half-split, expressing each per-M-cycle event/observer timing in
/// eighths of an M-cycle (8 eighths = 4 cc; MID = cc+2, END = cc+4). The
/// reframe lifts ZERO rows by itself — its worth is converting the parked
/// multi-chain CPU↔PPU read-phase problem from one boolean (which
/// conflates *when the edge committed* with *the single cc+2 observer*)
/// into a comparison where later increments can give an edge its own
/// sub-dot commit offset and a read chain its own sampling phase. This
/// test pins the equivalence the scaffold MUST preserve: every green floor
/// row still rides on the legacy boolean.
#[test]
fn eighth_grid_predicate_matches_half_split() {
    assert_eq!(MID_PHASE, 4, "cc+2 observer = M-cycle midpoint (eighths)");
    // The dot-END commit eighth of an edge firing on dot `i`; the last
    // dot commits at 8 eighths = cc+4, the full M-cycle (tick-then-access).
    let ss: Vec<u8> = (0..4u64).map(|i| edge_eighth(i, 4)).collect();
    assert_eq!(ss, [2, 4, 6, 8], "single speed: dot-end eighths");
    let ds: Vec<u8> = (0..2u64).map(|i| edge_eighth(i, 2)).collect();
    assert_eq!(ds, [4, 8], "double speed: dot-end eighths");
    // Bit-identical to the legacy half-split for every dot of both speeds:
    // an observer sampling at MID precedes (is blocked by) an edge whose
    // dot-end commit eighth exceeds MID.
    for dots in [2u64, 4] {
        for i in 0..dots {
            assert_eq!(
                obs_pre_edge(MID_PHASE, edge_eighth(i, dots)),
                2 * (i + 1) > dots,
                "dots={dots} i={i}"
            );
        }
    }
}

/// The cc-granular tick grid (`dot_ticks_on_cc` + `cc_eighth`) reproduces the
/// old `for i in 0..dots` dot loop exactly at `dot_phase` 0: the cc's (1..=4)
/// that tick a whole PPU dot, in order, stamp the same eighths
/// `edge_eighth(i, dots)` the loop produced for i in 0..dots. This is the
/// net-zero proof for the cc-granular reclock foundation — phase 0 is
/// bit-identical to the fixed-alignment loop. `dot_phase` 1 (double speed
/// only) ticks the complementary odd cc's, stamping the new odd-cc eighths
/// {2,6} the whole-dot fixed loop could never place — the half-dot offset a
/// STOP speed switch establishes (the LCD dot clock runs on across the switch).
#[test]
fn cc_grid_matches_dot_loop() {
    for (ds, dots) in [(false, 4u64), (true, 2)] {
        let cc_eighths: Vec<u8> = (1..=4u8)
            .filter(|&cc| dot_ticks_on_cc(cc, ds, 0))
            .map(cc_eighth)
            .collect();
        let loop_eighths: Vec<u8> = (0..dots).map(|i| edge_eighth(i, dots)).collect();
        assert_eq!(
            cc_eighths, loop_eighths,
            "ds={ds}: phase-0 cc grid == dot loop"
        );
    }
    // Double-speed phase 1 ticks the complementary odd cc's {1,3}, stamping
    // the odd-cc eighths {2,6} the fixed even-cc {4,8} loop could never reach.
    let p1: Vec<u8> = (1..=4u8)
        .filter(|&cc| dot_ticks_on_cc(cc, true, 1))
        .map(cc_eighth)
        .collect();
    assert_eq!(p1, [2, 6], "double speed phase 1: odd-cc eighths");
    // Single speed ignores the phase (one dot per cc regardless): 4 dots.
    assert_eq!(
        (1..=4u8)
            .filter(|&cc| dot_ticks_on_cc(cc, false, 1))
            .count(),
        4,
        "single speed is phase-independent"
    );
}

/// The `Option<u8>` edge-stamp (INC-G2a) must reproduce the legacy
/// precomputed boolean exactly: for an edge firing on dot `i`, a MID
/// observer is blocked iff the old `2 * (i + 1) > dots` half-split was
/// true, and an unstamped (`None`) M-cycle never blocks. This is the
/// net-zero proof — every green floor row still rides on it.
#[test]
fn stamp_blocks_matches_half_split() {
    for dots in [2u64, 4] {
        for i in 0..dots {
            assert_eq!(
                stamp_blocks(Some(edge_eighth(i, dots)), MID_PHASE),
                2 * (i + 1) > dots,
                "dots={dots} i={i}"
            );
        }
    }
    assert!(
        !stamp_blocks(None, MID_PHASE),
        "no edge this M-cycle never blocks"
    );
}

/// `event_phase` generalizes `edge_eighth` over an [`EdgeKind`] so a lift
/// can give one boundary event its own sub-dot offset (INC-G3). The
/// OAM/VRAM accessibility unblock (`M0Access`) and the halt-exit mode-0
/// rise (`M0Rise`) still ride the legacy dot-END commit eighth, for both
/// speeds and every dot — the scaffold stays net-zero for those edges. Both
/// calibrated kinds — `PalAccess` (task 5) and `StatMode` (task 6) — commit
/// at the whole-M-cycle END phase instead (their own assertions below).
#[test]
fn event_phase_net_zero_except_pal_and_stat() {
    // The dot-clocked kinds commit at their cc's `cc_eighth` for every cc on
    // the 1..=4 grid — the cc-granular net-zero seam (the cc already carries
    // the `dot_phase` sub-dot offset, so there is no `i`/`dots` parameter).
    for kind in [EdgeKind::M0Rise, EdgeKind::M0Access] {
        for cc in 1..=4u8 {
            assert_eq!(
                event_phase(kind, cc),
                cc_eighth(cc),
                "kind={kind:?} cc={cc}"
            );
        }
    }
    // The two calibrated whole-M-cycle blocks commit at END regardless of cc
    // (PalAccess: task 5; StatMode: task 6 — the double-speed sprite m3stat_ds
    // block, lifted from the dot-END half-split so a 1st-half flip also holds
    // the old mode 3).
    for kind in [EdgeKind::PalAccess, EdgeKind::StatMode] {
        for cc in 1..=4u8 {
            assert_eq!(event_phase(kind, cc), END_PHASE, "kind={kind:?} cc={cc}");
        }
    }
}

/// INC-G3 task 5: the CGB palette-RAM unblock commits at the M-cycle END
/// (phase 8 = cc+4), one observer grid later than OAM/VRAM's dot-split, so
/// a cc+2 [`ACCESS_PHASE`] FF69/FF6B read stays blocked ($FF) for the WHOLE
/// straddle M-cycle regardless of which dot lx==160 lands on — readable
/// only next M-cycle. The half-split under-blocked the 1st-half (scx2/scx5)
/// geometries that gambatte cgbpal_m3end `scx2_1`/`scx5_1`/`scx5_ds_1`
/// (out7) pin (+3 floor rows, zero cross-suite regression).
#[test]
fn pal_access_blocks_whole_mcycle() {
    for cc in 1..=4u8 {
        let e = event_phase(EdgeKind::PalAccess, cc);
        // Pin the exact commit phase, not just "blocks": the unblock commits
        // at the M-cycle END regardless of which cc lx==160 lands on.
        assert_eq!(e, END_PHASE, "palette commits at M-cycle end: cc={cc}");
        assert!(
            stamp_blocks(Some(e), ACCESS_PHASE),
            "palette blocked at MID for every cc: cc={cc} e={e}"
        );
    }
}

/// INC-G3 net-zero scaffold, task 4: every CPU bus access samples the edge
/// stamps at one fixed phase ([`ACCESS_PHASE`]) — correcting the reverted
/// G2c per-read-chain `obs_phase(addr)`. The single constant equals
/// [`MID_PHASE`] (cc+2), which is what keeps the scaffold net-zero; the
/// read chains are later separated by the EVENT's sub-dot position
/// ([`event_phase`]), not the observer's.
#[test]
fn access_phase_is_single_constant() {
    assert_eq!(
        ACCESS_PHASE, MID_PHASE,
        "the one CPU-access observer phase is the cc+2 midpoint (net-zero)"
    );
}
