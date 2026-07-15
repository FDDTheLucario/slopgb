use super::*;
use std::io::Read;

fn read_u32(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(buf[off..off + 4].try_into().unwrap())
}

fn temp_path(name: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("slopgb-avitest-{name}.avi"));
    p
}

#[test]
fn writes_riff_avi_container_with_patched_counts() {
    let path = temp_path("basic");
    let (w, h) = (4u32, 2u32);
    {
        let mut avi = AviWriter::create(&path, w, h, 60.0).unwrap();
        // Two solid frames: red then green (XRGB8888).
        avi.write_frame(&vec![0x00FF_0000; (w * h) as usize])
            .unwrap();
        avi.write_frame(&vec![0x0000_FF00; (w * h) as usize])
            .unwrap();
        avi.finish().unwrap();
    }
    let mut buf = Vec::new();
    File::open(&path).unwrap().read_to_end(&mut buf).unwrap();
    std::fs::remove_file(&path).ok();

    assert_eq!(&buf[0..4], b"RIFF");
    assert_eq!(&buf[8..12], b"AVI ");
    // RIFF size = file length - 8.
    assert_eq!(read_u32(&buf, 4) as usize, buf.len() - 8);
    // Patched frame counts.
    assert_eq!(read_u32(&buf, AVIH_FRAMES_POS as usize), 2);
    assert_eq!(read_u32(&buf, STRH_LENGTH_POS as usize), 2);
    // movi + idx1 present.
    assert_eq!(
        &buf[(MOVI_DATA_POS as usize - 4)..MOVI_DATA_POS as usize],
        b"movi"
    );
    assert!(
        buf.windows(4).any(|c| c == b"idx1"),
        "idx1 index chunk missing"
    );
    // First frame chunk id right after the movi fourcc.
    assert_eq!(
        &buf[MOVI_DATA_POS as usize..MOVI_DATA_POS as usize + 4],
        b"00db"
    );
}

#[test]
fn finish_is_idempotent() {
    let path = temp_path("idem");
    let mut avi = AviWriter::create(&path, 2, 2, 59.7).unwrap();
    avi.write_frame(&[0x00AA_BBCC; 4]).unwrap();
    avi.finish().unwrap();
    let len1 = std::fs::metadata(&path).unwrap().len();
    avi.finish().unwrap(); // second finish must not append a second index
    let len2 = std::fs::metadata(&path).unwrap().len();
    drop(avi);
    std::fs::remove_file(&path).ok();
    assert_eq!(len1, len2);
}

#[test]
fn frame_bytes_are_bottom_up_bgr() {
    let path = temp_path("bgr");
    let (w, h) = (2u32, 2u32);
    let mut avi = AviWriter::create(&path, w, h, 60.0).unwrap();
    // Distinct rows: top row red, bottom row blue.
    avi.write_frame(&[0x00FF_0000, 0x00FF_0000, 0x0000_00FF, 0x0000_00FF])
        .unwrap();
    avi.finish().unwrap();
    let mut buf = Vec::new();
    File::open(&path).unwrap().read_to_end(&mut buf).unwrap();
    std::fs::remove_file(&path).ok();
    // Frame data starts 8 bytes past the movi data pos (skip "00db"+len).
    let data = MOVI_DATA_POS as usize + 8;
    // Bottom-up: first stored row is the source's bottom row (blue) as BGR.
    let stride = ((w as usize * 3) + 3) & !3;
    assert_eq!(&buf[data..data + 3], &[0xFF, 0x00, 0x00]); // blue → B=FF
    // Next stored row is the top row (red) as BGR.
    assert_eq!(&buf[data + stride..data + stride + 3], &[0x00, 0x00, 0xFF]); // red → R=FF
}
