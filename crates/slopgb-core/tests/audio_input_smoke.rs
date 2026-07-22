//! End-to-end smokes for the two subsystems a headless test run otherwise leaves
//! blind: audio output and joypad input. Not accuracy tests (mooneye/gambatte
//! cover the APU + joypad timing) — these prove the *player-facing* paths carry
//! signal: a triggered channel makes finite, bounded, non-silent samples, and a
//! button press reaches the machine.

use slopgb_core::{Button, GameBoy, Model};

fn dmg() -> GameBoy {
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = 0x00;
    GameBoy::new(Model::Dmg, rom).expect("cart builds")
}

#[test]
fn triggered_square_channel_produces_finite_bounded_audible_samples() {
    let mut gb = dmg();
    // Power the APU, route + max both master channels, then trigger channel 1
    // (square) with a full-volume, non-decaying envelope. (NR52 must come first —
    // channel writes are ignored while the APU is powered down.)
    gb.debug_write(0xFF26, 0x80); // NR52: APU on
    gb.debug_write(0xFF25, 0xFF); // NR51: every channel to L+R
    gb.debug_write(0xFF24, 0x77); // NR50: max master volume both sides
    gb.debug_write(0xFF11, 0x80); // NR11: 50% duty
    gb.debug_write(0xFF12, 0xF0); // NR12: initial volume 15, no envelope decay
    gb.debug_write(0xFF13, 0x00); // NR13: frequency low byte
    gb.debug_write(0xFF14, 0x87); // NR14: trigger + frequency high bits

    let mut out = Vec::new();
    for _ in 0..8 {
        gb.run_frame();
        gb.drain_audio_raw(&mut out);
    }

    assert!(!out.is_empty(), "the APU produced no samples at all");
    let mut peak = 0.0f32;
    for &(l, r) in &out {
        assert!(
            l.is_finite() && r.is_finite(),
            "audio sample is NaN/inf: {l},{r}"
        );
        assert!(
            l.abs() <= 1.0 && r.abs() <= 1.0,
            "audio sample out of [-1,1]: {l},{r}"
        );
        peak = peak.max(l.abs()).max(r.abs());
    }
    assert!(
        peak > 0.01,
        "triggered square channel is silent (peak {peak})"
    );
}

#[test]
fn silent_apu_drains_finite_bounded_samples() {
    // Even with no channel triggered, draining must yield only finite, in-range
    // samples (a NaN/overflow here would click/pop a real player).
    let mut gb = dmg();
    gb.debug_write(0xFF26, 0x80);
    let mut out = Vec::new();
    for _ in 0..4 {
        gb.run_frame();
        gb.drain_audio_raw(&mut out);
    }
    for &(l, r) in &out {
        assert!(
            l.is_finite() && r.is_finite(),
            "silent-path NaN/inf: {l},{r}"
        );
        assert!(
            l.abs() <= 1.0 && r.abs() <= 1.0,
            "silent-path out of range: {l},{r}"
        );
    }
}

#[test]
fn button_press_and_release_reach_the_machine() {
    let mut gb = dmg();
    const ALL: [Button; 8] = [
        Button::Right,
        Button::Left,
        Button::Up,
        Button::Down,
        Button::A,
        Button::B,
        Button::Select,
        Button::Start,
    ];
    for b in ALL {
        assert!(!gb.debug_button(b), "{b:?} should start released");
        gb.press(b);
        assert!(gb.debug_button(b), "{b:?} press didn't register");
        gb.release(b);
        assert!(!gb.debug_button(b), "{b:?} release didn't register");
    }
}

#[test]
fn joypad_register_reflects_a_pressed_dpad_line() {
    // The register the ROM actually reads (FF00), not just the debug view: with
    // the direction column selected (P14 low) and Down held, Down's line reads 0.
    let mut gb = dmg();
    gb.press(Button::Down);
    // Select directions: write P15=1 (bit5, buttons off), P14=0 (bit4, dirs on).
    gb.debug_write(0xFF00, 0b0010_0000);
    let ff00 = gb.debug_read(0xFF00);
    // Down is bit 3 of the low nibble (active-low): held → 0.
    assert_eq!(
        ff00 & 0x08,
        0,
        "Down held with dirs selected must clear its line (FF00={ff00:#04X})"
    );
    gb.release(Button::Down);
    let released = gb.debug_read(0xFF00);
    assert_eq!(
        released & 0x08,
        0x08,
        "Down released must set its line high (FF00={released:#04X})"
    );
}
