//! Native unit tests for the target-independent MSU-1 logic: header parsing,
//! volume scaling, the status-register bit layout, control-register decoding,
//! and the seek-pointer assembly. The streaming + host-file/mailbox paths call
//! wasm host imports (unavailable natively), so they are proven end-to-end in
//! `slopgb-plugin-host/tests/msu1_roundtrip.rs` instead.

use super::*;

/// A valid `.pcm` header: magic + a little-endian loop point.
fn header(loop_point: u32) -> Vec<u8> {
    let mut h = PCM_MAGIC.to_vec();
    h.extend_from_slice(&loop_point.to_le_bytes());
    h
}

#[test]
fn pcm_header_parses_magic_and_loop_point() {
    assert_eq!(parse_pcm_header(&header(0)), Some(0));
    assert_eq!(parse_pcm_header(&header(44_100)), Some(44_100));
    // Wrong magic is rejected (not an MSU-1 track).
    let mut bad = header(0);
    bad[0] = b'X';
    assert_eq!(parse_pcm_header(&bad), None);
    // A truncated header is rejected rather than read out of bounds.
    assert_eq!(parse_pcm_header(&header(0)[..7]), None);
    assert_eq!(parse_pcm_header(&[]), None);
}

#[test]
fn volume_scales_linearly() {
    assert_eq!(scale_sample(0x4000, 0x00), 0, "volume 0 mutes");
    assert_eq!(scale_sample(0x4000, 0xFF), 0x3FC0, "0xFF ≈ unity (255/256)");
    assert_eq!(scale_sample(0x4000, 0x80), 0x2000, "half volume halves");
    assert_eq!(scale_sample(-0x4000, 0xFF), -0x3FC0, "sign preserved");
}

#[test]
fn status_byte_reports_revision_and_flags() {
    let mut m = Msu1::new();
    // Idle: just the revision in the low bits, no flags.
    assert_eq!(m.status(), REVISION);
    m.playing = true;
    m.repeat = true;
    assert_eq!(m.status(), REVISION | ST_AUDIO_PLAYING | ST_AUDIO_REPEAT);
    m.playing = false;
    m.repeat = false;
    m.track_missing = true;
    assert_eq!(m.status(), REVISION | ST_TRACK_MISSING);
}

#[test]
fn control_register_decodes_play_stop_repeat() {
    let mut m = Msu1::new();
    m.audio_pos = 1000; // pretend we were mid-track

    // Play (bit 0) starts from the beginning.
    m.write_control(CTL_PLAY | CTL_REPEAT);
    assert!(m.playing);
    assert!(m.repeat);
    assert_eq!(m.audio_pos, 0, "play restarts at the track start");

    // Stop (no bits) halts without resetting the position.
    m.audio_pos = 500;
    m.write_control(0);
    assert!(!m.playing);
    assert_eq!(m.audio_pos, 500);

    // Resume (bit 2) keeps the current position.
    m.write_control(CTL_RESUME);
    assert!(m.playing);
    assert_eq!(m.audio_pos, 500, "resume keeps the position");

    // A missing track can never be played.
    m.track_missing = true;
    m.write_control(CTL_PLAY);
    assert!(!m.playing, "a missing track stays stopped");
}

#[test]
fn seek_pointer_assembles_little_endian_on_the_fourth_write() {
    let mut m = Msu1::new();
    // Writes to ports 0..=2 buffer the low bytes without committing.
    m.port_write(0, 0x78);
    m.port_write(1, 0x56);
    m.port_write(2, 0x34);
    assert_eq!(m.data_pos, 0, "seek not committed until the port-3 write");
    m.port_write(3, 0x12);
    assert_eq!(m.data_pos, 0x1234_5678, "port 3 commits the 32-bit LE seek");
}

#[test]
fn id_ports_spell_out_the_chip() {
    let mut m = Msu1::new();
    let id: Vec<u8> = (2..=7).map(|p| m.port_read(p)).collect();
    assert_eq!(&id, b"S-MSU1", "ports 2..=7 read back the MSU-1 id string");
}
