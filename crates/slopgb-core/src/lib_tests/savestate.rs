//! Save-state round-trips, corrupt/foreign rejection, clone independence.

use super::*;

#[test]
fn save_state_round_trips_the_whole_machine() {
    // Several save points (mid-frame / many-frames-in) on the simple DMG/ch1
    // oracle.
    assert_round_trips(
        Model::Dmg,
        &savestate_oracle_rom(),
        &[777, 30_000, 70_111],
        300,
        "dmg",
    );
}

#[test]
fn save_state_round_trips_cgb_mbc_and_all_channels() {
    // The serializer fields the simple oracle leaves at default — MBC1 banking +
    // cart RAM, ch2/ch3(wave)/ch4(noise), and (CGB) SVBK/VBK/palette RAM — are
    // driven non-default here, so a drift in those write_state/read_state pairs
    // (which the DMG/ch1 oracle would round-trip-pass silently) diverges.
    let rom = comprehensive_oracle_rom(false);
    assert_round_trips(Model::Dmg, &rom, &[2000, 40_000], 150, "dmg-comprehensive");
    let cgb_rom = comprehensive_oracle_rom(true);
    assert_round_trips(
        Model::Cgb,
        &cgb_rom,
        &[2000, 40_000],
        150,
        "cgb-comprehensive",
    );
}

#[test]
fn save_state_round_trips_sgb_with_audio_subsystem() {
    // On SGB the save state also carries the SPC700 + S-DSP (the v4 tail).
    // Exercise the full serialization chain and confirm the Game Boy side stays
    // byte-identical across save/load — the SGB APU content round-trip itself is
    // unit-tested in `sgb::apu`. The oracle issues no SGB sound commands, so the
    // SNES side is the deterministic IPL.
    assert_round_trips(
        Model::Sgb,
        &savestate_oracle_rom(),
        &[2000, 40_000],
        100,
        "sgb",
    );
}

#[test]
fn load_state_rejects_corrupt_or_foreign_states() {
    let rom = savestate_oracle_rom();
    let mut gb = GameBoy::new(Model::Dmg, rom.clone()).unwrap();
    gb.run_frame();
    let good = gb.save_state();

    // Round-trips into the same machine.
    assert!(gb.load_state(&good).is_ok());

    // Bad magic / truncated / version.
    assert_eq!(gb.load_state(&[0; 2]), Err(StateError::Truncated));
    assert_eq!(
        gb.load_state(b"XXXX\x01\x00"),
        Err(StateError::BadMagic),
        "wrong magic"
    );
    let mut bad_ver = good.clone();
    bad_ver[4] = 0xFF; // bump the version u16
    assert_eq!(gb.load_state(&bad_ver), Err(StateError::BadVersion));

    // A state for a *different* ROM (different title) is rejected.
    let mut other_rom = rom.clone();
    other_rom[0x134..0x13B].copy_from_slice(b"OTHERXX");
    let other = GameBoy::new(Model::Dmg, other_rom).unwrap().save_state();
    assert_eq!(gb.load_state(&other), Err(StateError::RomMismatch));

    // The ROM fingerprint also pins the cartridge TYPE (0x147): a same-title ROM
    // with a different mapper is rejected, so a fingerprint collision can't
    // mis-deserialize the (variant-dispatched) mapper state.
    let mut diff_mapper = rom.clone();
    diff_mapper[0x147] = 0x03; // MBC1+RAM+BATTERY vs the original ROM-ONLY (0x00)
    let other_mapper = GameBoy::new(Model::Dmg, diff_mapper).unwrap().save_state();
    assert_eq!(gb.load_state(&other_mapper), Err(StateError::RomMismatch));

    // A failed load leaves the machine intact (atomic).
    let pc_before = gb.cpu_regs().pc;
    let _ = gb.load_state(b"XXXX");
    assert_eq!(gb.cpu_regs().pc, pc_before, "failed load is a no-op");
}

#[test]
fn load_state_rejects_cross_model_sgb_vs_dmg() {
    // Same ROM, different system: an SGB state carries the SPC700 + S-DSP tail,
    // a DMG state doesn't. Loading one into the other must be a clear
    // `ModelMismatch` — never a silent tail-drop (SGB→DMG) nor an opaque
    // `Truncated` (DMG→SGB).
    let rom = savestate_oracle_rom();
    let mut sgb = GameBoy::new(Model::Sgb, rom.clone()).unwrap();
    sgb.run_frame();
    let sgb_state = sgb.save_state();
    let mut dmg = GameBoy::new(Model::Dmg, rom.clone()).unwrap();
    dmg.run_frame();
    let dmg_state = dmg.save_state();

    assert_eq!(
        dmg.load_state(&sgb_state),
        Err(StateError::ModelMismatch),
        "SGB state into DMG must not silently drop the audio tail"
    );
    assert_eq!(
        sgb.load_state(&dmg_state),
        Err(StateError::ModelMismatch),
        "DMG state into SGB must not fail as Truncated"
    );
    // Same-model still round-trips.
    assert!(sgb.load_state(&sgb_state).is_ok());
    assert!(dmg.load_state(&dmg_state).is_ok());
}

#[test]
fn clone_is_an_independent_machine_snapshot() {
    // The Quick Save/Load primitive (MN6): GameBoy: Clone must be a deep,
    // independent copy — advancing one must not touch the other.
    let mut gb = GameBoy::new(Model::Dmg, rom_with_cgb_flag(0x00)).unwrap();
    gb.run_frame();
    let snap = gb.clone();
    let (pc0, cyc0) = (snap.cpu_regs().pc, snap.cycles());
    for _ in 0..10 {
        gb.run_frame();
    }
    assert_ne!(gb.cycles(), cyc0, "original advanced");
    assert_eq!(snap.cycles(), cyc0, "clone is frozen at the snapshot");
    assert_eq!(
        snap.cpu_regs().pc,
        pc0,
        "clone PC unchanged by the original"
    );
    // Restoring rewinds the machine exactly to the snapshot.
    let restored = snap.clone();
    assert_eq!(restored.cycles(), cyc0);
    assert_eq!(restored.cpu_regs().pc, pc0);
}

/// Link task 5: link state is transient — never serialized. A save taken with
/// a peer attached restores into a machine with no peer, and adds no bytes to
/// the state blob (the on-disk format is unchanged → golden-safe).
#[test]
fn link_state_is_not_serialized() {
    let a = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    let baseline = a.save_state();
    let mut a = a;
    a.link_connect(true);
    a.link_push_recv(0xA5);
    let with_link = a.save_state();
    assert_eq!(
        with_link.len(),
        baseline.len(),
        "link adds no bytes to the save state"
    );
    let mut b = GameBoy::new(Model::Dmg, write_c000_rom()).unwrap();
    b.load_state(&with_link).unwrap();
    assert!(
        !b.link_connected(),
        "link state is not restored from a save"
    );
}
