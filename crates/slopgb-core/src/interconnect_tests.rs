//! Unit tests for the interconnect (memory map, DMA engines, IO routing,
//! sub-dot access machinery, speed switch). Split out of `interconnect.rs`
//! for file size; compiled as `super::tests` via the `#[path]` attribute.

use super::*;

/// 32 KiB no-MBC cart. `0x1000..0x1100` carries a recognisable pattern
/// for DMA source tests.
fn test_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    for i in 0..0x100usize {
        rom[0x1000 + i] = (i as u8) ^ 0x5A;
    }
    rom
}

fn ic(model: Model) -> Interconnect {
    Interconnect::new(model, Cartridge::from_bytes(test_rom()).unwrap())
}

fn ic_cgb_mode() -> Interconnect {
    let mut rom = test_rom();
    rom[0x143] = 0x80;
    Interconnect::new(Model::Cgb, Cartridge::from_bytes(rom).unwrap())
}

fn ticks(b: &mut Interconnect, n: u32) {
    for _ in 0..n {
        b.tick();
    }
}

// ---- memory map -----------------------------------------------------

#[test]
fn rom_reads_route_to_cartridge() {
    let mut b = ic(Model::Dmg);
    assert_eq!(b.read(0x1000), 0x5A);
    assert_eq!(b.read(0x1001), 0x5B);
}

#[test]
fn wram_and_echo_are_the_same_memory() {
    let mut b = ic(Model::Dmg);
    b.write(0xC000, 0x11);
    b.write(0xDDFF, 0x22);
    assert_eq!(b.read(0xE000), 0x11);
    assert_eq!(b.read(0xFDFF), 0x22);
    b.write(0xE123, 0x33);
    assert_eq!(b.read(0xC123), 0x33);
}

#[test]
fn hram_round_trips() {
    let mut b = ic(Model::Dmg);
    b.write(0xFF80, 0xAB);
    b.write(0xFFFE, 0xCD);
    assert_eq!(b.read(0xFF80), 0xAB);
    assert_eq!(b.read(0xFFFE), 0xCD);
}

#[test]
fn ie_stores_all_8_bits() {
    let mut b = ic(Model::Dmg);
    b.write(0xFFFF, 0xFF);
    assert_eq!(b.read(0xFFFF), 0xFF);
    b.write(0xFFFF, 0xE4);
    assert_eq!(b.read(0xFFFF), 0xE4);
}

#[test]
fn if_upper_three_bits_read_one() {
    let mut b = ic(Model::Dmg);
    b.write(0xFF0F, 0x00);
    assert_eq!(b.read(0xFF0F), 0xE0);
    b.write(0xFF0F, 0xFF);
    assert_eq!(b.read(0xFF0F), 0xFF);
    assert_eq!(b.pending(), 0); // IE = 0
    b.write(0xFFFF, 0x1F);
    assert_eq!(b.pending(), 0x1F);
    b.ack(0);
    assert_eq!(b.read(0xFF0F), 0xFE);
}

// ---- halt-exit IE & IF sampling (Bus::pending_halt_wake) ------------

/// Arm the timer so that the reload + IF commit lands on the last
/// T-substep of M-cycle 5 (div starts at 0, TAC bit 3 = 16 T period:
/// falling edge at div 16 on the last substep of cycle 4, reload one
/// cycle later on the same substep).
fn arm_late_timer_irq(b: &mut Interconnect) {
    b.ie = 0x04;
    b.timer.write(0xFF07, 0x05);
    b.timer.write(0xFF05, 0xFF);
}

/// A timer IF committed in the second half of an M-cycle is readable
/// and `pending()`-visible in that cycle (the running CPU's frozen
/// end-of-fetch sampling), but the mid-cycle halt-exit sampling misses
/// it until the next cycle, on every model (gambatte tima/tc*_irq_*
/// dmg08+cgb04c shared expectations; wilbertpol timer_if rounds 5/6
/// vs 3/4 on its full model matrix; SameBoy `GB_cpu_run`).
#[test]
fn halt_wake_misses_late_timer_if_for_one_cycle() {
    for model in [Model::Dmg, Model::Cgb, Model::Agb] {
        let mut b = ic(model);
        arm_late_timer_irq(&mut b);
        ticks(&mut b, 5); // cycle 5 = the reload + IF commit cycle
        assert_eq!(b.read_no_tick(0xFF0F) & 0x04, 0x04, "{model:?}: IF read");
        assert_eq!(b.pending(), 0x04, "{model:?}: running-CPU sampling");
        assert_eq!(b.pending_halt_wake(), 0, "{model:?}: halt wake misses it");
        b.tick();
        assert_eq!(b.pending_halt_wake(), 0x04, "{model:?}: visible next cycle");
    }
}

/// Non-timer IF bits stay live for the halt wake: the PPU IRQ anchors
/// are calibrated against the running CPU's end-of-fetch sampling, so
/// the intra-cycle offset is already absorbed there (mooneye
/// intr_2_0_timing passes on all models against this view; see
/// `pending_halt_wake` for the unmodelled CGB remainder).
#[test]
fn halt_wake_sees_non_timer_if_in_the_same_cycle() {
    for model in [Model::Dmg, Model::Cgb] {
        let mut b = ic(model);
        b.ie = 0x01;
        b.write(0xFF0F, 0x01); // bit lands during this M-cycle
        assert_eq!(b.pending_halt_wake(), 0x01, "{model:?}");
    }
}

/// The mode-0 STAT rise's half-cycle halt law (`Ppu::take_m0_rise` →
/// `if_late`): the IF bit is readable and dispatch-visible within its
/// own M-cycle for every phase, but the halt-exit sampler misses a
/// rise committed in the cycle's second half (PPU dots 3-4) for one
/// M-cycle. With the LCD enabled at an M-cycle boundary the rise dot
/// is 254 + SCX%8 on line 1 (glitch line 452 dots, ≡ 0 mod 4):
/// SCX=0 → dot ≡ 2 (first half, halt-visible at once), SCX=1 →
/// dot ≡ 3 (second half, halt-late). mooneye hblank_ly_scx_timing-GS
/// and gbmicrotest int_hblank_halt_scx0-7 pin all eight phases.
#[test]
fn m0_rise_second_half_commit_is_halt_late() {
    for (scx, late) in [(0u8, false), (1, true)] {
        let mut b = ic(Model::Dmg);
        b.ie = 0x02;
        b.write(0xFF43, scx);
        b.write(0xFF41, 0x08); // hblank STAT source
        b.write(0xFF40, 0x91);
        // Line 1 starts at dot 452 (the enable line is 4 dots
        // short); its mode-0 rise lands at 452 + 254 + SCX%8.
        let rise = 452 + 254 + u32::from(scx);
        // Run whole M-cycles up to the one containing the rise,
        // then drop the enable line's own rise from IF.
        ticks(&mut b, rise.div_ceil(4) - 1);
        b.intf = 0;
        assert_eq!(b.pending(), 0, "scx {scx}: not risen yet");
        b.tick();
        assert_eq!(b.pending(), 0x02, "scx {scx}: dispatch-visible");
        assert_eq!(
            b.pending_halt_wake(),
            if late { 0 } else { 0x02 },
            "scx {scx}: halt-wake view"
        );
        b.tick();
        assert_eq!(b.pending_halt_wake(), 0x02, "scx {scx}: next cycle");
    }
}

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
    // A flip on the M-cycle's FIRST dot (i=0): the INC-DS-1 dot-END eighth
    // was `edge_eighth(0,2)=4` = the M-cycle midpoint, which the cc+2 MID
    // observer does NOT precede (the half-split left it readable as mode 0).
    // `event_phase(StatMode)` now returns END_PHASE, so the whole straddle
    // M-cycle blocks regardless of which dot the flip lands on.
    b.stat_mode_edge = Some(event_phase(EdgeKind::StatMode, 0, 2));
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
    // speeds — the same value the dot-loop stamps).
    b.stat_mode_edge = Some(event_phase(EdgeKind::StatMode, 0, 2));
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
    for kind in [EdgeKind::M0Rise, EdgeKind::M0Access] {
        for dots in [2u64, 4] {
            for i in 0..dots {
                assert_eq!(
                    event_phase(kind, i, dots),
                    edge_eighth(i, dots),
                    "kind={kind:?} dots={dots} i={i}"
                );
            }
        }
    }
    // The two calibrated whole-M-cycle blocks commit at END regardless of
    // dot/speed (PalAccess: task 5; StatMode: task 6 — the double-speed
    // sprite m3stat_ds block, lifted from the dot-END half-split so a
    // 1st-half flip also holds the old mode 3).
    for kind in [EdgeKind::PalAccess, EdgeKind::StatMode] {
        for dots in [2u64, 4] {
            for i in 0..dots {
                assert_eq!(
                    event_phase(kind, i, dots),
                    END_PHASE,
                    "kind={kind:?} dots={dots} i={i}"
                );
            }
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
    for dots in [2u64, 4] {
        for i in 0..dots {
            let e = event_phase(EdgeKind::PalAccess, i, dots);
            // Pin the exact commit phase, not just "blocks": the unblock
            // commits at the M-cycle END regardless of dot/speed.
            assert_eq!(
                e, END_PHASE,
                "palette commits at M-cycle end: dots={dots} i={i}"
            );
            assert!(
                stamp_blocks(Some(e), ACCESS_PHASE),
                "palette blocked at MID for every dot: dots={dots} i={i} e={e}"
            );
        }
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

#[test]
fn ff50_reads_ff_and_ignores_writes() {
    let mut b = ic(Model::Dmg);
    b.write(0xFF50, 0x00);
    assert_eq!(b.read(0xFF50), 0xFF);
}

#[test]
fn unmapped_io_reads_ff() {
    let mut b = ic(Model::Dmg);
    for addr in [
        0xFF03, 0xFF08, 0xFF0E, 0xFF4C, 0xFF4E, 0xFF57, 0xFF6D, 0xFF7F,
    ] {
        assert_eq!(b.read(addr), 0xFF, "{addr:04X}");
    }
}

#[test]
fn dmg_has_no_cgb_registers() {
    let mut b = ic(Model::Dmg);
    for addr in [
        0xFF4D, 0xFF4F, 0xFF51, 0xFF52, 0xFF53, 0xFF54, 0xFF55, 0xFF56, 0xFF68, 0xFF69, 0xFF6A,
        0xFF6B, 0xFF6C, 0xFF70, 0xFF72, 0xFF73, 0xFF74, 0xFF75, 0xFF76, 0xFF77,
    ] {
        b.write(addr, 0x00);
        assert_eq!(b.read(addr), 0xFF, "{addr:04X}");
    }
}

// ---- dispatch-ack source sync-ahead (gambatte Memory::ackIrq) -------

/// A timer IF set produced by the machine tick right after a
/// dispatch ack is consumed by it on both families (gambatte ackIrq
/// `updateTimaIrq(cc + 2 + isCgb())` reaches past the last-substep
/// commit of the next M-cycle's reload; tima/tc00_irq_late_retrigger_3
/// reads E0 on dmg08 *and* cgb04c). The TMA reload itself still
/// happens — only the IF bit is consumed.
#[test]
fn dispatch_ack_consumes_timer_set_due_next_cycle() {
    for model in [Model::Dmg, Model::Cgb] {
        let mut b = ic(model);
        arm_late_timer_irq(&mut b);
        ticks(&mut b, 4); // overflow armed; reload + IF due next tick
        b.ack(2); // the dispatch's IF clear
        ticks(&mut b, 1);
        assert_eq!(b.read_no_tick(0xFF0F) & 0x04, 0, "{model:?}");
        assert_eq!(
            b.timer.read(0xFF05),
            b.timer.read(0xFF06),
            "{model:?}: reload"
        );
    }
}

/// The sync-ahead window is one M-cycle on the DMG family and two on
/// CGB/AGB (`+ isCgb()`): a set committing in the second tick after
/// the ack survives on DMG and is consumed on CGB — the
/// tc00_irq_late_retrigger_2 dmg08_outE4 / cgb04c_outE0 split. Three
/// cycles out it survives everywhere.
#[test]
fn dispatch_ack_timer_window_is_one_cycle_dmg_two_cgb() {
    for (model, expect) in [
        (Model::Dmg, 0x04),
        (Model::Sgb, 0x04),
        (Model::Cgb, 0x00),
        (Model::Agb, 0x00),
    ] {
        let mut b = ic(model);
        arm_late_timer_irq(&mut b);
        ticks(&mut b, 3);
        b.ack(2);
        ticks(&mut b, 2); // overflow in tick 4, reload + IF in tick 5
        assert_eq!(b.read_no_tick(0xFF0F) & 0x04, expect, "{model:?}");
    }
    for model in [Model::Dmg, Model::Cgb] {
        let mut b = ic(model);
        arm_late_timer_irq(&mut b);
        ticks(&mut b, 2);
        b.ack(2);
        ticks(&mut b, 3);
        assert_eq!(
            b.read_no_tick(0xFF0F) & 0x04,
            0x04,
            "{model:?}: past window"
        );
    }
}

/// Serial transfer-complete IF: same ack windows via gambatte's
/// `updateSerial(cc + 3 + isCgb())` — with the completion on the
/// DIV-edge boundary, DMG consumes the set due in the next tick,
/// CGB also the one after (serial/start_wait_trigger_int8_read_if_2:
/// dmg08_outE8 vs cgb04c_outE0; round 3 E0 on both).
#[test]
fn dispatch_ack_consumes_serial_set_like_gambatte_ackirq() {
    // Completion (8th shift) at div 4096 = machine tick 1024.
    for (model, gap, expect) in [
        (Model::Dmg, 1, 0x00),
        (Model::Cgb, 1, 0x00),
        (Model::Dmg, 2, 0x08),
        (Model::Cgb, 2, 0x00),
        (Model::Dmg, 3, 0x08),
        (Model::Cgb, 3, 0x08),
    ] {
        let mut b = ic(model);
        b.serial.write(0xFF01, 0x00);
        b.serial.write(0xFF02, 0x81);
        ticks(&mut b, 1024 - gap);
        b.ack(3);
        ticks(&mut b, gap);
        assert_eq!(b.read_no_tick(0xFF0F) & 0x08, expect, "{model:?} gap {gap}");
        assert_eq!(
            b.serial.read(0xFF02) & 0x80,
            0,
            "{model:?}: transfer still ends"
        );
    }
}

/// The ack only consumes the *acked* source: a timer ack does not
/// swallow a serial set in the window (gambatte ackIrq clears one
/// bit; the sync-ahead merely flags the others earlier).
#[test]
fn dispatch_ack_squash_is_per_source() {
    let mut b = ic(Model::Cgb);
    b.serial.write(0xFF02, 0x81);
    ticks(&mut b, 1023);
    b.ack(2); // timer ack, serial completion due next tick
    ticks(&mut b, 1);
    assert_eq!(b.read_no_tick(0xFF0F) & 0x08, 0x08);
}

/// STAT/VBlank rises go through `lcd_.update(cc + 2)` — only the
/// first 2 dots of the next tick. The vblank rise is a line-anchored
/// event emitted in the *second half* of its M-cycle at single
/// speed, so an ack in the cycle before must NOT consume it
/// (gambatte m2int_m2irq_late_retrigger_1 and
/// irq_precedence/late_m0irq_retrigger_scx1_1 pin the keeps; the
/// consumed cases live on the gambatte `*_late_retrigger_ds_2` rows,
/// where the 2-dot window spans the whole double-speed tick, and on
/// the mode-0 rise's early-dot grid).
#[test]
fn dispatch_ack_does_not_reach_single_speed_line_anchored_rises() {
    for model in [Model::Dmg, Model::Cgb] {
        // Find the tick of the first vblank IF after an LCD enable
        // (per model: the CGB line timeline may shift it).
        let rise = {
            let mut b = ic(model);
            b.write_no_tick(0xFF40, 0x91);
            let mut n = 0;
            while b.read_no_tick(0xFF0F) & 0x01 == 0 {
                b.tick();
                n += 1;
            }
            n
        };
        for gap in [1, 2] {
            let mut b = ic(model);
            b.write_no_tick(0xFF40, 0x91);
            ticks(&mut b, rise - gap);
            b.ack(0);
            ticks(&mut b, gap);
            assert_eq!(
                b.read_no_tick(0xFF0F) & 0x01,
                0x01,
                "{model:?} gap {gap}: kept"
            );
        }
    }
}

// ---- tick-then-access -----------------------------------------------

#[test]
fn access_observes_state_after_the_cycles_tick() {
    let mut b = ic(Model::Dmg);
    // TAC = freq 01 (DIV bit 3, every 16 T). Write cycle: div 0 -> 4.
    b.write(0xFF07, 0x05);
    b.tick(); // div 8
    assert_eq!(b.read(0xFF05), 0, "read cycle: div 12, no edge yet");
    // This read's own tick takes div to 16 — the bit-3 falling edge
    // clocks TIMA *before* the access observes it.
    assert_eq!(b.read(0xFF05), 1);
}

#[test]
fn timer_overflow_requests_if_bit2() {
    let mut b = ic(Model::Dmg);
    b.write(0xFF05, 0xFF);
    b.write(0xFF07, 0x05);
    ticks(&mut b, 8);
    assert_eq!(b.read(0xFF0F) & 0x04, 0x04);
}

#[test]
fn joypad_press_requests_if_bit4() {
    let mut b = ic(Model::Dmg);
    b.write(0xFF00, 0x10); // select the button column
    b.joypad_mut().press(crate::joypad::Button::Start);
    b.tick();
    assert_eq!(b.read(0xFF0F) & 0x10, 0x10);
    assert_eq!(b.read(0xFF00), 0xD7);
}

#[test]
fn vblank_requests_if_bit0() {
    let mut b = ic(Model::Dmg);
    b.write(0xFF40, 0x91);
    // 145 lines is comfortably past the vblank IF at 144:4.
    ticks(&mut b, 145 * 114);
    assert_eq!(b.read(0xFF0F) & 0x01, 0x01);
}

#[test]
fn serial_transfer_requests_if_bit3() {
    let mut b = ic(Model::Dmg);
    b.write(0xFF02, 0x81);
    ticks(&mut b, 8 * 128 + 2);
    assert_eq!(b.read(0xFF0F) & 0x08, 0x08);
    assert_eq!(b.read(0xFF01), 0xFF);
}

// ---- OAM DMA ---------------------------------------------------------

/// Fill WRAM 0xC000.. with `base+i` through untimed writes.
fn fill_wram(b: &mut Interconnect, addr: u16, base: u8, len: u16) {
    for i in 0..len {
        b.write_no_tick(addr + i, base.wrapping_add(i as u8));
    }
}

#[test]
fn oam_dma_setup_cycle_leaves_oam_accessible() {
    let mut b = ic(Model::Dmg);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xC0); // cycle W
    // Cycle W+1: setup delay, OAM still reads its old content
    // (oam_dma_start executes an opcode from OAM here).
    assert_eq!(b.read(0xFE00), 0x00);
    // Cycle W+2: byte 0 is in flight, OAM reads $FF.
    assert_eq!(b.read(0xFE00), 0xFF);
}

/// acceptance/oam_dma_timing: OAM unlocks exactly 162 M-cycles after
/// the FF46 write cycle (1 setup + 160 transfer + the access cycle).
#[test]
fn oam_dma_timing_exact() {
    for (extra, expected) in [(0u32, 0xFF), (1, 0x80)] {
        let mut b = ic(Model::Dmg);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        b.write(0xFF46, 0xC0);
        ticks(&mut b, 160 + extra);
        assert_eq!(b.read(0xFE00), expected, "extra={extra}");
    }
}

#[test]
fn oam_dma_copies_all_160_bytes() {
    let mut b = ic(Model::Dmg);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xC0);
    ticks(&mut b, 161);
    assert_eq!(b.read(0xFE00), 0x80);
    assert_eq!(b.read(0xFE9F), 0x80u8.wrapping_add(159));
}

/// The PPU's OAM view disconnects while the OAM DMA controller owns
/// OAM — for the dots of cycles W+3 .. W+162 around an FF46 write at
/// cycle W: gambatte memory.cpp timestamps startOamDma at the byte-0
/// copy step (the end of our W+2) and endOamDma one step past byte
/// 159 (the end of our W+162), and the OamReader latches real OAM up
/// to each timestamp. 160 disconnected M-cycles total; the gambatte
/// oamdma/late_sp* `_1`/`_2` pairs pin both edges at M-cycle
/// granularity per scanned sprite slot.
#[test]
fn oam_dma_disconnects_ppu_scan_for_160_cycles() {
    let mut b = ic(Model::Dmg);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xC0); // cycle W
    b.tick();
    assert!(!b.ppu.oam_dma_scan_disconnected(), "W+1: setup delay");
    b.tick();
    assert!(
        !b.ppu.oam_dma_scan_disconnected(),
        "W+2: byte 0 lands at the cycle's end"
    );
    for k in 3..163 {
        b.tick();
        assert!(b.ppu.oam_dma_scan_disconnected(), "W+{k}");
    }
    b.tick();
    assert!(!b.ppu.oam_dma_scan_disconnected(), "W+163: reconnected");
}

/// HALT gates the controller's clock mid-transfer: the disconnect
/// level persists for the whole freeze (gambatte updateOamDma's
/// halted() path advances no position and never reaches endOamDma —
/// the OamReader source stays rdisabledRam; dmg08-verified by
/// gambatte oamdma_late_halt_stat_1/_2).
#[test]
fn oam_dma_disconnect_persists_through_halt_freeze() {
    let mut b = ic(Model::Dmg);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xC0);
    ticks(&mut b, 10);
    assert!(b.ppu.oam_dma_scan_disconnected());
    b.set_cpu_halted(true);
    ticks(&mut b, 400); // far past the un-frozen end of the transfer
    assert!(
        b.ppu.oam_dma_scan_disconnected(),
        "frozen transfer still owns OAM"
    );
    b.set_cpu_halted(false);
    // The remaining ~152 bytes finish after the wake; then reconnect.
    ticks(&mut b, 160);
    assert!(!b.ppu.oam_dma_scan_disconnected());
}

#[test]
fn oam_dma_reg_reads_back_last_write() {
    let mut b = ic(Model::Dmg);
    b.write(0xFF46, 0x90);
    assert_eq!(b.read(0xFF46), 0x90);
    b.write(0xFF46, 0x8F); // restart mid-transfer
    assert_eq!(b.read(0xFF46), 0x8F);
}

/// acceptance/oam_dma/sources-GS: source pages $E0-$FF re-read WRAM,
/// including $FE/$FF -> $DE00/$DF00.
#[test]
fn oam_dma_high_sources_read_wram_echo() {
    for (page, base) in [(0xE0u8, 0x80u8), (0xFE, 0x21), (0xFF, 0x42)] {
        let mut b = ic(Model::Dmg);
        fill_wram(&mut b, 0xC000, 0x80, 160);
        fill_wram(&mut b, 0xDE00, 0x21, 0x100);
        fill_wram(&mut b, 0xDF00, 0x42, 0x100);
        b.write(0xFF46, page);
        ticks(&mut b, 161);
        assert_eq!(b.read(0xFE00), base, "page {page:02X}");
        assert_eq!(b.read(0xFE01), base + 1, "page {page:02X}");
    }
}

#[test]
fn oam_dma_from_rom_and_vram() {
    let mut b = ic(Model::Dmg);
    b.write(0x9000, 0x77); // LCD off: VRAM writable
    b.write(0xFF46, 0x10); // ROM pattern page
    ticks(&mut b, 161);
    assert_eq!(b.read(0xFE00), 0x5A);
    b.write(0xFF46, 0x90);
    ticks(&mut b, 161);
    assert_eq!(b.read(0xFE00), 0x77);
}

#[test]
fn oam_writes_dropped_and_reads_ff_during_dma() {
    let mut b = ic(Model::Dmg);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xC0);
    b.tick(); // setup
    b.write(0xFE10, 0x99); // transfer running: dropped
    assert_eq!(b.read(0xFEA0), 0xFF); // prohibited area also $FF
    ticks(&mut b, 161);
    assert_eq!(b.read(0xFE10), 0x90, "DMA value, not the CPU write");
}

/// gbctr bus conflicts: a CPU read on the bus the DMA is using returns
/// the byte the DMA is transferring; the other bus is unaffected.
/// (Write at cycle W; byte i is in flight at cycle W+2+i, so reads at
/// W+3, W+4, ... observe bytes 1, 2, ...)
#[test]
fn oam_dma_bus_conflicts() {
    // ROM source (external bus): ROM/WRAM reads conflict on DMG, VRAM
    // reads do not.
    let mut b = ic(Model::Dmg);
    b.write(0x8500, 0x33);
    b.write(0xFF46, 0x10); // cycle W
    b.tick(); // W+1 setup
    b.tick(); // W+2: byte 0 in flight
    assert_eq!(b.read(0x4242), 0x5A ^ 1, "ROM read sees DMA byte 1");
    assert_eq!(b.read(0xC000), 0x5A ^ 2, "DMG WRAM shares the bus");
    assert_eq!(b.read(0x8500), 0x33, "VRAM bus unaffected");

    // VRAM source: external bus unaffected.
    let mut b = ic(Model::Dmg);
    b.write(0x8000, 0x44);
    b.write(0x8001, 0x45);
    b.write(0xFF46, 0x80);
    b.tick();
    b.tick();
    assert_eq!(b.read(0x9999), 0x45, "VRAM read sees DMA byte 1");
    assert_eq!(b.read(0x1000), 0x5A, "external bus unaffected");
}

/// The OAM DMA controller runs on the CPU core clock, which HALT gates
/// off (the PPU keeps its own clock): a transfer in progress does not
/// proceed while the CPU is halted. Bytes already copied stay, the byte
/// in flight never commits, the rest of OAM keeps its old contents, and
/// the transfer resumes exactly where it stopped when the CPU wakes.
/// Hardware-verified by madness/mgb_oam_dma_halt_sprites.s: halting
/// after the third byte's read leaves that OAM byte un-replaced, and the
/// PPU renders from the old/new mixture indefinitely.
#[test]
fn oam_dma_freezes_while_cpu_halted() {
    let mut b = ic(Model::Mgb);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write_no_tick(0xFE02, 0x30); // old OAM byte the freeze must keep
    b.write(0xFF46, 0xC0); // cycle W
    b.tick(); // W+1: setup delay
    b.tick(); // W+2: byte 0 in flight
    b.tick(); // W+3: byte 1 in flight
    b.set_cpu_halted(true);
    // Frozen for hundreds of M-cycles: no progress. (On hardware the
    // halted CPU performs no bus accesses, so these reads observe
    // unobservable state: raw OAM, no bus conflict — LCD is off here.)
    for _ in 0..200 {
        assert_eq!(b.read(0xFE00), 0x80, "copied byte 0 stays");
    }
    assert_eq!(b.read(0xFE01), 0x81, "copied byte 1 stays");
    assert_eq!(b.read(0xFE02), 0x30, "frozen: old OAM byte persists");
    assert_eq!(b.read(0xC000), 0x80, "no DMA traffic on the external bus");
    // Waking copies byte 2 in the release's catch-up cycle (see
    // `halt_wake_advances_oam_dma_one_catchup_cycle`); 157 transfer
    // cycles remain after it.
    b.set_cpu_halted(false);
    ticks(&mut b, 156);
    assert_eq!(b.read(0xFE00), 0xFF, "byte 159 in flight: OAM blocked");
    assert_eq!(b.read(0xFE00), 0x80, "transfer complete");
    assert_eq!(b.read(0xFE02), 0x82, "resumed transfer replaced the byte");
    assert_eq!(b.read(0xFE9F), 0x80u8.wrapping_add(159));
}

/// Releasing the core-clock gate advances a frozen OAM DMA by one
/// catch-up M-cycle *at the release itself*, before the CPU's first
/// post-wake cycle: the controller's clock restarts with the halt
/// exit, one M-cycle ahead of the CPU pipeline (SameBoy sm83_cpu.c
/// `GB_cpu_run` halt exit: `gb->dma_cycles = 4; GB_dma_run(gb)` on
/// both the IME=0 resume and the dispatch path, while `GB_dma_run`
/// itself returns early whenever `gb->halted`). Hardware-pinned by
/// gambatte oamdma/oamdmasrc80_halt_lycirq_read8000 /
/// _m2irq_read8000 (out81, both models), dma/hdma_transition_oamdma_2
/// (out67) and dma/hdma_transition_speedchange_oamdma (out71), all of
/// which observe the in-flight source index after a wake.
#[test]
fn halt_wake_advances_oam_dma_one_catchup_cycle() {
    let mut b = ic(Model::Dmg);
    fill_wram(&mut b, 0xC000, 0x50, 0xA0);
    b.write(0xFF46, 0xC0); // cycle W
    ticks(&mut b, 6); // W+2..W+6 copy idx 0..4
    b.set_cpu_halted(true);
    ticks(&mut b, 50);
    assert_eq!(b.peek(0xFE05), 0x00, "frozen");
    b.set_cpu_halted(false);
    assert_eq!(b.peek(0xFE05), 0x55, "catch-up copy at the gate release");
    assert_eq!(b.peek(0xFE06), 0x00, "exactly one cycle of catch-up");
    b.tick(); // copies idx 6, committing at the next cycle's head
    b.tick();
    assert_eq!(b.peek(0xFE06), 0x56);
}

/// The speed-switch pause releases the same core-clock gate but
/// performs *no* catch-up cycle: the next OAM DMA byte copies on the
/// first machine cycle after the pause, not at the release (gambatte
/// oamdma/oamdmasrcC0_speedchange_readC000 out11 pins the exact
/// post-pause in-flight index, one position below a caught-up resume;
/// SameBoy's `speed_switch_halt_countdown` expiry likewise just clears
/// `halted` with no `GB_dma_run` call, unlike its halt-exit paths).
#[test]
fn speed_switch_pause_exit_does_not_catch_up_oam_dma() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x50, 0xA0);
    b.write(0xFF4D, 0x01); // arm the switch
    b.write(0xFF46, 0xC0); // cycle W
    ticks(&mut b, 6); // W+2..W+6 copy idx 0..4
    assert!(b.stop(0x0000, false)); // read cycle copies idx 5, then pause
    assert_eq!(b.peek(0xFE05), 0x55);
    assert_eq!(b.peek(0xFE06), 0x00, "frozen across the pause, no catch-up");
    b.tick(); // copies idx 6: the first post-pause cycle...
    b.tick(); // ...committing at the next cycle's head
    assert_eq!(
        b.peek(0xFE06),
        0x56,
        "resumes on the first post-pause cycle"
    );
}

/// The FF46 1 M-cycle setup delay counts on the same gated clock, so a
/// CPU halting right after the FF46 write freezes the transfer before
/// its first byte (companion to `oam_dma_freezes_while_cpu_halted`).
#[test]
fn oam_dma_setup_delay_freezes_while_cpu_halted() {
    let mut b = ic(Model::Mgb);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xC0);
    b.set_cpu_halted(true);
    for _ in 0..10 {
        assert_eq!(b.read(0xFE00), 0x00, "setup delay frozen: no transfer");
    }
    // The release's catch-up cycle elapses the setup delay; the next
    // cycle copies byte 0.
    b.set_cpu_halted(false);
    assert_eq!(b.read(0xFE00), 0xFF, "byte 0 in flight");
    ticks(&mut b, 159);
    assert_eq!(b.read(0xFE00), 0x80, "transfer complete");
}

/// Gating the clock mid-transfer hands the PPU the frozen in-flight
/// access (OAM index + source byte) for the MGB OAM scan glitch
/// (madness/mgb_oam_dma_halt_sprites.s); ungating (or freezing with no
/// transfer / only the setup delay in flight) hands over nothing.
#[test]
fn cpu_halt_hands_frozen_dma_access_to_ppu() {
    let mut b = ic(Model::Mgb);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.set_cpu_halted(true);
    assert_eq!(b.ppu.oam_dma_freeze(), None, "no transfer running");
    b.set_cpu_halted(false);
    b.write(0xFF46, 0xC0); // cycle W
    b.set_cpu_halted(true);
    assert_eq!(b.ppu.oam_dma_freeze(), None, "setup delay: no OAM access");
    b.set_cpu_halted(false); // catch-up cycle: setup delay elapses
    b.tick(); // byte 0 in flight
    b.tick(); // byte 1 in flight
    b.set_cpu_halted(true);
    assert_eq!(
        b.ppu.oam_dma_freeze(),
        Some((2, 0x82)),
        "byte 2 frozen mid-access"
    );
    b.set_cpu_halted(false);
    assert_eq!(b.ppu.oam_dma_freeze(), None, "cleared on wake");
}

/// CGB WRAM has its own bus: a WRAM-source transfer leaves the
/// external bus alone, and a ROM-source transfer never puts its byte
/// on the WRAM bus — a WRAM-region read mid-transfer goes through the
/// conflict *redirect* (same cell here: FF46 bit 4 = 0, offset 0)
/// rather than observing the ROM byte.
#[test]
fn cgb_wram_is_a_separate_bus() {
    let mut b = ic(Model::Cgb);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0x00); // ROM source
    b.tick();
    b.tick();
    assert_eq!(b.read(0xC000), 0x80, "no ROM byte on the CGB WRAM bus");
    let mut b = ic(Model::Cgb);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xC0); // WRAM source
    b.tick();
    b.tick();
    assert_eq!(b.read(0x1000), 0x5A, "ROM does not conflict with CGB WRAM");
    assert_eq!(b.read(0xC050), 0x82, "WRAM read sees DMA byte 2");
}

// ---- OAM DMA bus-conflict writes and CGB quirks ----------------------
//
// Semantics mirrored from gambatte-core memory.cpp (nontrivial_read /
// nontrivial_write OAM-DMA conflict blocks) and calibrated against the
// hardware-recorded gambatte/oamdma expectation matrix; per-test
// citations name the pinning ROMs.

/// DMG: a CPU write on pages the running transfer occupies derails
/// into the in-flight OAM slot (pure CPU byte for a ROM source) and
/// never reaches the addressed memory
/// (oamdma_src0000_busypushC001_dmg08_out55AA1234: both pushed bytes
/// land in OAM $9D/$9E, the WRAM/SRAM marker bytes survive).
#[test]
fn dmg_conflicted_write_lands_in_oam_slot_not_memory() {
    let mut b = ic(Model::Dmg);
    b.write_no_tick(0xC050, 0x34); // marker
    b.write(0xFF46, 0x10); // ROM source, cycle W
    b.tick(); // W+1 setup
    b.tick(); // W+2: byte 0 in flight
    // Cycle W+3: byte 1 (ROM $1001 = $5B) is in flight; the WRAM write
    // is on the conflicting external bus.
    b.write(0xC050, 0xAA);
    ticks(&mut b, 165); // run the transfer out
    assert_eq!(b.read(0xFE01), 0xAA, "CPU byte replaced DMA byte 1");
    assert_eq!(b.read(0xFE02), 0x58, "byte 2 unmolested (ROM $1002)");
    assert_eq!(b.read(0xC050), 0x34, "memory write suppressed");
}

/// DMG WRAM-source conflict wire-ANDs the CPU byte into the in-flight
/// byte (oamdma_srcC000_busypushC001_dmg08_out45221234: $65&$55=$45,
/// $76&$AA=$22).
#[test]
fn dmg_wram_source_write_conflict_is_wired_and() {
    let mut b = ic(Model::Dmg);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xC0);
    b.tick();
    b.tick();
    b.write(0x4000, 0x55); // ROM page: same external bus on DMG
    ticks(&mut b, 165);
    assert_eq!(b.read(0xFE01), 0x81 & 0x55, "wired-AND of DMA and CPU byte");
}

/// CGB VRAM-source conflicts: a conflicted write puts $00 in the slot
/// (oamdma_src8000_busypush8001_cgb04c_out00761234), and a conflicted
/// read returns the in-flight byte but zeroes the OAM slot afterwards
/// (gambatte memory.cpp nontrivial_read: `ioamhram_[oamDmaPos_] = 0`
/// for vram sources). DMG keeps the pure CPU byte on writes
/// (src8000_busypush8001_dmg08_out55761234).
#[test]
fn cgb_vram_source_conflicts_zero_oam() {
    for (model, expect_w) in [(Model::Cgb, 0x00), (Model::Dmg, 0x55)] {
        let mut b = ic(model);
        b.write(0x8000, 0x44);
        b.write(0x8001, 0x45);
        b.write(0x8002, 0x46);
        b.write(0xFF46, 0x80);
        b.tick();
        b.tick(); // byte 0 in flight
        b.write(0x9123, 0x55); // byte 1 cycle: VRAM-bus write conflict
        assert_eq!(b.read(0x9456), 0x46, "byte 2 cycle: conflicted read");
        ticks(&mut b, 162);
        assert_eq!(b.read(0xFE01), expect_w, "{model:?}: write conflict");
        let expect_r = if model.is_cgb() { 0x00 } else { 0x46 };
        assert_eq!(b.read(0xFE02), expect_r, "{model:?}: read zeroes slot");
    }
}

/// CGB: ROM/SRAM-source transfers conflict with the WRAM pages too,
/// but accesses there are redirected to WRAM bank 0 / the banked page
/// (selected by FF46 bit 4) at offset `addr & 0xFFF` — they never
/// touch OAM (oamdma_src0000_busypopDFFF_cgb04c_out657655AA: a $DFFF
/// read mid-transfer returns WRAM0[$FFF];
/// oamdma_srcE000_busypushC001_cgb04c_outFFAA1255: the $C000 write
/// lands in WRAM0[0], read back as $55 post-DMA).
#[test]
fn cgb_conflict_wram_access_redirects_to_ff46_bank() {
    let mut b = ic(Model::Cgb);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write_no_tick(0xCFFF, 0x21);
    b.write_no_tick(0xDFFF, 0x43);
    b.write(0xFF46, 0x00); // ROM source, FF46 bit 4 = 0
    b.tick();
    b.tick();
    assert_eq!(b.read(0xDFFF), 0x21, "read redirected to WRAM0[$FFF]");
    b.write(0xD123, 0x99); // redirected to WRAM0[$123]
    ticks(&mut b, 162);
    assert_eq!(b.read(0xC123), 0x99, "write landed in WRAM bank 0");
    assert_eq!(b.read(0xD123), 0x00, "addressed cell untouched");
    assert_eq!(b.read(0xFE02), 0x00, "OAM untouched by the redirect");

    // FF46 bit 4 set: the banked page is addressed instead.
    let mut b = ic(Model::Cgb);
    b.write_no_tick(0xD456, 0x77);
    b.write(0xFF46, 0x10); // ROM source, bit 4 = 1
    b.tick();
    b.tick();
    assert_eq!(b.read(0xC456), 0x77, "read redirected to banked WRAM page");
}

/// CGB WRAM-source transfers conflict only with the WRAM pages, and
/// CPU writes there are swallowed entirely
/// (oamdma_srcC000_busypushE001_cgb04c_out65761234: markers intact,
/// OAM untouched).
#[test]
fn cgb_wram_source_wram_write_swallowed() {
    let mut b = ic(Model::Cgb);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write_no_tick(0xC050, 0x34);
    b.write(0xFF46, 0xC0);
    b.tick();
    b.tick();
    b.write(0xC050, 0xAA);
    ticks(&mut b, 165);
    assert_eq!(b.read(0xFE01), 0x81, "OAM untouched");
    assert_eq!(b.read(0xC050), 0x34, "write swallowed");
}

/// CGB: FF46 ≥ $E0 is an invalid source — the engine reads $FF
/// (gambatte memory.cpp oamDmaSrcPtr → rdisabledRam; every
/// srcE000/EF00/F000/FE00/FF00 cgb04c expectation shows $FF OAM
/// bytes) while conflicting like a ROM source
/// (srcE000_busypush8001_cgb04c_outFFAA1255). DMG keeps the WRAM echo
/// (mooneye sources-GS, `oam_dma_high_sources_read_wram_echo`).
#[test]
fn cgb_high_sources_read_ff_and_conflict() {
    let mut b = ic(Model::Cgb);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xE0);
    b.tick();
    b.tick(); // byte 0 in flight
    assert_eq!(b.read(0x4000), 0xFF, "ROM page read sees the $FF byte");
    b.write(0x4000, 0xAA); // conflicted write lands in the OAM slot
    ticks(&mut b, 162);
    assert_eq!(b.read(0xFE00), 0xFF);
    assert_eq!(b.read(0xFE02), 0xAA, "CPU byte in slot 2");
    assert_eq!(b.read(0xFE9F), 0xFF);
}

/// Restarting a transfer retargets the in-flight run immediately: the
/// handover copies before the new transfer's byte 0 read from the NEW
/// source at the old indices (gambatte memory.cpp FF46 handler updates
/// ioamhram_[0x146] + oamDmaInitSetup before the next copy;
/// hardware-pinned by oamdma_src8000_srcchange0000_busyread0000_1/2.
/// mooneye oam_dma_restart restarts with the same page and cannot
/// discriminate).
#[test]
fn oam_dma_restart_handover_copies_from_new_source() {
    let mut b = ic(Model::Dmg);
    fill_wram(&mut b, 0xC000, 0x80, 160); // old source
    fill_wram(&mut b, 0xD000, 0x10, 160); // new source
    b.write(0xFF46, 0xC0); // cycle W
    b.tick(); // W+1 setup
    b.tick(); // W+2 old byte 0
    b.write(0xFF46, 0xD0); // cycle W+3: old byte 1 copied, then retarget
    // Cycle W+4 (new setup): the handover copy reads the NEW source at
    // the old index 2. Observe it through the external-bus conflict.
    assert_eq!(b.read(0x0000), 0x12, "handover byte came from $D002");
    // Cycle W+5: new transfer byte 0.
    assert_eq!(b.read(0x0000), 0x10);
    ticks(&mut b, 161);
    assert_eq!(b.read(0xFE00), 0x10);
    assert_eq!(b.read(0xFE05), 0x15);
}

// ---- prohibited area ------------------------------------------------

#[test]
fn prohibited_area_dmg() {
    let mut b = ic(Model::Dmg);
    assert_eq!(b.read(0xFEA0), 0x00, "LCD off: OAM idle");
    b.write(0xFEA0, 0x55); // writes ignored
    assert_eq!(b.read(0xFEA0), 0x00);
    b.write(0xFF40, 0x91);
    // Advance into mode 3 of a steady line (the glitched enable line
    // blocks from dot 78 already, take line 1 to be safe).
    ticks(&mut b, (452 + 120) / 4);
    assert_eq!(b.read(0xFEA0), 0xFF, "OAM locked: reads $FF");
}

/// FEA0-FEFF on CPU CGB C (the silicon [`Model::Cgb`] pins, see
/// ARCHITECTURE §CGB revision policy): extra OAM RAM whose low address
/// bits 3-4 don't decode, so each of the 24 cells is mirrored 4 times
/// across the region (Pan Docs "FEA0-FEFF range", revisions 0-D;
/// gambatte-core memory.cpp indexes `ioamhram_[(p - 0xFE00) & 0xE7]`;
/// pinned by gambatte oamdma_srcXXXX_busypushFEA1/FF01 cgb04c rows,
/// whose markers written there survive a dropped mid-DMA push).
#[test]
fn prohibited_area_cgb_c_is_extra_ram_with_mirrors() {
    let mut b = ic(Model::Cgb);
    b.write(0xFEA0, 0x12);
    b.write(0xFEC1, 0x34);
    b.write(0xFEFF, 0x56);
    assert_eq!(b.read(0xFEA0), 0x12);
    for mirror in [0xFEA8, 0xFEB0, 0xFEB8] {
        assert_eq!(b.read(mirror), 0x12, "{mirror:04X} mirrors FEA0");
    }
    assert_eq!(b.read(0xFEC9), 0x34, "FEC9 mirrors FEC1");
    assert_eq!(b.read(0xFEF7), 0x56, "FEF7 mirrors FEFF");
    assert_eq!(b.read(0xFEA1), 0x00, "distinct cell untouched");
}

/// The extra RAM sits behind the same OAM gating as FE00-FE9F: $FF /
/// dropped while a DMA byte is in flight (gambatte memory.cpp:
/// `oamDmaPos_ < oam_size` guards both paths).
#[test]
fn cgb_extra_ram_blocked_during_oam_dma() {
    let mut b = ic(Model::Cgb);
    b.write(0xFEA0, 0x12);
    fill_wram(&mut b, 0xC000, 0x80, 160);
    b.write(0xFF46, 0xC0);
    b.tick(); // setup
    b.write(0xFEA0, 0x99); // in flight: dropped
    assert_eq!(b.read(0xFEA0), 0xFF, "in flight: reads $FF");
    ticks(&mut b, 161);
    assert_eq!(b.read(0xFEA0), 0x12, "marker survived the transfer");
}

/// AGB (and CGB revision E) instead echo the high nibble of the low
/// address byte twice (Pan Docs "FEA0-FEFF range").
#[test]
fn prohibited_area_agb_echoes_high_nibble() {
    let mut b = ic(Model::Agb);
    assert_eq!(b.read(0xFEA3), 0xAA);
    assert_eq!(b.read(0xFEB0), 0xBB);
    assert_eq!(b.read(0xFEFF), 0xFF);
}

// ---- CGB registers and modes ------------------------------------------

#[test]
fn cgb_dmg_compat_mode_disables_cgb_only_registers() {
    let mut b = ic(Model::Cgb); // DMG cart on CGB hardware
    assert!(!b.cgb_mode);
    for addr in [
        0xFF4D, 0xFF51, 0xFF55, 0xFF56, 0xFF69, 0xFF6B, 0xFF70, 0xFF74,
    ] {
        b.write(addr, 0x00);
        assert_eq!(b.read(addr), 0xFF, "{addr:04X}");
    }
    assert_eq!(b.read(0xFF4F), 0xFE, "VBK still reads bank 0");
    b.write(0xFF4F, 0x01); // locked: write ignored
    assert_eq!(b.read(0xFF4F), 0xFE);
    // FF72/73/75 exist in both modes (boot_hwio-C).
    b.write(0xFF72, 0xAB);
    assert_eq!(b.read(0xFF72), 0xAB);
    b.write(0xFF75, 0xFF);
    assert_eq!(b.read(0xFF75), 0xFF);
    b.write(0xFF75, 0x00);
    assert_eq!(b.read(0xFF75), 0x8F);
    assert_eq!(b.read(0xFF76), 0x00);
    assert_eq!(b.read(0xFF77), 0x00);
    // SVBK locked: D000 stays bank 1.
    b.write(0xC000, 1);
    b.write(0xD000, 2);
    b.write(0xFF70, 0x03);
    assert_eq!(b.read(0xD000), 2);
}

#[test]
fn cgb_mode_decodes_only_header_bit7() {
    // Pan Docs "CGB flag" (0x143): the CGB boot ROM tests only bit 7,
    // so 0x84 enables CGB mode just like 0x80/0xC0 — and `auto_model`
    // must agree (shared predicate, `cartridge::cgb_flag`).
    let mut rom = test_rom();
    rom[0x143] = 0x84;
    assert_eq!(crate::GameBoy::auto_model(&rom), Model::Cgb);
    let b = Interconnect::new(Model::Cgb, Cartridge::from_bytes(rom).unwrap());
    assert!(b.cgb_mode);
}

#[test]
fn cgb_mode_vbk_banks_vram() {
    let mut b = ic_cgb_mode();
    b.write(0x8000, 0x11);
    b.write(0xFF4F, 0x01);
    assert_eq!(b.read(0xFF4F), 0xFF);
    assert_eq!(b.read(0x8000), 0x00);
    b.write(0x8000, 0x22);
    b.write(0xFF4F, 0xFE); // only bit 0 matters
    assert_eq!(b.read(0x8000), 0x11);
    b.write(0xFF4F, 0x01);
    assert_eq!(b.read(0x8000), 0x22);
}

#[test]
fn cgb_mode_svbk_banks_wram() {
    let mut b = ic_cgb_mode();
    assert_eq!(b.read(0xFF70), 0xF8);
    for bank in 1..8u8 {
        b.write(0xFF70, bank);
        b.write(0xD000, 0xB0 + bank);
    }
    for bank in 1..8u8 {
        b.write(0xFF70, 0xF8 | bank); // upper bits ignored
        assert_eq!(b.read(0xFF70), 0xF8 | bank);
        assert_eq!(b.read(0xD000), 0xB0 + bank, "bank {bank}");
    }
    // Bank 0 selects bank 1; C000 region is always bank 0.
    b.write(0xFF70, 0x00);
    assert_eq!(b.read(0xD000), 0xB1);
    b.write(0xC000, 0x77);
    assert_eq!(b.read(0xC000), 0x77);
    assert_eq!(b.read(0xE000), 0x77);
    // Echo of D000 region follows the bank.
    b.write(0xFF70, 0x04);
    assert_eq!(b.read(0xF000), 0xB4);
}

#[test]
fn key1_speed_switch_via_stop() {
    // Register semantics only: `interrupt_pending = true` takes the
    // instantaneous-switch path (SameBoy gates the pause and the
    // skipped-byte read on !interrupt_pending), keeping the pause
    // machinery out of this test (covered separately below).
    let mut b = ic_cgb_mode();
    assert_eq!(b.read(0xFF4D), 0x7E);
    assert!(!b.stop(0x0000, true), "not armed: deep stop");
    b.write(0xFF4D, 0xFF);
    assert_eq!(b.read(0xFF4D), 0x7F);
    ticks(&mut b, 100);
    assert!(b.stop(0x0000, true), "armed: switch performed");
    assert_eq!(b.read(0xFF4D), 0xFE, "double speed, no longer armed");
    assert_eq!(b.read(0xFF04), 0x00, "STOP reset DIV");
    // Switch back.
    b.write(0xFF4D, 0x01);
    assert!(b.stop(0x0000, true));
    assert_eq!(b.read(0xFF4D), 0x7E);
}

/// With IE & IF pending an armed switch is instantaneous — no
/// skipped-byte read, no pause (SameBoy sm83_cpu.c stop() gates both
/// on !interrupt_pending; age caution/spsw-interrupts).
#[test]
fn speed_switch_with_pending_interrupt_takes_no_time() {
    let mut b = ic_cgb_mode();
    b.write(0xFF4D, 0x01);
    let c0 = b.cycles();
    assert!(b.stop(0x0000, true));
    assert_eq!(b.cycles() - c0, 0);
    assert_eq!(b.read(0xFF4D), 0xFE);
}

#[test]
fn stop_resets_div_on_dmg() {
    let mut b = ic(Model::Dmg);
    ticks(&mut b, 100);
    assert_ne!(b.read(0xFF04), 0);
    assert!(!b.stop(0x0000, true));
    assert_eq!(b.read(0xFF04), 0);
}

/// STOP's skipped byte costs one real read M-cycle when no interrupt
/// is pending (SameBoy sm83_cpu.c stop(): `cycle_read(gb, gb->pc++)`),
/// and none when one is (1-byte-opcode path).
#[test]
fn stop_skipped_byte_costs_one_read_cycle() {
    let mut b = ic(Model::Dmg);
    let c0 = b.cycles();
    assert!(!b.stop(0x0000, false));
    assert_eq!(b.cycles() - c0, 4, "one read M-cycle");
    let c0 = b.cycles();
    assert!(!b.stop(0x0000, true));
    assert_eq!(b.cycles() - c0, 0, "pending interrupt: no read");
}

/// The STOP-triggered switch pauses the CPU while the rest of the
/// machine runs: ~0x8000 M-cycles measured on the *new* clock
/// (gambatte memory.cpp Memory::stop:
/// `intreq_.setEventTime<intevent_unhalt>(cc + 0x20000 + 4)` with cc
/// counting 4 per M-cycle at either speed — so the dot cost doubles
/// when leaving double speed; the gambatte speedchange LY families
/// pin that asymmetry against SameBoy's flat 0x20008 8-MHz countdown).
#[test]
fn speed_switch_pause_advances_machine_on_the_new_clock() {
    let mut b = ic_cgb_mode();
    b.write(0xFF4D, 0x01);
    let c0 = b.cycles();
    assert!(b.stop(0x0000, false));
    // Read + internal cycle at the old pace (4 dots each, gambatte
    // re-paces the LCD at cc + 8 when entering), pause at the new.
    assert_eq!(b.cycles() - c0, 2 * 4 + 0x7FFF * 2);
    // Switching back re-paces from the read cycle on (cc + 0).
    b.write(0xFF4D, 0x01);
    let c0 = b.cycles();
    assert!(b.stop(0x0000, false));
    assert_eq!(b.cycles() - c0, 0x8001 * 4);
}

/// DIV restarts from the STOP reset and TIMA keeps counting M-cycles
/// through the pause: TAC=$04 (4096 Hz, +1 per 256 M-cycles) over
/// 0x8001 M-cycles yields exactly 0x80 (gambatte speedchange_tima00_1a
/// expects $80).
#[test]
fn speed_switch_pause_ticks_tima_from_div_reset() {
    let mut b = ic_cgb_mode();
    b.write(0xFF07, 0x04);
    b.write(0xFF4D, 0x01);
    assert!(b.stop(0x0000, false));
    assert_eq!(b.read_no_tick(0xFF05), 0x80);
}

/// The PPU keeps running through the pause: entering double speed
/// costs 65542 dots = 143 lines + 334 dots (speedchange_ly44_m3_ly:
/// LY 0x44 reads 0x39 = 0x44 + 143 mod 154 after the switch).
#[test]
fn speed_switch_pause_runs_the_ppu() {
    let mut b = ic_cgb_mode();
    b.write(0xFF40, 0x91);
    ticks(&mut b, 113); // glitched enable line is 452 dots: line 1 dot 0
    assert_eq!(b.read_no_tick(0xFF44), 1);
    b.write(0xFF4D, 0x01); // +4 dots (line 1 dot 4)
    assert!(b.stop(0x0000, false));
    // 65542 more dots: 143 full lines + 338 dots into line 144.
    assert_eq!(b.read_no_tick(0xFF44), 144);
}

/// IE & IF != 0 ends the pause early, exactly like halt mode
/// (gambatte's pause is a halt: the halted intevent_interrupts path
/// unhalts it).
#[test]
fn speed_switch_pause_cut_short_by_interrupt() {
    let mut b = ic_cgb_mode();
    b.write(0xFFFF, 0x04);
    b.write(0xFF07, 0x05); // 262144 Hz: +1 per 4 M-cycles
    b.write(0xFF05, 0xF0);
    b.write(0xFF4D, 0x01);
    let c0 = b.cycles();
    assert!(b.stop(0x0000, false));
    let elapsed_m = (b.cycles() - c0 - 8) / 2; // pause M-cycles
    assert!(elapsed_m < 0x100, "TIMA IRQ after ~64 M, got {elapsed_m}");
    assert_ne!(b.pending(), 0);
}

#[test]
fn double_speed_halves_dots_per_m_cycle() {
    let mut b = ic_cgb_mode();
    b.write(0xFF4D, 0x01);
    b.stop(0x0000, true);
    let c0 = b.cycles();
    b.tick();
    assert_eq!(b.cycles() - c0, 2, "2 dots per M-cycle in double speed");
    // LY advances half as fast: a 456-dot line takes 228 M-cycles.
    b.write(0xFF40, 0x91);
    ticks(&mut b, 226); // glitched enable line is 452 dots
    assert_eq!(b.read(0xFF44), 1);
}

// ---- CGB VRAM DMA -----------------------------------------------------

fn setup_gdma_regs(b: &mut Interconnect, src: u16, dst: u16) {
    b.write(0xFF51, (src >> 8) as u8);
    b.write(0xFF52, src as u8);
    b.write(0xFF53, (dst >> 8) as u8);
    b.write(0xFF54, dst as u8);
}

/// A GDMA write only *requests* the transfer; the copy steals the bus
/// at the head of the CPU's next machine cycle — 8 M-cycles per block
/// (2 bytes per M-cycle at normal speed) plus one teardown M-cycle
/// (gambatte memory.cpp dma(): `cc += 2 + 2 * doubleSpeed` per byte,
/// `cc += 4` at the end; see `service_vram_dma` for the seam).
#[test]
fn gdma_steals_the_next_machine_cycle_plus_teardown() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x40);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    let before = b.cycles();
    b.write(0xFF55, 0x03); // 4 blocks = 64 bytes, requested
    assert_eq!(b.cycles() - before, 4, "the write cycle only flags");
    assert_eq!(b.peek(0x8000), 0x00, "nothing copied yet");
    let before = b.cycles();
    b.tick(); // the steal precedes this op's own cycle
    assert_eq!(b.cycles() - before, (4 * 8 + 1 + 1) * 4, "stall + teardown");
    assert_eq!(b.peek(0x8000), 0x40);
    assert_eq!(b.peek(0x803F), 0x7F);
    assert_eq!(b.read(0xFF55), 0xFF, "completed");
    // HDMA1-4 are write-only.
    assert_eq!(b.read(0xFF51), 0xFF);
    assert_eq!(b.read(0xFF54), 0xFF);
}

#[test]
fn gdma_continues_from_incremented_addresses() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x00, 0x20);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF55, 0x00); // one block
    b.tick();
    b.write(0xFF55, 0x00); // next block continues at +0x10
    b.tick();
    assert_eq!(b.read(0x8010), 0x10);
    assert_eq!(b.read(0x801F), 0x1F);
}

/// FF51-FF54 write straight into the *live* DMA address counters
/// (gambatte memory.cpp cases 0x51-0x54: `dmaSource_ = data << 8 |
/// (dmaSource_ & 0xFF)` etc.; SameBoy's GB_IO_HDMA1-4 handlers agree):
/// rewriting only FF51 after blocks have copied keeps the incremented
/// low byte, so the next transfer reads from (new high byte | live low
/// byte), not from a fresh xx00.
#[test]
fn hdma_partial_src_rewrite_blends_live_counter() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x00, 0x30);
    fill_wram(&mut b, 0xD030, 0xA0, 0x10);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF55, 0x02); // 3 blocks: src counter is then 0xC030
    b.tick();
    b.write(0xFF51, 0xD0); // rewrite the high byte only
    b.write(0xFF55, 0x00); // 1 block: src 0xD030.., dst continues at 0x30
    b.tick();
    assert_eq!(b.read(0x8030), 0xA0, "live low byte kept: src 0xD030");
    assert_eq!(b.read(0x803F), 0xAF);
}

/// VRAM and 0xE000+ are not valid VRAM-DMA sources (Pan Docs "CGB
/// DMA"); the engine copies 0xFF instead of looping VRAM back into
/// itself (SameBoy GB_hdma_run only drives the bus for ROM/SRAM/WRAM
/// sources; everything else yields the idle data-bus value).
#[test]
fn gdma_invalid_sources_fill_destination_with_ff() {
    for src in [0x8000u16, 0x9000, 0xE000, 0xF000] {
        let mut b = ic_cgb_mode();
        // Distinct data at the would-be source and the destination.
        b.write(0x8000, 0x12);
        b.write(0x9000, 0x34);
        for i in 0..16 {
            b.write(0x9800 + i, 0x55);
        }
        setup_gdma_regs(&mut b, src, 0x1800);
        b.write(0xFF55, 0x00); // one block
        b.tick();
        for i in 0..16 {
            assert_eq!(b.read(0x9800 + i), 0xFF, "src {src:04X} byte {i}");
        }
    }
}

/// The destination is a full 16-bit counter: a transfer reaching
/// 0x10000 terminates there with FF55 bit 7 latched — it does *not*
/// wrap back into VRAM (gambatte memory.cpp dma(): `if (dmaDest +
/// length >= 0x10000) { length = 0x10000 - dmaDest; ioamhram_[0x155]
/// |= 0x80; }`, hardware-captured by gambatte dma/dma_dst_wrap_2;
/// FF53 keeps the full high byte, masked only at the VRAM write).
/// This replaces the earlier SameBoy-derived wrap-to-0x8000 model,
/// which that capture contradicts.
#[test]
fn gdma_terminates_at_dest_0x10000_crossing() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x20);
    setup_gdma_regs(&mut b, 0xC000, 0xFFF0);
    b.write(0xFF55, 0x01); // 2 blocks requested, only one fits
    b.tick();
    assert_eq!(b.peek(0x9FF0), 0x40, "dest 0xFFF0 masks to VRAM 0x1FF0");
    assert_eq!(b.peek(0x9FFF), 0x4F);
    assert_eq!(b.peek(0x8000), 0x00, "no wrap into a second block");
    // With the display off the truncated GDMA still retires its whole
    // length (gambatte dma(): `if (!(lcdc & en) && gdmaReqFlagged)
    // dmaLength = 0`), reading back $FF.
    assert_eq!(b.read(0xFF55), 0xFF);
}

#[test]
fn hblank_dma_one_block_per_hblank() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x20);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91); // LCD on: glitched line, hblank from ~dot 250
    b.write(0xFF55, 0x81); // hblank DMA, 2 blocks (PPU at dot 4)
    assert_eq!(b.read(0xFF55), 0x01, "2 blocks remaining reads 1");
    assert_eq!(b.peek(0x8000), 0x00, "nothing copied before hblank");
    // Run into the glitched line's hblank; the block transfer steals
    // 8 M-cycles + 1 teardown at the next boundary.
    ticks(&mut b, 90); // ~dot 400 incl. the stall
    assert_eq!(b.read(0xFF55), 0x00, "one block left");
    assert_eq!(b.peek(0x8000), 0x40);
    assert_eq!(b.peek(0x800F), 0x4F);
    assert_eq!(b.peek(0x8010), 0x00, "second block waits for next hblank");
    // Run well into line 1's hblank.
    ticks(&mut b, 100);
    assert_eq!(b.read(0xFF55), 0xFF, "done");
    assert_eq!(b.peek(0x8010), 0x50);
    assert_eq!(b.peek(0x801F), 0x5F);
}

/// Cancelling latches bit 7 plus the *written* length bits — the
/// FF55 write replaces the live count before the cancel takes effect
/// (gambatte memory.cpp case 0x55: `ioamhram_[0x155] = data & 0x7F`
/// precedes the `|= 0x80`; SameBoy sets hdma_steps_left first, too).
#[test]
fn hblank_dma_cancel_sets_bit7_and_latches_written_length() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x80);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    b.write(0xFF55, 0x87); // 8 blocks
    ticks(&mut b, 90); // first hblank: one block done
    assert_eq!(b.read(0xFF55), 0x06);
    b.write(0xFF55, 0x02); // cancel, writing length bits 0x02
    assert_eq!(b.read(0xFF55), 0x82, "bit 7 + the written length bits");
    ticks(&mut b, 101); // into line 1's hblank
    assert_eq!(b.peek(0x8010), 0x00, "no further blocks after cancel");
}

/// Enabling HBlank DMA with the LCD off copies one block immediately
/// and leaves the transfer armed (gambatte video.cpp enableHdma's
/// LCD-off branch flags a request at once; SameBoy GB_IO_HDMA5:
/// `(STAT & 3) == 0 && display_state != 7 → hdma_on = true`).
#[test]
fn hblank_enable_with_lcd_off_copies_one_block_immediately() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x20);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF55, 0x81); // LCD is off
    b.tick();
    assert_eq!(b.peek(0x8000), 0x40);
    assert_eq!(b.peek(0x800F), 0x4F);
    assert_eq!(b.peek(0x8010), 0x00, "exactly one block");
    assert_eq!(b.read(0xFF55), 0x00, "armed, one block left");
    // The remaining block fires at the first mode-0 entry after the
    // display comes on.
    b.write(0xFF40, 0x91);
    ticks(&mut b, 90);
    assert_eq!(b.peek(0x8010), 0x50);
    assert_eq!(b.read(0xFF55), 0xFF, "completed");
}

/// Enabling HBlank DMA inside the hblank window fires the first block
/// in that same hblank; within 3 dots of the line end it waits for
/// the next one (gambatte video.cpp enableHdma →
/// `isHdmaPeriod(...)`: `ly < 144 && cc + 3 + 3 * ds <
/// lyCounter.time() && cc >= m0TimeOfCurrentLy`).
#[test]
fn hblank_enable_inside_window_fires_immediately() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x20);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    while !b.ppu.hblank_active() {
        b.tick();
    }
    b.write(0xFF55, 0x80); // 1 block, enabled mid-hblank
    b.tick();
    assert_eq!(b.peek(0x8000), 0x40);
    assert_eq!(b.read(0xFF55), 0xFF, "completed in the same hblank");
}

/// The window cutoff: in double speed (2-dot M-cycles) an enable
/// landing 2 dots before the line end is outside the window and
/// waits for the next hblank.
#[test]
fn hblank_enable_past_window_cutoff_waits() {
    let mut b = ic_cgb_mode();
    b.write(0xFF4D, 0x01);
    b.stop(0x0000, true); // double speed, instantly
    fill_wram(&mut b, 0xC000, 0x40, 0x10);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    // Glitched enable line: 452 dots, hblank from ~dot 250. Park 2
    // dots before its end (dot 450 = 225 double-speed M-cycles).
    ticks(&mut b, 224);
    assert!(b.ppu.hblank_active(), "still in the glitch line's hblank");
    b.write(0xFF55, 0x80); // PPU at dot 450: 2 dots left < 3-dot margin
    b.tick();
    assert_eq!(b.peek(0x8000), 0x00, "no block this close to line end");
    assert_eq!(b.read(0xFF55), 0x00, "armed, nothing copied");
    // The next line's mode-0 entry fires it.
    ticks(&mut b, 250);
    assert_eq!(b.peek(0x8000), 0x40);
}

/// The block/CPU-access race has M-cycle granularity: a block flagged
/// in an earlier M-cycle steals the bus at the head of the next bus
/// operation (the racing access loses), while an access whose own
/// tick contains the trigger still commits first (the gambatte
/// hdma_late_destl/_wrambank/_length `_1`/`_2` adjacent-cycle pairs:
/// shifting the same code by one cycle flips the winner).
#[test]
fn hblank_block_race_has_machine_cycle_granularity() {
    // Calibrate: machine cycles from arming to the trigger dot.
    let lead_ticks = {
        let mut b = ic_cgb_mode();
        fill_wram(&mut b, 0xC000, 0x40, 0x10);
        setup_gdma_regs(&mut b, 0xC000, 0x0000);
        b.write(0xFF40, 0x91);
        b.write(0xFF55, 0x80);
        let mut n = 0u32;
        while !b.ppu.hdma_trigger_level() {
            b.tick();
            n += 1;
        }
        n
    };
    // Trigger during tick N, dest write afterwards: the steal heads
    // the write — the block uses the old destination.
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x10);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    b.write(0xFF55, 0x80);
    ticks(&mut b, lead_ticks);
    b.write(0xFF53, 0x90);
    assert_eq!(b.peek(0x8000), 0x40, "block first: old dest");
    assert_eq!(b.peek(0x9000), 0x00);
    // Trigger inside the write's own tick: the write commits first
    // and the block uses the new destination.
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x10);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    b.write(0xFF55, 0x80);
    ticks(&mut b, lead_ticks - 1);
    b.write(0xFF53, 0x90); // this op's tick contains the trigger
    b.tick(); // the steal happens here
    assert_eq!(b.peek(0x9000), 0x40, "write first: new dest");
    assert_eq!(b.peek(0x8000), 0x00);
}

/// HBlank DMA never proceeds while the core clock is gated: a block
/// flagged before HALT is deferred and re-flagged at wake, where it
/// copies without the teardown M-cycle (gambatte Memory::halt →
/// haltHdmaState_ = hdma_requested; video.h flagHdmaReq is suppressed
/// while halted; Memory::event intevent_dma: `cc -= 4` for the
/// deferred block).
#[test]
fn hblank_block_defers_while_core_clock_gated() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x10);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    b.write(0xFF55, 0x80);
    // Stop on the tick that flags the block (the trigger leads the
    // hblank by one dot) so the clock gate lands before any bus op
    // can service the request.
    while !b.ppu.hdma_trigger_level() {
        b.tick();
    }
    b.set_cpu_halted(true);
    ticks(&mut b, 300); // crosses further hblanks: nothing copies
    assert_eq!(b.peek(0x8000), 0x00);
    assert_eq!(b.read_no_tick(0xFF55), 0x00, "still armed");
    b.set_cpu_halted(false); // wake re-flags the deferred block
    let before = b.cycles();
    b.tick(); // the steal heads this op
    assert_eq!(b.cycles() - before, (8 + 1) * 4, "no teardown cycle");
    assert_eq!(b.peek(0x8000), 0x40);
    assert_eq!(b.read_no_tick(0xFF55), 0xFF);
}

/// A halt that begins *outside* the hblank window fires a block on a
/// wake landing inside one; a halt that begins inside it does not
/// retrigger the same hblank (gambatte haltHdmaState_ low vs high).
#[test]
fn halt_wake_inside_hblank_window_fires_block_once() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x10);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    b.write(0xFF55, 0x80);
    // Halt right after arming, before the first hblank (state Low).
    b.set_cpu_halted(true);
    while !b.ppu.hblank_active() {
        b.tick();
    }
    b.set_cpu_halted(false); // wake inside the window: block fires
    b.tick();
    assert_eq!(b.peek(0x8000), 0x40);
    // Re-arm inside the same hblank, halt, wake immediately: the halt
    // began inside the window (state High) — no retrigger.
    setup_gdma_regs(&mut b, 0xC000, 0x0010);
    assert!(b.ppu.hblank_active());
    b.write(0xFF55, 0x80);
    // (the enable itself fired a request: let it run, then re-halt)
    b.tick();
    assert_eq!(b.peek(0x8010), 0x40);
}

/// Disabling the display kills an armed HBlank transfer: FF55 keeps
/// reading "active" but no further block ever copies, even after the
/// display returns (gambatte video.cpp lcdcChange: the disable branch
/// parks every memevent, and only an armed-while-off transfer is
/// re-anchored by the enable branch).
#[test]
fn lcd_disable_kills_hblank_arming_but_not_ff55() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x20);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    b.write(0xFF55, 0x81); // armed with the LCD on, before any hblank
    b.write(0xFF40, 0x11); // display off
    ticks(&mut b, 300);
    assert_eq!(b.peek(0x8000), 0x00, "arming died with the display");
    assert_eq!(b.read(0xFF55), 0x01, "FF55 reads active (stale)");
    b.write(0xFF40, 0x91); // re-enabling does not revive it
    ticks(&mut b, 500);
    assert_eq!(b.peek(0x8000), 0x00);
}

/// The pending-block × speed-switch matrix (gambatte Memory::stop):
/// entering double speed the request survives into the pause and the
/// gated service aborts the transfer with the count latched; leaving
/// double speed it is deferred and completes normally after the pause
/// (hdma_transition_speedchange_hdmalen*_hdma5 = $80|len vs
/// hdma_late_m3speedchange_hdma5_*_ds_1 = still active).
#[test]
fn speed_switch_aborts_pending_hblank_block_entering_double_speed() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x40, 0x20);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    b.write(0xFF4D, 0x01); // arm first: any later bus op would
    b.write(0xFF55, 0x81); // service the request (2 blocks)
    while !b.ppu.hdma_trigger_level() {
        b.tick();
    }
    // The request flagged during the last tick is still pending when
    // STOP executes (gambatte: prefetched = hdmaReqFlagged).
    assert!(b.stop(0x0000, false));
    assert_eq!(b.peek(0x8000), 0x40, "the block still copied");
    assert_eq!(b.peek(0x800F), 0x4F);
    assert_eq!(b.read(0xFF55), 0x81, "aborted: bit 7 + armed count");
    ticks(&mut b, 300);
    assert_eq!(b.peek(0x8010), 0x00, "no further blocks");
}

#[test]
fn speed_switch_defers_pending_hblank_block_leaving_double_speed() {
    let mut b = ic_cgb_mode();
    b.write(0xFF4D, 0x01);
    assert!(b.stop(0x0000, true)); // enter double speed instantly
    fill_wram(&mut b, 0xC000, 0x40, 0x20);
    setup_gdma_regs(&mut b, 0xC000, 0x0000);
    b.write(0xFF40, 0x91);
    b.write(0xFF4D, 0x01); // arm first (see the abort test above)
    b.write(0xFF55, 0x81);
    while !b.ppu.hdma_trigger_level() {
        b.tick();
    }
    assert!(b.stop(0x0000, false)); // back to normal speed, with pause
    assert_eq!(b.read_no_tick(0xFF55), 0x01, "still active");
    assert_eq!(b.peek(0x8000), 0x00, "block deferred across the pause");
    b.tick();
    assert_eq!(b.peek(0x8000), 0x40);
    assert_eq!(b.read_no_tick(0xFF55), 0x00);
}

// ---- OAM DMA x VRAM DMA bus composition -------------------------------

/// While a VRAM DMA owns the bus, a concurrently running OAM DMA keeps
/// advancing one position per M-cycle but performs no source reads of
/// its own: each advance latches the VRAM DMA's bus traffic instead,
/// writing the stolen byte to OAM[hdma_src & 0xFF] — the *address* the
/// VRAM DMA drove, not the OAM DMA's own position (gambatte-core
/// memory.cpp `dma()`: `ioamhram_[src & 0xFF] = data` once per 4 cc,
/// gated `cc - 3 > lOamDmaUpdate`, which at normal speed lands the
/// advance on the *second* byte of each 2-byte stolen M-cycle —
/// hardware-pinned by dma/hdma_transition_oamdma_1's 50 9E 52 9C and
/// oamdma/oamdmasrcC000_hdmasrc0000's single 94 capture).
#[test]
fn vram_dma_steal_advances_oam_dma_capturing_the_bus() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x50, 0xA0);
    for i in 0..0xA0 {
        b.write(0xFE00 + i, 0xF0);
    }
    setup_gdma_regs(&mut b, 0x1000, 0x0000); // ROM pattern i ^ 0x5A
    b.write(0xFF46, 0xC0); // cycle W: OAM DMA from WRAM
    ticks(&mut b, 5); // W+2..W+5 copy idx 0..3
    b.write(0xFF55, 0x00); // W+6 copies idx 4, then flags 1 GDMA block
    b.tick(); // steal: 8 M-cycles (idx 5..12 advance) + teardown (idx 13)
    for _ in 0..160 {
        b.tick(); // let the transfer finish
    }
    let rom = |i: u8| i ^ 0x5A;
    // Positions copied normally before the steal, even slots: kept.
    assert_eq!(b.peek(0xFE00), 0x50);
    assert_eq!(b.peek(0xFE02), 0x52);
    assert_eq!(b.peek(0xFE04), 0x54);
    // Captures land at OAM[src & 0xFF] of the second stolen byte of
    // each M-cycle — the odd HDMA source offsets — overwriting the
    // earlier normal copies of idx 1/3.
    assert_eq!(b.peek(0xFE01), rom(0x01), "capture over earlier copy");
    assert_eq!(b.peek(0xFE03), rom(0x03), "capture over earlier copy");
    // Positions 5..12 advanced during the steal without copying their
    // own source: odd ones hold captures, even ones keep the prefill.
    assert_eq!(b.peek(0xFE05), rom(0x05));
    assert_eq!(b.peek(0xFE07), rom(0x07));
    assert_eq!(b.peek(0xFE09), rom(0x09));
    assert_eq!(b.peek(0xFE0B), rom(0x0B));
    for i in [0x06u16, 0x08, 0x0A, 0x0C] {
        assert_eq!(b.peek(0xFE00 + i), 0xF0, "idx {i:#x} skipped");
    }
    // Captures at offsets 0x0D/0x0F are overwritten again by the
    // normal copies resuming at idx 13 (teardown cycle onward).
    assert_eq!(b.peek(0xFE0D), 0x5D);
    assert_eq!(b.peek(0xFE0F), 0x5F);
    assert_eq!(b.peek(0xFE10), 0x60);
    assert_eq!(b.peek(0xFE9F), 0xEF);
}

/// A captured bus byte whose address low byte is ≥ 0xA0 lands in the
/// CGB-C extra OAM RAM behind FEA0-FEFF, decoded with the same bits
/// 3-4 alias (gambatte memory.cpp dma(): `ioamhram_[p & 0xE7] = data`
/// for `p >= oam_size`, skipped on AGB).
#[test]
fn vram_dma_steal_capture_reaches_extra_oam_ram() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x50, 0xA0);
    setup_gdma_regs(&mut b, 0x10A0, 0x0000);
    b.write(0xFF46, 0xC0);
    ticks(&mut b, 5);
    b.write(0xFF55, 0x00);
    b.tick();
    for _ in 0..170 {
        b.tick(); // transfer done, OAM idle again
    }
    // Captures land at odd offsets 0xA1..0xAF; the bits-3/4 alias
    // folds 0xA9/0xAB onto the 0xA1/0xA3 cells, so the later capture
    // wins each cell.
    assert_eq!(b.read(0xFEA1), 0xA9 ^ 0x5A);
    assert_eq!(b.read(0xFEA3), 0xAB ^ 0x5A);
    assert_eq!(b.read(0xFEA9), 0xA9 ^ 0x5A, "bits 3-4 alias");
}

/// In double speed the VRAM DMA copies one byte per stolen M-cycle, so
/// *every* stolen byte advances the OAM DMA and is captured (gambatte
/// dma(): `cc += 2 + 2 * doubleSpeed` per byte vs the 4-cc advance
/// period).
#[test]
fn vram_dma_steal_captures_every_byte_in_double_speed() {
    let mut b = ic_cgb_mode();
    b.write(0xFF4D, 0x01);
    assert!(b.stop(0x0000, true)); // enter double speed instantly
    fill_wram(&mut b, 0xC000, 0x50, 0xA0);
    for i in 0..0xA0 {
        b.write(0xFE00 + i, 0xF0);
    }
    setup_gdma_regs(&mut b, 0x1000, 0x0000);
    b.write(0xFF46, 0xC0);
    ticks(&mut b, 5);
    b.write(0xFF55, 0x00);
    b.tick(); // steal: 16 M-cycles, one advance + capture per byte
    for _ in 0..160 {
        b.tick();
    }
    // All 16 block offsets captured — including 0..=4, whose earlier
    // normal copies are overwritten; positions 5..=20 advanced during
    // the steal, so none of the captures is re-copied afterwards.
    for i in 0..16u16 {
        assert_eq!(b.peek(0xFE00 + i), (i as u8) ^ 0x5A, "offset {i:#x}");
    }
    // Positions 16..=20 advanced during the steal too: no capture
    // (the block only drove offsets 0..=15), no copy — prefill stays.
    for i in 16..21u16 {
        assert_eq!(b.peek(0xFE00 + i), 0xF0, "idx {i:#x} skipped");
    }
    assert_eq!(b.peek(0xFE15), 0x65, "normal copies resume at idx 21");
}

/// A block serviced while the core clock is gated (the speed-switch
/// pause) advances nothing: the OAM DMA controller is frozen with the
/// CPU (gambatte dma(): the advance is gated on `!halted()`).
#[test]
fn vram_dma_steal_does_not_advance_a_halt_frozen_oam_dma() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x50, 0xA0);
    for i in 0..0xA0 {
        b.write(0xFE00 + i, 0xF0);
    }
    setup_gdma_regs(&mut b, 0x1000, 0x0000);
    b.write(0xFF46, 0xC0);
    ticks(&mut b, 5); // idx 0..3 copied
    b.set_cpu_halted(true);
    b.vram_dma_req = Some(VramDmaReq::Gdma);
    b.run_vram_dma();
    assert_eq!(b.peek(0xFE01), 0x51, "no capture while frozen");
    assert_eq!(b.peek(0xFE05), 0xF0, "no position consumed");
    assert_eq!(b.dma_run.unwrap().idx, 4, "frozen position kept");
    b.set_cpu_halted(false);
    ticks(&mut b, 170);
    assert_eq!(b.peek(0xFE05), 0x55, "transfer resumed normally");
    assert_eq!(b.peek(0xFE9F), 0xEF);
}

/// The OAM DMA setup delay keeps counting during a steal: the start
/// promotion happens on a stolen advance, which captures instead of
/// copying byte 0 (gambatte dma(): `if (oamDmaPos_ == oamDmaStartPos_)
/// startOamDma(...)` inside the steal loop).
#[test]
fn vram_dma_steal_counts_oam_dma_startup_delay() {
    let mut b = ic_cgb_mode();
    fill_wram(&mut b, 0xC000, 0x50, 0xA0);
    for i in 0..0xA0 {
        b.write(0xFE00 + i, 0xF0);
    }
    setup_gdma_regs(&mut b, 0x1000, 0x0000);
    b.write(0xFF46, 0xC0); // cycle W: delay = 1 at commit
    b.write(0xFF55, 0x00); // W+1 ticks delay to 0, then flags the GDMA
    b.tick(); // steal precedes this cycle: the start promotes inside it
    for _ in 0..170 {
        b.tick();
    }
    // Steal advance 1 (2nd stolen byte, offset 1): promote, idx 0
    // consumed by the capture at OAM[1]. Advances 2..8: idx 1..7
    // consumed, captures at offsets 3/5/7/9/B/D/F. Normal copies
    // resume at idx 8 (teardown cycle), overwriting captures 9/B/D/F.
    assert_eq!(b.peek(0xFE00), 0xF0, "byte 0's copy was stolen");
    assert_eq!(b.peek(0xFE01), 0x01 ^ 0x5A, "capture during promote");
    assert_eq!(b.peek(0xFE03), 0x03 ^ 0x5A);
    assert_eq!(b.peek(0xFE02), 0xF0, "idx 2 skipped (capture at 3)");
    assert_eq!(b.peek(0xFE07), 0x07 ^ 0x5A);
    assert_eq!(b.peek(0xFE08), 0x58, "normal copies resume at idx 8");
    assert_eq!(b.peek(0xFE09), 0x59, "capture at 9 re-copied");
}

// ---- peek (side-effect-free harness view) -----------------------------

/// `peek` takes `&self`: it ticks nothing and observes raw memory —
/// WRAM/echo, HRAM, OAM, IE — without advancing time.
#[test]
fn peek_reads_plain_memory_without_time() {
    let mut b = ic(Model::Dmg);
    b.write_no_tick(0xC123, 0x11);
    b.write_no_tick(0xFF80, 0x22);
    b.write_no_tick(0xFE05, 0x33);
    b.write_no_tick(0xFFFF, 0xE4);
    let cycles = b.cycles();
    assert_eq!(b.peek(0xC123), 0x11);
    assert_eq!(b.peek(0xE123), 0x11, "echo");
    assert_eq!(b.peek(0xFF80), 0x22);
    assert_eq!(b.peek(0xFE05), 0x33);
    assert_eq!(b.peek(0xFFFF), 0xE4);
    assert_eq!(b.cycles(), cycles, "no time passed");
}

/// `peek` is omniscient by design: it ignores the PPU's mode-based
/// VRAM/OAM lockout that makes a real CPU read return $FF.
#[test]
fn peek_ignores_ppu_access_blocking() {
    let mut b = ic(Model::Dmg);
    b.write_no_tick(0x8500, 0x44);
    b.write_no_tick(0xFE00, 0x55);
    b.write(0xFF40, 0x91); // LCD on
    // Into mode 3 of the glitched first line: VRAM and OAM locked.
    ticks(&mut b, (452 + 120) / 4);
    assert_eq!(b.read(0x8500), 0xFF, "real VRAM read: locked");
    assert_eq!(b.read(0xFE00), 0xFF, "real OAM read: locked");
    assert_eq!(b.peek(0x8500), 0x44);
    assert_eq!(b.peek(0xFE00), 0x55);
}

/// IO registers are not peekable; the whole FF00-FF7F range (and the
/// FEA0-FEFF prohibited area) reads $FF through `peek`.
#[test]
fn peek_io_reads_ff() {
    let mut b = ic(Model::Dmg);
    b.write(0xFF40, 0x91);
    assert_eq!(b.read(0xFF40), 0x91, "real IO read works");
    assert_eq!(b.peek(0xFF40), 0xFF, "peek does not");
    assert_eq!(b.peek(0xFF00), 0xFF);
    assert_eq!(b.peek(0xFF0F), 0xFF);
    assert_eq!(b.peek(0xFEA0), 0xFF);
}

/// `peek` follows the live VBK/SVBK banking on CGB.
#[test]
fn peek_follows_cgb_banking() {
    let mut b = ic_cgb_mode();
    b.write(0x8000, 0x11);
    b.write(0xFF4F, 0x01);
    b.write(0x8000, 0x22);
    assert_eq!(b.peek(0x8000), 0x22, "active VRAM bank");
    b.write(0xFF4F, 0x00);
    assert_eq!(b.peek(0x8000), 0x11);
    b.write(0xFF70, 0x03);
    b.write(0xD000, 0x33);
    b.write(0xFF70, 0x04);
    b.write(0xD000, 0x44);
    assert_eq!(b.peek(0xD000), 0x44, "active WRAM bank");
    assert_eq!(b.peek(0xF000), 0x44, "echo follows the bank");
    b.write(0xFF70, 0x03);
    assert_eq!(b.peek(0xD000), 0x33);
}

// ---- post-boot state ---------------------------------------------------

fn booted(model: Model) -> Interconnect {
    let mut b = ic(model);
    b.apply_post_boot_state();
    b
}

/// The boot ROM leaves its logo graphics in VRAM at hand-off: the
/// header logo decompressed into tiles $01-$18 (even bytes — one
/// bitplane), the (R) trademark tile at $19, and on DMG-family models
/// the two logo tile-map rows (gambatte initstate.cpp setInitialVram
/// hardware dump; the expected bytes below are that dump's prefix for
/// the standard Nintendo logo). mealybug m3_scx_low_3_bits renders
/// the leftover (R) tile.
#[test]
fn post_boot_vram_boot_logo_leftovers() {
    // The fixed logo applies regardless of the cart header (the boot
    // ROM locks up on a mismatch, so hardware VRAM only ever holds
    // the canonical image; gambatte's test carts have no header logo
    // and their references still show it).
    for model in [Model::Dmg, Model::Cgb] {
        let mut b = ic(model);
        b.apply_post_boot_state();
        // $CE -> F0 F0 FC FC, $ED -> FC FC F3 F3 (even bytes).
        for (off, want) in [
            (0x00u16, 0xF0u8),
            (0x02, 0xF0),
            (0x04, 0xFC),
            (0x06, 0xFC),
            (0x08, 0xFC),
            (0x0A, 0xFC),
            (0x0C, 0xF3),
            (0x0E, 0xF3),
            // $66 -> 3C 3C 3C 3C twice.
            (0x10, 0x3C),
            (0x16, 0x3C),
            (0x18, 0x3C),
            (0x1E, 0x3C),
        ] {
            assert_eq!(
                b.ppu().vram_read_raw(0x8010 + off),
                want,
                "{model:?} +{off:#x}"
            );
        }
        assert_eq!(b.ppu().vram_read_raw(0x8011), 0, "high bitplane untouched");
        // (R) trademark tile $19.
        assert_eq!(b.ppu().vram_read_raw(0x8190), 0x3C, "{model:?}");
        assert_eq!(b.ppu().vram_read_raw(0x8192), 0x42, "{model:?}");
        assert_eq!(b.ppu().vram_read_raw(0x8194), 0xB9, "{model:?}");
        assert_eq!(b.ppu().vram_read_raw(0x819E), 0x3C, "{model:?}");
        // The logo tile-map rows are deliberately not installed
        // (see install_boot_logo_vram): the pinned gambatte
        // reference PNGs encode a cleared map.
        assert_eq!(b.ppu().vram_read_raw(0x9904), 0x00, "{model:?}");
        assert_eq!(b.ppu().vram_read_raw(0x9910), 0x00, "{model:?}");
    }
}

/// Real DMG-family WRAM powers up in the $00/$FF half-page stripe
/// pattern, mirrored into D000-DFFF (gambatte-core mem_dumps.h
/// `setInitialDmgWram` base pattern; see `install_power_on_wram`).
/// The $DE00 page reading $FF is what the gambatte oamdma_srcFE00_*
/// expectations encode (OAM DMA from $FE00 reads the $DE00 echo).
/// CGB WRAM stays zero-filled.
#[test]
fn post_boot_wram_power_on_pattern() {
    for model in [Model::Dmg0, Model::Dmg, Model::Mgb, Model::Sgb, Model::Sgb2] {
        let b = booted(model);
        for (addr, want) in [
            (0xC000u16, 0x00u8),
            (0xC0FF, 0x00),
            (0xC100, 0xFF),
            (0xC1FF, 0xFF),
            (0xC2A0, 0x00),
            (0xC700, 0xFF),
            // Polarity inverts across the 2 KiB half...
            (0xC800, 0xFF),
            (0xC900, 0x00),
            (0xCE42, 0xFF),
            (0xCF00, 0x00),
            // ...and D000-DFFF mirrors C000-CFFF.
            (0xD000, 0x00),
            (0xD100, 0xFF),
            (0xDE00, 0xFF),
            (0xDEFF, 0xFF),
            (0xDF00, 0x00),
        ] {
            assert_eq!(b.peek(addr), want, "{model:?} {addr:04X}");
        }
    }
    let b = booted(Model::Cgb);
    for addr in [0xC100u16, 0xC800, 0xDE00] {
        assert_eq!(b.peek(addr), 0x00, "CGB WRAM zero-filled at {addr:04X}");
    }
}

/// The CGB boot ROM hands a CGB-flagged cart off 0x7D8 T-cycles
/// earlier than a DMG cart (the DMG-compat palette tail), shifting
/// DIV and the LCD phase together: DIV $1E9C pinned by gambatte
/// div/start_inc_1/2 (FF04 reads $1E at +96 T immediately before
/// the increment to $1F00) and tima/tc00_start_1/2 (first TIMA
/// increment, DIV bit-9 edge, exactly between rounds at +356), LY
/// $90 by display_startstate ly/stat. The DMG-cart side keeps
/// mooneye misc/boot_div-cgbABCDE's $2674 with the LCD 0x7D8 dots
/// further on (line 148, still in the pandocs#426 LY window).
#[test]
fn post_boot_cgb_cart_hands_off_earlier_than_dmg_cart() {
    let mut dmg_cart = booted(Model::Cgb);
    assert_eq!(dmg_cart.timer.div_counter(), 0x2674);
    assert_eq!(dmg_cart.read(0xFF44), 148);

    let mut cgb_cart = ic_cgb_mode();
    cgb_cart.apply_post_boot_state();
    let div = cgb_cart.timer.div_counter();
    assert_eq!(div, 0x1E9C);
    assert_eq!(div, 0x2674 - 0x7D8);
    // div/start_inc oracle: the read 24 M-cycles in.
    assert_eq!((div + 96) >> 8, 0x1E, "round 1 high byte");
    assert!(
        (div + 96) & 0xFF >= 0xFC,
        "immediately before the increment"
    );
    assert_eq!((div + 100) >> 8, 0x1F, "round 2 high byte");
    // tc00_start oracle: bit-9 falling edge between the rounds.
    assert_eq!((div + 356) % 0x400, 0);
    assert_eq!(cgb_cart.read(0xFF44), 144);
}

#[test]
fn post_boot_io_dmg() {
    let mut b = booted(Model::Dmg);
    assert_eq!(b.read(0xFF00), 0xCF);
    assert_eq!(b.read(0xFF02), 0x7E);
    assert_eq!(b.read(0xFF0F), 0xE1);
    assert_eq!(b.read(0xFF26), 0xF1, "channel 1 beep still on");
    assert_eq!(b.read(0xFF11), 0xBF);
    assert_eq!(b.read(0xFF12), 0xF3);
    assert_eq!(b.read(0xFF24), 0x77);
    assert_eq!(b.read(0xFF25), 0xF3);
    assert_eq!(b.read(0xFF40), 0x91);
    assert_eq!(b.read(0xFF47), 0xFC);
    assert_eq!(b.read(0xFF46), 0xFF);
    assert_eq!(b.read(0xFFFF), 0x00);
}

#[test]
fn post_boot_io_sgb() {
    let mut b = booted(Model::Sgb);
    assert_eq!(b.read(0xFF00), 0xFF, "P1 columns deselected on SGB");
    assert_eq!(b.read(0xFF26), 0xF0, "no boot beep on SGB");
}

#[test]
fn post_boot_io_cgb_dmg_cart() {
    let mut b = booted(Model::Cgb);
    assert_eq!(b.read(0xFF00), 0xFF);
    assert_eq!(b.read(0xFF02), 0x7E, "fast-clock bit absent in DMG mode");
    assert_eq!(b.read(0xFF26), 0xF1);
    assert_eq!(b.read(0xFF46), 0x00);
    assert_eq!(b.read(0xFF4D), 0xFF);
    assert_eq!(b.read(0xFF4F), 0xFE);
    assert_eq!(b.read(0xFF55), 0xFF);
    assert_eq!(b.read(0xFF68), 0xC8, "BCPS boot leftover");
    assert_eq!(b.read(0xFF69), 0xFF, "BCPD unreadable in DMG mode");
    assert_eq!(b.read(0xFF6A), 0xD0, "OCPS boot leftover");
    assert_eq!(b.read(0xFF6C), 0xFF, "OPRI = DMG-style priority");
    assert_eq!(b.read(0xFF70), 0xFF);
    assert_eq!(b.read(0xFF74), 0xFF);
    assert_eq!(b.read(0xFF75), 0x8F);
}

/// For DMG carts whose licensee is not Nintendo (no title-hash lookup),
/// the CGB boot ROM installs the *default* compatibility palette
/// combination — BG palette 0 = $7FFF/$1BEF/$6180/$0000, OBJ palettes 0
/// and 1 = $7FFF/$421F/$1CF2/$0000 (Pan Docs "Compatibility palettes";
/// SameBoy BootROMs/cgb_boot.asm default combination OBJ0=4, OBJ1=4,
/// BG=29). Pins that the BG table differs from the OBJ table and that
/// *both* OBJ slots receive it.
#[test]
fn post_boot_cgb_compat_palettes_are_boot_defaults() {
    fn le_bytes(table: [u16; 4]) -> [u8; 8] {
        let mut out = [0u8; 8];
        for (i, c) in table.into_iter().enumerate() {
            [out[2 * i], out[2 * i + 1]] = c.to_le_bytes();
        }
        out
    }
    for model in [Model::Cgb, Model::Agb] {
        let b = booted(model);
        let (bg, obj) = b.ppu.palette_ram();
        assert_eq!(
            bg[..8],
            le_bytes([0x7FFF, 0x1BEF, 0x6180, 0x0000]),
            "{model:?} BG palette 0"
        );
        let obj_table = le_bytes([0x7FFF, 0x421F, 0x1CF2, 0x0000]);
        assert_eq!(obj[..8], obj_table, "{model:?} OBJ palette 0");
        assert_eq!(obj[8..16], obj_table, "{model:?} OBJ palette 1");
    }
}

#[test]
fn post_boot_io_cgb_mode_cart() {
    let mut rom = test_rom();
    rom[0x143] = 0x80;
    let mut b = Interconnect::new(Model::Cgb, Cartridge::from_bytes(rom).unwrap());
    b.apply_post_boot_state();
    assert_eq!(b.read(0xFF4D), 0x7E);
    assert_eq!(b.read(0xFF02), 0x7C, "CGB-mode SC has the fast-clock bit");
    assert_eq!(b.read(0xFF6C), 0xFE, "OPRI = OAM index priority");
    assert_eq!(b.read(0xFF70), 0xF8);
    assert_eq!(b.read(0xFF56), 0x3E, "RP idle, not receiving");
}

/// Replicate acceptance/boot_div-dmgABCmgb: DIV reads at M-cycles 14,
/// 78, 141, 205, 269 and 334 after hand-off observe AC AD AD AE AF B1.
#[test]
fn post_boot_div_phase_dmg() {
    let mut b = booted(Model::Dmg);
    let mut cycle = 0u32;
    let mut read_at = |b: &mut Interconnect, m: u32| {
        while cycle + 1 < m {
            b.tick();
            cycle += 1;
        }
        cycle += 1;
        b.read(0xFF04)
    };
    let got = [14, 78, 141, 205, 269, 334].map(|m| read_at(&mut b, m));
    assert_eq!(got, [0xAC, 0xAD, 0xAD, 0xAE, 0xAF, 0xB1]);
}

/// SGB DIV depends on the header bits: an all-zero header yields 731
/// zero bits in the transferred packets -> DIV base + 4*731.
#[test]
fn post_boot_div_sgb_header_dependence() {
    let mut b = booted(Model::Sgb);
    // test_rom() header region 0x104-0x14F is all zeros: payload zeros =
    // 6 * 15 * 8 = 720, command bytes F1/F3/F5/F7/F9/FB add 11.
    assert_eq!(sgb_header_zero_bits(b.cartridge()), 731);
    // div = 0xD170 + 4 * 731 = 0xDCDC; the first read observes +4.
    assert_eq!(b.read(0xFF04), 0xDC);
}

/// Replicate the LY/STAT bytes of boot_hwio-dmgABCmgb: STAT read at
/// M-cycle 1139 is $80 (mode 0, line 9), LY read at 1190 is $0A.
#[test]
fn post_boot_lcd_phase_dmg() {
    let mut b = booted(Model::Dmg);
    ticks(&mut b, 1138);
    assert_eq!(b.read(0xFF41), 0x80);
    let mut b = booted(Model::Dmg);
    ticks(&mut b, 1189);
    assert_eq!(b.read(0xFF44), 0x0A);
}

/// boot_hwio-dmg0: STAT $83 (mode 3, line 1), LY $01.
#[test]
fn post_boot_lcd_phase_dmg0() {
    let mut b = booted(Model::Dmg0);
    ticks(&mut b, 1138);
    assert_eq!(b.read(0xFF41), 0x83);
    let mut b = booted(Model::Dmg0);
    ticks(&mut b, 1189);
    assert_eq!(b.read(0xFF44), 0x01);
}

/// The IF value survives until boot_hwio's read at M-cycle 285 (no
/// stray STAT/vblank bits from the warmed-up PPU).
#[test]
fn post_boot_if_stable() {
    for model in [Model::Dmg0, Model::Dmg, Model::Sgb, Model::Cgb] {
        let mut b = booted(model);
        ticks(&mut b, 284);
        assert_eq!(b.read(0xFF0F), 0xE1, "{model:?}");
    }
}

// ---- DMG OAM corruption bug (Pan Docs "OAM Corruption Bug") ------

/// Interconnect with the LCD freshly enabled (`ic` powers on with the
/// LCD off; the enable glitch line passes before any scan window).
fn ic_lcd_on(model: Model) -> Interconnect {
    let mut b = ic(model);
    b.write(0xFF40, 0x91);
    b
}

/// Distinct OAM fill through the DMA-engine path (ignores blocking,
/// takes no machine time).
fn fill_oam_distinct(b: &mut Interconnect) {
    for i in 0..0xA0u8 {
        b.ppu_mut().oam_dma_write(i, i ^ 0xA5);
    }
}

fn oam_snapshot(b: &Interconnect) -> [u8; 0xA0] {
    let mut snap = [0u8; 0xA0];
    for (i, byte) in snap.iter_mut().enumerate() {
        *byte = b.peek(0xFE00 + i as u16);
    }
    snap
}

/// Tick until the *next* M-cycle's access lands on scan row `row`
/// (every access advances the machine one M-cycle first, so park one
/// row short).
fn park_before_oam_row(b: &mut Interconnect, row: u8) {
    assert!((0x10..=0x98).contains(&row) && row % 8 == 0);
    for _ in 0..200_000 {
        if b.ppu.oam_bug_row() == Some(row - 8) {
            return;
        }
        b.tick();
    }
    panic!("scan row {row:#04x} never reached");
}

#[test]
fn oam_bug_read_in_mode2_corrupts_on_dmg_family_only() {
    for model in [Model::Dmg, Model::Dmg0, Model::Mgb, Model::Sgb, Model::Sgb2] {
        let mut b = ic_lcd_on(model);
        park_before_oam_row(&mut b, 0x20);
        fill_oam_distinct(&mut b);
        let before = oam_snapshot(&b);
        assert_eq!(b.read(0xFE00), 0xFF, "{model:?}: OAM still locked");
        let after = oam_snapshot(&b);
        // Read pattern at row 0x20: glitched word in rows 3 *and* 4,
        // row tail copied from row 3.
        let glitched = before[0x18] | (before[0x20] & before[0x1C]);
        assert_eq!(after[0x20], glitched, "{model:?}");
        assert_eq!(after[0x18], glitched, "{model:?}");
        assert_eq!(after[0x22..0x28], before[0x1A..0x20], "{model:?}");
        assert_eq!(after[..0x18], before[..0x18], "{model:?}: earlier rows");
    }
    for model in [Model::Cgb, Model::Agb] {
        let mut b = ic_lcd_on(model);
        park_before_oam_row(&mut b, 0x20);
        fill_oam_distinct(&mut b);
        let before = oam_snapshot(&b);
        b.read(0xFE00);
        assert_eq!(oam_snapshot(&b), before, "{model:?}: no bug on CGB");
    }
}

#[test]
fn oam_bug_triggers_across_the_whole_fexx_page_only() {
    // The trigger keys on the address byte $FE on the bus: the
    // FEA0-FEFF prohibited area corrupts like OAM proper (blargg
    // oam_bug/8-instr_effect pops from $FEF0), neighbours do not.
    let mut b = ic_lcd_on(Model::Dmg);
    park_before_oam_row(&mut b, 0x20);
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    b.read(0xFEA0);
    assert_ne!(oam_snapshot(&b), before, "prohibited-area read corrupts");
    for addr in [0xFDFF, 0xFF00] {
        let mut b = ic_lcd_on(Model::Dmg);
        park_before_oam_row(&mut b, 0x20);
        fill_oam_distinct(&mut b);
        let before = oam_snapshot(&b);
        b.read(addr);
        assert_eq!(oam_snapshot(&b), before, "read {addr:#06x} is inert");
    }
}

#[test]
fn oam_bug_write_corrupts_with_write_pattern_and_is_dropped() {
    let mut b = ic_lcd_on(Model::Dmg);
    park_before_oam_row(&mut b, 0x20);
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    b.write(0xFE21, 0x77);
    let after = oam_snapshot(&b);
    for i in 0..2 {
        let (a, p0, p2) = (before[0x20 + i], before[0x18 + i], before[0x1C + i]);
        assert_eq!(after[0x20 + i], ((a ^ p2) & (p0 ^ p2)) ^ p2, "byte {i}");
    }
    assert_eq!(after[0x22..0x28], before[0x1A..0x20], "row tail copied");
    assert!(
        !after.contains(&0x77),
        "the blocked CPU write must not land"
    );
}

#[test]
fn oam_bug_internal_cycle_value_corrupts_via_tick_addr() {
    // INC rr's internal cycle carries no memory access; the register
    // value alone triggers the write pattern (blargg oam_bug/2-causes).
    let mut b = ic_lcd_on(Model::Dmg);
    park_before_oam_row(&mut b, 0x20);
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    Bus::tick_addr(&mut b, 0xFE00);
    let after = oam_snapshot(&b);
    for i in 0..2 {
        let (a, p0, p2) = (before[0x20 + i], before[0x18 + i], before[0x1C + i]);
        assert_eq!(after[0x20 + i], ((a ^ p2) & (p0 ^ p2)) ^ p2, "byte {i}");
    }
    assert_eq!(after[0x22..0x28], before[0x1A..0x20]);
    // Out-of-range values are inert (blargg oam_bug/3-non_causes).
    let mut b = ic_lcd_on(Model::Dmg);
    park_before_oam_row(&mut b, 0x20);
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    Bus::tick_addr(&mut b, 0xFDFF);
    Bus::tick_addr(&mut b, 0xFF00);
    assert_eq!(oam_snapshot(&b), before);
}

#[test]
fn oam_bug_increase_read_uses_the_read_increase_pattern() {
    // POP/LD A,(HL+) style reads: special pattern at rows 4..=18
    // (SameBoy v0.12.1 GB_trigger_oam_bug_read_increase).
    let mut b = ic_lcd_on(Model::Dmg);
    park_before_oam_row(&mut b, 0x20);
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    assert_eq!(Bus::read_inc(&mut b, 0xFE05), 0xFF);
    let after = oam_snapshot(&b);
    let mut prev = [0u8; 8];
    prev.copy_from_slice(&before[0x18..0x20]);
    for i in 0..2 {
        let (a, p0, c, d) = (
            before[0x10 + i],
            before[0x18 + i],
            before[0x20 + i],
            before[0x1C + i],
        );
        prev[i] = (p0 & (a | c | d)) | (a & c & d);
    }
    for i in 0..8 {
        assert_eq!(after[0x10 + i], prev[i], "two rows back {i}");
        assert_eq!(after[0x18 + i], prev[i], "preceding row {i}");
        assert_eq!(after[0x20 + i], prev[i], "current row {i}");
    }
}

#[test]
fn oam_bug_suppressed_while_the_core_clock_is_gated() {
    // The halted CPU performs no bus accesses on hardware; the
    // discarded halt prefetch (see cpu::Bus docs) must stay
    // side-effect-free even with PC in $FExx.
    let mut b = ic_lcd_on(Model::Dmg);
    park_before_oam_row(&mut b, 0x20);
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    b.set_cpu_halted(true);
    b.read(0xFE00);
    assert_eq!(oam_snapshot(&b), before, "halted: no corruption");
    b.set_cpu_halted(false);
    park_before_oam_row(&mut b, 0x20);
    b.read(0xFE00);
    assert_ne!(oam_snapshot(&b), before, "running again: corruption");
}

#[test]
fn oam_bug_suppressed_while_oam_dma_copies() {
    // While the DMA engine owns OAM, CPU-side $FExx traffic does not
    // corrupt (the interplay is untested on hardware — SameBoy leaves
    // the same Todo — so the conservative gate wins). The DMA source
    // mirrors the OAM contents so the copy itself is invisible.
    let mut b = ic_lcd_on(Model::Dmg);
    for i in 0..0xA0u16 {
        b.write(0xC000 + i, (i as u8) ^ 0xA5);
    }
    park_before_oam_row(&mut b, 0x10);
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    b.write(0xFF46, 0xC0);
    b.tick(); // setup delay
    b.tick(); // first byte copies; the engine owns OAM from here
    b.read(0xFE00); // still inside the scan window (row 0x28)
    assert_eq!(oam_snapshot(&b), before);
}

#[test]
fn oam_bug_inert_outside_the_scan_window() {
    // blargg oam_bug/6-timing_no_bug: accesses bracketing the per-line
    // window, hammering vblank, and with the LCD off are all clean.
    let access_all = |b: &mut Interconnect| {
        let keep = b.peek(0xFE00);
        b.read(0xFE00);
        Bus::tick_addr(b, 0xFE00);
        Bus::read_inc(b, 0xFE00);
        b.write(0xFE00, keep); // may land outside mode 2/3: same value
    };
    // VBlank.
    let mut b = ic_lcd_on(Model::Dmg);
    while b.ppu.mode_bits() != 1 {
        b.tick();
    }
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    access_all(&mut b);
    assert_eq!(oam_snapshot(&b), before, "vblank");
    // Mode 3 (entered fresh, lasts >= 43 M-cycles).
    let mut b = ic_lcd_on(Model::Dmg);
    while b.ppu.mode_bits() != 2 {
        b.tick();
    }
    while b.ppu.mode_bits() != 3 {
        b.tick();
    }
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    access_all(&mut b);
    assert_eq!(oam_snapshot(&b), before, "mode 3");
    // HBlank right after that mode 3.
    while b.ppu.mode_bits() != 0 {
        b.tick();
    }
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    access_all(&mut b);
    assert_eq!(oam_snapshot(&b), before, "hblank");
    // LCD off.
    let mut b = ic_lcd_on(Model::Dmg);
    b.write(0xFF40, 0x00);
    fill_oam_distinct(&mut b);
    let before = oam_snapshot(&b);
    access_all(&mut b);
    assert_eq!(oam_snapshot(&b), before, "LCD off");
}
