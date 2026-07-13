//! The eighth-grid sub-cc phase model: the free helpers + `EdgeKind` that the
//! `tick`/`memory` submodules use to place PPU boundary events and CPU-access
//! observers at sub-M-cycle phases. No `impl` block — pure functions/consts.
//! Re-exported into the parent via `use phase::*;` (see `interconnect.rs`), so
//! sibling submodules keep resolving these through their `use super::*`.

/// Eighth-grid sub-cc phase model. An M-cycle spans 4 cc = 8 *eighths*; PPU
/// events commit and CPU observers sample at sub-cc phases within it.
/// `MID_PHASE` is the cc+2 observer phase (the M-cycle midpoint a
/// tick-then-access read effectively samples at — gambatte's access offset
/// two dots before our cc+4 end-sampled view, which is the full M-cycle = 8
/// eighths). See [`edge_eighth`] / [`obs_pre_edge`] and the dot-loop in
/// [`Interconnect::tick_machine`].
pub(super) const MID_PHASE: u8 = 4;

/// The M-cycle END phase (cc+4 = 8 eighths) — [`edge_eighth`]'s last-dot value
/// for both speeds. An event committing here is past every observer (it blocks
/// the whole M-cycle and is visible only next M-cycle); the CGB palette unblock
/// commits here (`event_phase(EdgeKind::PalAccess, ..)`).
pub(super) const END_PHASE: u8 = 8;

/// The dot-END commit phase (in eighths of an M-cycle) of an event that
/// fired on dot `i` of a `dots`-dot M-cycle (`dots` = 4 single speed / 2
/// double speed). Single speed → {2,4,6,8}; double speed → {4,8}. The
/// edge commits at the end of its dot, so a later increment adds a small
/// negative offset (e.g. −1 eighth) to model an edge that leads the dot end.
#[inline]
pub(super) fn edge_eighth(i: u64, dots: u64) -> u8 {
    // `dots` is the PPU-dots-per-M-cycle, structurally 4 (single speed) or 2
    // (double speed); the eighth table {2,4,6,8}/{4,8} relies on it.
    debug_assert!(dots == 2 || dots == 4, "dots must be 2 or 4, got {dots}");
    ((i + 1) * 8 / dots) as u8
}

/// The commit eighth (of 8 per M-cycle) of an event on the dot that ticks at
/// cc `cc` (1..=4). The cc grid IS the single-speed dot grid — cc is the
/// single-speed dot index + 1 — so the eighth is the single-speed dot-END
/// [`edge_eighth`]: `cc*2` → {2,4,6,8}. Double speed selects a 2-cc subset of
/// these per [`dot_ticks_on_cc`] (phase 0 → the even cc, eighths {4,8} = today;
/// phase 1 → the odd cc, eighths {2,6} = the half-dot offset the whole-dot loop
/// could never place). At `dot_phase` 0 the dot-tick cc's reproduce
/// [`edge_eighth`]'s per-`i` sequence exactly (`cc_grid_matches_dot_loop`).
#[inline]
pub(super) fn cc_eighth(cc: u8) -> u8 {
    debug_assert!((1..=4).contains(&cc), "cc must be 1..=4, got {cc}");
    edge_eighth(u64::from(cc) - 1, 4)
}

/// Whether an observer sampling at phase `obs` (eighths) precedes the event
/// committing at phase `edge` — i.e. the observer sees the pre-commit state.
/// For accessibility/STAT reads that means "still blocked / pre-flip"; for
/// the mode-0 IRQ rise it means "the halt-exit sampler misses the rise this
/// M-cycle". Bit-identical to the legacy `2 * (i + 1) > dots` half-split when
/// `obs == MID_PHASE` (see `eighth_grid_predicate_matches_half_split`).
#[inline]
pub(super) fn obs_pre_edge(obs: u8, edge: u8) -> bool {
    obs < edge
}

/// Whether a CPU read/write observing at phase `obs` (eighths) is still
/// blocked by a per-M-cycle accessibility/STAT edge stamped at its dot-END
/// commit eighth (`Some(edge)` from [`edge_eighth`]; `None` = no edge this
/// M-cycle). The edge-stamp replaces the old precomputed boolean: storing the
/// raw commit eighth (rather than `obs_pre_edge(MID_PHASE, edge)`) is what lets
/// an EVENT carry its own sub-dot position via [`event_phase`] — the
/// discriminator between read chains, since every CPU access observes at the
/// one [`ACCESS_PHASE`] (a per-read-chain observer phase was the wrong
/// premise). `stamp_blocks(Some(edge), MID_PHASE)` is bit-identical to
/// the legacy half-split for every dot/speed (`stamp_blocks_matches_half_split`).
#[inline]
pub(super) fn stamp_blocks(stamp: Option<u8>, obs: u8) -> bool {
    stamp.is_some_and(|edge| obs_pre_edge(obs, edge))
}

/// The boundary events that commit a per-M-cycle sub-cc edge. Each PPU edge
/// commits at its own dot-END eighth today ([`event_phase`] returns
/// [`edge_eighth`] for every kind — net-zero), so the kinds are
/// interchangeable; the enum is the seam a later increment uses to give
/// one event its own sub-dot lead/lag (the cc-exact boundary positions from
/// the gambatte xpos formulas — e.g. the CGB palette unblock trails the mode-0
/// IRQ rise by a half-dot, m0Time=xpos+7 vs IRQ+6) without recalibrating the
/// dot-clocked pixel pipe or the other events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EdgeKind {
    /// The mode-0 STAT IRQ rise (consumed by the halt-exit sampler, not stamped).
    M0Rise,
    /// The OAM/VRAM mode-3→mode-0 accessibility unblock (`m0_access_edge`).
    M0Access,
    /// The CGB palette-RAM pipe-end unblock (`pal_access_edge`).
    PalAccess,
    /// The double-speed FF41 STAT mode-bit flip (`stat_mode_edge`); commits at
    /// the whole-M-cycle END phase, like `PalAccess`.
    StatMode,
}

/// The commit phase (eighths of an M-cycle) of boundary event `kind` on the
/// dot that ticks at cc `cc` (1..=4 — see [`dot_ticks_on_cc`]), shifted by a
/// per-event `lead_eighths` sub-dot offset (signed; positive = commit later,
/// negative = earlier). Most kinds commit at their dot-END eighth
/// ([`cc_eighth`]); `PalAccess`/`StatMode` at the M-cycle END.
/// `lead_eighths` is the eighth-grid reclock hook: at `lead_eighths == 0`
/// the result is identical to the pre-reclock fixed phase (net-zero —
/// `event_phase_lead_zero_is_identity`); a non-zero lead lets one event carry
/// its own sub-dot commit position (e.g. the per-SCX CGB palette unblock) WITHOUT
/// moving the whole-dot pixel pipe. The result is clamped to `0..=END_PHASE`:
/// `0` never blocks an `ACCESS_PHASE` observer, `END_PHASE` blocks the whole
/// straddle M-cycle (the stamp resets each tick, so a cross-M-cycle lead is
/// indistinguishable from `END_PHASE`).
#[inline]
pub(super) fn event_phase(kind: EdgeKind, cc: u8, lead_eighths: i8) -> u8 {
    let base = match kind {
        // The CGB palette-RAM unblock commits at the M-cycle END (phase 8 =
        // cc+4), one observer grid later than OAM/VRAM's dot-split: a cc+2 MID
        // FF69/FF6B read stays blocked for the ENTIRE straddle M-cycle and reads
        // $FF until the next M-cycle, regardless of which dot lx==160 lands on.
        // The dot-split half-classification under-blocked the geometries where
        // lx==160 falls in the M-cycle's first half — gambatte cgbpal_m3end
        // scx2_1/scx5_1/scx5_ds_1 (out7) pin the late effect across SCX. The
        // palette unblock physically lags the pixel-pipe end (gambatte
        // cgbpAccessible vs m0Time), so it gets the whole-M-cycle block where
        // OAM/VRAM only get the second half.
        EdgeKind::PalAccess => END_PHASE,
        // The double-speed FF41 STAT mode-bit block also commits at the
        // M-cycle END: a sprite-line m3→m0 flip anywhere in the
        // straddle M-cycle holds the cc+2 read at the old mode 3, not only a
        // 2nd-half flip. The earlier dot-END half-split caught the +43 rows
        // whose flip lands in the M-cycle's second half; promoting StatMode to
        // the whole-M-cycle block lifts the +84 residual `m3stat_ds_1` rows
        // whose flip lands in the FIRST half (gambatte sprites). The full-gbtr
        // ratchet measured +84/−3 (net floor −84): the only regressions are the
        // 3 `late_sizechange_sp00/01/39_ds_1` (out0, want mode 0) — a net-neutral
        // in-cluster A/B swap, since their `_ds_2` siblings (out3) are in the
        // lift. Whole-M-cycle forces both the size-change `_1` and `_2` reads on
        // the straddle line to mode 3; the `_2` want it, the `_1` do not, and no
        // `event_phase` offset separates two reads in the same M-cycle (the
        // parked multi-chain CPU↔PPU phase problem). Taken on the half-dot-grid
        // branch (net-positive trades OK); see the swap note in
        // tests/gbtr/baselines/gambatte.txt.
        EdgeKind::StatMode => END_PHASE,
        // Every other event commits at its dot-END eighth.
        _ => cc_eighth(cc),
    };
    (i16::from(base) + i16::from(lead_eighths)).clamp(0, i16::from(END_PHASE)) as u8
}

/// The single sub-cc phase (eighths) at which every CPU bus access samples the
/// accessibility/STAT edge stamps. M-cycles are dot-aligned to the PPU, so all
/// CPU accesses sample at the SAME M-cycle cc-offset — the discriminator
/// between read chains is the EVENT's sub-dot position ([`event_phase`]), not
/// the observer's (a per-read-chain observer phase was the wrong premise).
/// Equals [`MID_PHASE`] (cc+2), so this is net-zero
/// (`access_phase_is_single_constant`).
pub(super) const ACCESS_PHASE: u8 = MID_PHASE;
