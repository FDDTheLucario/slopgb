//! Tests for the decoupled mode timeline: each pins a concrete number against
//! SameBoy 1.0.2 `display.c` (asserting the spec value, not the function's own
//! definition).

use super::*;

/// `display.c:1493`: visible mode→0 sits at `80 + 167 + (SCX & 7)` over the
/// 80-dot mode 2, extended by sprite/window penalties.
#[test]
fn visible_mode0_dot_matches_sameboy_mode3_length() {
    assert_eq!(ModeTimeline::bare(1, 0).visible_mode0_dot(), 247);
    assert_eq!(ModeTimeline::bare(1, 5).visible_mode0_dot(), 252);
    assert_eq!(
        ModeTimeline::with_penalty(1, 2, 6).visible_mode0_dot(),
        247 + 2 + 6
    );
}

/// `display.c:2091` (step A) vs `:2108` (step C): the mode-0 STAT IRQ fires
/// exactly one dot AFTER the visible STAT mode flips 3→0.
#[test]
fn mode0_irq_trails_visible_mode0_by_one_dot() {
    let t = ModeTimeline::bare(1, 0);
    assert_eq!(t.mode0_irq_dot(), 248);
    assert_eq!(t.mode0_irq_dot(), t.visible_mode0_dot() + 1);
}

/// `display.c:1787` vs `:1792` ("OAM int 1 T-cycle early") and the `:1778`
/// "except on line 0" exception: the mode-2 STAT IRQ leads its visible edge by
/// one dot on lines 1-143, but not on line 0.
#[test]
fn mode2_irq_leads_by_one_dot_except_line_0() {
    assert_eq!(ModeTimeline::bare(1, 0).mode2_irq_offset(), -1);
    assert_eq!(ModeTimeline::bare(143, 0).mode2_irq_offset(), -1);
    assert_eq!(
        ModeTimeline::bare(0, 0).mode2_irq_offset(),
        0,
        "line 0: no early fire"
    );
}

/// The two-field decoupling (`gb.h:612` `mode_for_interrupt` vs `io[STAT]&3`):
/// on the dot the visible mode reads 0, the interrupt-facing mode still holds
/// 3 — the one-dot gap the whole-dot `vis_mode` cannot represent — and they
/// re-converge to 0 once the mode-0 IRQ has fired.
#[test]
fn visible_and_interrupt_mode_decoupled_at_boundary() {
    let t = ModeTimeline::bare(1, 0);
    let b = t.visible_mode0_dot();
    assert_eq!(
        t.visible_mode(b),
        MODE_HBLANK,
        "visible mode reads 0 at the boundary"
    );
    assert_eq!(t.mode_for_interrupt(b), MODE_XFER, "interrupt mode still 3");
    assert_eq!(t.visible_mode(b - 1), MODE_XFER);
    assert_eq!(
        t.mode_for_interrupt(t.mode0_irq_dot()),
        MODE_HBLANK,
        "re-converge after the IRQ"
    );
}

/// The kernel-resolver MECHANISM, at the model level:
/// the anchors swing 2 dots (mode-2 IRQ −1, mode-0 IRQ +1 from their visible
/// edges), and a 2-dot read separation straddles the mode-3→0 edge — so two
/// equal-latency reads anchored on the two IRQs land on opposite sides (mode 3
/// vs mode 0) with no call-stack discriminator. This is a unit-level proof that
/// the decoupled structure *can* represent what the whole-dot model collapses;
/// the full per-ROM dispatch+ISR latency is validated
/// end-to-end only by the wired port against the actual ROMs.
#[test]
fn kernel_pair_separates_on_the_decoupled_grid() {
    let t = ModeTimeline::bare(1, 0);
    let boundary = t.visible_mode0_dot();

    let m0_swing = (i32::from(t.visible_mode0_dot()) - i32::from(t.mode0_irq_dot())).unsigned_abs();
    let m2_swing = i32::from(t.mode2_irq_offset()).unsigned_abs(); // visible mode-2 edge is dot 0
    assert_eq!(
        m0_swing + m2_swing,
        2,
        "the mode-2/mode-0 anchors swing 2 dots total"
    );

    assert_eq!(
        t.visible_mode(boundary - 2),
        MODE_XFER,
        "m2int-side read: mode 3 (out3)"
    );
    assert_eq!(
        t.visible_mode(boundary),
        MODE_HBLANK,
        "m0int-side read: mode 0 (out0)"
    );
    assert_ne!(
        t.visible_mode(boundary - 2),
        t.visible_mode(boundary),
        "the kernel pair the whole-dot model collapses onto one value is separable here"
    );
}

/// The kernel separation pinned to the EMULATOR-MEASURED read dots: under the
/// leading-edge path the two ROMs' `ldh a,(FF41)`
/// land at our-line dots **248** (`m2int_m3stat_1`) and **252**
/// (`m0int_m3stat_2`) on a bare CGB line (SCX 0). Mapped onto this
/// `ModeTimeline` (whose dot 0 = the mode-2 start, i.e. our-line dot
/// [`OUR_MODE2_START`]), those reads straddle [`ModeTimeline::visible_mode0_dot`]
/// — proving the *faithful SameBoy frame* separates the pair the whole-dot model
/// collapses, with no call-stack discriminator. It also pins the residual: our
/// PRODUCTION visible flip sits at our-line dot 254 (= mt 250), **3 dots later**
/// than SameBoy's mt-247 boundary — so in production both reads land before the
/// flip (both mode 3); matching SameBoy would back-date the visible flip those
/// 3 dots so it slots between the two reads.
#[test]
fn measured_kernel_reads_straddle_sameboy_visible_boundary() {
    // Our `vis_mode` puts the visible mode-2 start at our-line dot 4 (dots 0-3
    // are the line-start window); the `ModeTimeline` spine starts at its dot 0.
    const OUR_MODE2_START: u16 = 4;
    // The leading-edge read dots (bare, SCX 0).
    const M2INT_READ_OUR_DOT: u16 = 248;
    const M0INT_READ_OUR_DOT: u16 = 252;
    // Our production visible mode→0 flip dot (pipe end 256 − 2; `m0_flip_events`).
    const OUR_VISIBLE_FLIP_DOT: u16 = 254;

    let t = ModeTimeline::bare(1, 0);
    let m2_mt = M2INT_READ_OUR_DOT - OUR_MODE2_START;
    let m0_mt = M0INT_READ_OUR_DOT - OUR_MODE2_START;

    // On the faithful frame the two equal-`ldh` reads land on opposite sides of
    // the visible 3→0 boundary — m2int reads 3 (out3), m0int reads 0 (out0).
    assert_eq!(t.visible_mode(m2_mt), MODE_XFER, "m2int → mode 3 (out3)");
    assert_eq!(t.visible_mode(m0_mt), MODE_HBLANK, "m0int → mode 0 (out0)");

    // The boundary sits strictly between the two reads — the open interval
    // (m2int, m0int] the back-dated visible flip must land in.
    let b = t.visible_mode0_dot();
    assert!(
        m2_mt < b && b <= m0_mt,
        "SameBoy visible_mode0_dot {b} separates the reads ({m2_mt}, {m0_mt}]"
    );

    // Residual: our production flip (mt 250) is 3 dots past SameBoy's (mt 247),
    // which collapses BOTH measured reads onto mode 3 (the whole-dot floor).
    let our_flip_mt = OUR_VISIBLE_FLIP_DOT - OUR_MODE2_START;
    assert_eq!(our_flip_mt - b, 3, "production visible flip is 3 dots late");
    assert!(
        m2_mt < our_flip_mt && m0_mt < our_flip_mt,
        "at the production flip dot both reads are still mode 3 (the collapse)"
    );
}
