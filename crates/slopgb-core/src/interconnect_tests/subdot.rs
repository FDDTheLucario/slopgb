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
    b.stat_mode_edge = Some(event_phase(EdgeKind::StatMode, 2, 0));
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
    b.stat_mode_edge = Some(event_phase(EdgeKind::StatMode, 2, 0));
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

/// R3 closure pin — the single-speed FF41 STAT mode-bit read at cc+2 MID stays
/// CLOSED on BOTH models (CC-RECLOCK Phase-0 + R3, ppu-subdot-ladder.md). The
/// abandoned R3 override (force the FF41 mode bits to 3 when a bare-line
/// mode-3→mode-0 flip lands in the read M-cycle's second half, reusing the
/// `m0_access_edge` half-split, the single-speed analog of the double-speed
/// `stat_mode_edge` override) was re-measured + classified END-TO-END this
/// session and is NOT shippable on either model. The CGB slice is a −22
/// read-site swap — class-A `lcd_offset` / class-B `speedchange` / window /
/// display-start flip-POSITION errors our CGB `m0_flip_events` projection gets
/// wrong (the out-of-scope pixel-pipe reclock, gambatte.txt class-A header),
/// with no read-site discriminator separating the few clean CGB lifts from
/// those regressors. The DMG slice looked like a clean gambatte +4/−1, but it
/// is the CROSS-ORACLE swap the project rejected — it lifts +16 wilbertpol
/// `intr_2_mode0_*_nops` (class-E 2016-era expectations) while REGRESSING 4
/// currently-green gbmicrotest `ppu_sprite0_scx{1,2,5,6}_b` [Dmg] (class-H
/// one-dot conflicts the mode-0 grid pins — "don't chase one-sidedly",
/// gbmicrotest.txt header), i.e. dropping SameBoy-aligned hardware rows to gain
/// the rejected wilbertpol side, which violates the oracle policy (CLAUDE.md:
/// never drop a test SameBoy passes).
///
/// So the single-speed FF41 read keeps the PPU end view on every model no matter
/// which sub-dot edge is stamped this M-cycle — only the double-speed StatMode
/// override (pinned above) ever moves the mode bits. Fails the moment any
/// single-speed FF41 sub-dot override (e.g. via `m0_access_edge`) is introduced.
#[test]
fn single_speed_ff41_keeps_end_view_under_every_edge() {
    for model in [Model::Dmg, Model::Cgb] {
        let mut b = ic(model);
        b.write(0xFF40, 0x80);
        assert!(b.ppu.lcd_enabled(), "{model:?}: lcd on");
        b.double_speed = false;
        let end_view = b.ppu.read(0xFF41) & 0x03;
        // The end view must NOT already be mode 3, or this would pass vacuously
        // while an override (which forces 3) was live.
        assert_ne!(end_view, 3, "{model:?}: end view differs from the forced 3");
        // Stamp all three sub-dot edges live + blocking: a second-half
        // `M0Access` (cc 3 → eighth 6 > MID 4) + the whole-M-cycle
        // `PalAccess`/`StatMode` END-phase edges. Each WOULD block its own
        // consumer (OAM/VRAM, palette RAM, the double-speed FF41 read).
        b.m0_access_edge = Some(event_phase(EdgeKind::M0Access, 3, 0));
        b.pal_access_edge = Some(event_phase(EdgeKind::PalAccess, 2, 0));
        b.stat_mode_edge = Some(event_phase(EdgeKind::StatMode, 2, 0));
        assert!(
            stamp_blocks(b.m0_access_edge, ACCESS_PHASE),
            "{model:?}: edge is live"
        );
        assert_eq!(
            b.read_no_tick(0xFF41) & 0x03,
            end_view,
            "{model:?}: single-speed FF41 keeps the end view (R3 stays closed)"
        );
    }
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
                event_phase(kind, cc, 0),
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
            assert_eq!(event_phase(kind, cc, 0), END_PHASE, "kind={kind:?} cc={cc}");
        }
    }
}

/// The eighth-grid reclock hook (INC-G3 / pixel-pipe reclock S0): a non-zero
/// `lead_eighths` shifts an event's commit phase by that many eighths (signed —
/// negative pulls it earlier toward unblock, positive later), clamped to
/// `0..=END_PHASE`. This is the per-event sub-dot offset the reclock lifts use
/// (e.g. the per-SCX CGB palette unblock — S2) WITHOUT moving the whole-dot
/// pixel pipe; `lead_eighths == 0` is the net-zero identity already pinned by
/// `event_phase_net_zero_except_pal_and_stat`. The clamp keeps the result a
/// valid phase: `0` never blocks an `ACCESS_PHASE` observer, `END_PHASE` blocks
/// the whole straddle M-cycle (the stamp resets each tick, so a larger lead is
/// indistinguishable from `END_PHASE`).
#[test]
fn event_phase_lead_shifts_and_clamps() {
    // cc_eighth(4) == 8: a negative lead pulls a dot-END kind earlier.
    assert_eq!(event_phase(EdgeKind::M0Access, 4, 0), 8);
    assert_eq!(event_phase(EdgeKind::M0Access, 4, -2), 6);
    assert_eq!(event_phase(EdgeKind::M0Access, 4, -4), 4);
    // Clamp below 0 (cc_eighth(1) == 2; 2 - 8 -> 0) and above END (8 + 4 -> 8).
    assert_eq!(event_phase(EdgeKind::M0Access, 1, -8), 0);
    assert_eq!(event_phase(EdgeKind::PalAccess, 1, 4), END_PHASE);
    // A negative lead pulls the whole-M-cycle PalAccess off END.
    assert_eq!(event_phase(EdgeKind::PalAccess, 1, -1), 7);
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
        let e = event_phase(EdgeKind::PalAccess, cc, 0);
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

// ---- S1 deferred-commit CPU clock wiring (net-zero) --------------------
//
// Every CPU-driven M-cycle (the five `Bus` access methods) drives the
// `CycleClock`; the instruction boundary `flush_pending` drains it. The
// clock is write-only scaffold today (nothing samples it), so its only
// observable property is conservation: after a boundary flush its committed
// position equals 4 T-cycles × the M-cycle count, in either speed. This pins
// the wiring; the clock's own arithmetic is unit-tested in `cycle_clock`.

#[test]
fn cpu_clock_deferred_commit_conserves_t_count() {
    let mut b = ic(Model::Dmg);
    // A read latches at the M-cycle leading edge (cc+0) and parks its own 4.
    assert_eq!(b.read(0xFF80), 0, "fresh HRAM");
    assert_eq!((b.cpu_clock_t(), b.cpu_clock_pending()), (0, 4));
    // An internal cycle parks +4 without committing.
    b.tick();
    assert_eq!((b.cpu_clock_t(), b.cpu_clock_pending()), (0, 8));
    // The next access commits the 8 parked T-cycles, then parks 4.
    b.read(0xFF80);
    assert_eq!((b.cpu_clock_t(), b.cpu_clock_pending()), (8, 4));
    // A plain write (Conflict::ReadOld) commits at the leading edge, reparks 4.
    b.write(0xFF80, 0);
    assert_eq!((b.cpu_clock_t(), b.cpu_clock_pending()), (12, 4));
    b.tick();
    assert_eq!((b.cpu_clock_t(), b.cpu_clock_pending()), (12, 8));
    // The instruction boundary drains the debt.
    b.flush_pending();
    // 5 M-cycles executed → 20 CPU T-cycles, debt fully drained.
    assert_eq!((b.cpu_clock_t(), b.cpu_clock_pending()), (20, 0));
    assert_eq!(b.cpu_clock_t(), 4 * 5);
}

#[test]
fn cpu_clock_read_inc_and_tick_addr_drive_the_clock() {
    // Both read_inc and tick_addr are leading-edge cycles (cycle_read /
    // cycle_oam_bug): each commits the prior debt and reparks 4.
    let mut b = ic(Model::Dmg);
    b.read_inc(0xFF80); // leading-edge read: commit 0, park 4
    b.tick_addr(0x0000); // cycle_oam_bug: commit 4, park 4
    assert_eq!((b.cpu_clock_t(), b.cpu_clock_pending()), (4, 4));
    b.flush_pending();
    assert_eq!(b.cpu_clock_t(), 8, "2 M-cycles = 8 T");
    assert_eq!(b.cpu_clock_pending(), 0);
}

#[test]
fn cpu_clock_write_first_conserves_with_no_parked_debt() {
    // A standalone first write (no preceding fetch → pending==0) is the case
    // the production `write` relaxed from a panic to a saturating commit:
    // ReadOld commits at the current clock (no advance) and reparks 4, still
    // conserving the per-M-cycle 4 T. Pins "no panic + conserves".
    let mut b = ic(Model::Dmg);
    b.write(0xFF80, 0); // pending was 0
    assert_eq!((b.cpu_clock_t(), b.cpu_clock_pending()), (0, 4));
    b.flush_pending();
    assert_eq!(b.cpu_clock_t(), 4, "one M-cycle = 4 T even when the write is first");
}

#[test]
fn cpu_clock_write_routes_through_the_per_model_conflict_class() {
    // `Bus::write` selects the SameBoy conflict class via `write_conflict`
    // (A3, byte-identical because the commit position is still discarded).
    // The class's re-park is observable on the deferred-commit clock: a
    // fetch parks 4, then the write re-parks per class. (Same pattern as
    // `cpu_clock_deferred_commit_conserves_t_count`; here we vary the address
    // so the class — not just `ReadOld` — drives the re-park.)
    let repark = |model: Model, addr: u16| {
        let mut b = ic(model);
        b.read(0xFF80); // fetch: parks 4
        b.write(addr, 0);
        b.cpu_clock_pending()
    };
    // (model, addr, repark, class). DMG map (sm83_cpu.c:56) vs CGB
    // (sm83_cpu.c:31) differ on LYC (ReadOld vs WriteCpu) and SCX (SCX vs
    // ReadOld) — pinning that the lookup keys on the model, not the address.
    for (model, addr, want, class) in [
        (Model::Dmg, 0xFF80u16, 4u32, "HRAM ReadOld"),
        (Model::Dmg, 0xFF0F, 3, "DMG IF WriteCpu"),
        (Model::Dmg, 0xFF43, 6, "DMG SCX EarlyTwo"),
        (Model::Dmg, 0xFF40, 5, "DMG LCDC ReadNew"),
        (Model::Dmg, 0xFF4B, 3, "DMG WX WxHold"),
        (Model::Dmg, 0xFF45, 4, "DMG LYC ReadOld"),
        (Model::Cgb, 0xFF45, 3, "CGB LYC WriteCpu"),
        (Model::Cgb, 0xFF43, 4, "CGB SCX ReadOld"),
        // CGB LCDC's value-dependent tile-sel glitch is deferred to S6, so it
        // stays ReadOld(4) here — pins the documented deferral, not WxHold(3).
        (Model::Cgb, 0xFF40, 4, "CGB LCDC ReadOld S6"),
    ] {
        assert_eq!(repark(model, addr), want, "{class}");
    }
}

// ---- S2a leading-edge (cc+0) FF41 read ---------------------------------
//
// A leading-edge read latches FF41 at the M-cycle's *leading* edge (before
// the PPU advances), the slopgb equivalent of SameBoy force-syncing the PPU
// to the read's access cycle (`ppu-timing-map.md` §6 (i)). At a mode-3→0
// boundary M-cycle that reads mode 3, where today's trailing cc+4 view reads
// mode 0. The flag is held off in production (byte-identical); these tests
// drive it directly. It goes live only at the S2d atomic flip.

#[test]
fn leading_edge_ff41_reads_pre_tick_mode_at_the_mode0_boundary() {
    // Line-1 bare-line mode-0 geometry. The production IRQ-dispatch flip
    // (`line_render_done`) sits at our dot 254; the flag-on `vis_early`
    // back-dates the CPU-VISIBLE mode→0 boundary to SameBoy's 251 (3 dots
    // earlier), decoupled from the dispatch (`ppu/mod.rs` field docs). A
    // leading-edge (cc+0) read therefore separates by its sample dot.

    // Trailing cc+4 view (production default): the read's M-cycle ends past the
    // 254 boundary, so FF41 reads mode 0.
    let pos = (452 + 254u32).div_ceil(4) - 1;
    let mut b = ic(Model::Dmg);
    b.write(0xFF40, 0x91); // LCD + BG on
    ticks(&mut b, pos);
    assert_eq!(b.read(0xFF41) & 3, 0, "trailing cc+4 view reads mode 0");

    // Flag-on leading cc+0 + the vis_early back-date: a read whose leading edge
    // samples the m2int dot (248, before the back-dated 251 boundary) reads
    // mode 3; one sampling the m0int dot (252, at/after it) reads mode 0 — the
    // instrumented kernel-pair separation, at the two ROMs' actual read dots.
    let read_leading = |sample_dot: u32| -> u8 {
        // `read` samples FF41 at the M-cycle leading edge (before its tick), so
        // `pos` M-cycles place the sample on dot `pos * 4` (= 452 + sample_dot).
        let pos = (452 + sample_dot) / 4;
        let mut b = ic(Model::Dmg);
        b.write(0xFF40, 0x91);
        b.set_leading_edge_reads(true);
        ticks(&mut b, pos);
        b.read(0xFF41) & 3
    };
    assert_eq!(read_leading(248), 3, "m2int leading read (dot 248): mode 3");
    assert_eq!(read_leading(252), 0, "m0int leading read (dot 252): mode 0");
}

#[test]
fn leading_edge_routes_read_inc_too() {
    // `read_inc` (POP/RET-via-SP, LD A,(HL±)) is wired through the same
    // leading-edge sample as `read`, so an FF41 read_inc shows the same cc+0
    // separation across the back-dated visible boundary. (A regression that
    // forgot to route read_inc would read the trailing view on both.)
    let pos = (452 + 254u32).div_ceil(4) - 1;
    let mut b = ic(Model::Dmg);
    b.write(0xFF40, 0x91);
    ticks(&mut b, pos);
    assert_eq!(b.read_inc(0xFF41) & 3, 0, "read_inc trailing cc+4 view: mode 0");

    let read_inc_leading = |sample_dot: u32| -> u8 {
        let pos = (452 + sample_dot) / 4;
        let mut b = ic(Model::Dmg);
        b.write(0xFF40, 0x91);
        b.set_leading_edge_reads(true);
        ticks(&mut b, pos);
        b.read_inc(0xFF41) & 3
    };
    assert_eq!(
        read_inc_leading(248),
        3,
        "read_inc m2int leading (dot 248): mode 3"
    );
    assert_eq!(
        read_inc_leading(252),
        0,
        "read_inc m0int leading (dot 252): mode 0"
    );
}

#[test]
fn leading_edge_is_inert_off_boundary_and_for_non_ppu_reads() {
    // Mid-mode-3, far from any boundary: leading and trailing views agree, so
    // the flag changes nothing here.
    let settle = 452 + 120; // line 1, deep in mode 3
    let pos = settle / 4;
    for flag in [false, true] {
        let mut b = ic(Model::Dmg);
        b.write(0xFF40, 0x91);
        b.set_leading_edge_reads(flag);
        ticks(&mut b, pos);
        assert_eq!(b.read(0xFF41) & 3, 3, "flag {flag}: steady mode 3");
    }
    // A non-PPU read (HRAM) is never routed through the leading-edge path.
    for flag in [false, true] {
        let mut b = ic(Model::Dmg);
        b.set_leading_edge_reads(flag);
        b.write(0xFF80, 0xA5);
        assert_eq!(b.read(0xFF80), 0xA5, "flag {flag}: HRAM read unaffected");
    }
}
