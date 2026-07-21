use super::*;

#[test]
fn round_trips_byte_identical() {
    let regions = Regions {
        dir: vec![1, 2, 3, 4, 5],
        instr: vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF],
        brr: vec![0x93, 0x77, 0x77, 0x77, 0x77, 0x88, 0x88, 0x88, 0x88],
    };
    let path = std::env::temp_dir().join(format!("slopgb-sf2-cache-test-{}.smpl", std::process::id()));

    write_cache(&path, &regions).expect("write must succeed");
    let read_back = read_cache(&path).expect("read must succeed");

    assert_eq!(read_back.dir, regions.dir);
    assert_eq!(read_back.instr, regions.instr);
    assert_eq!(read_back.brr, regions.brr);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn serialize_deserialize_match_write_read_cache() {
    let regions = Regions {
        dir: vec![9, 8, 7, 6],
        instr: vec![0x10, 0x20, 0x30, 0x40, 0x50, 0x60],
        brr: vec![0x93, 0x11, 0x22, 0x33, 0x44, 0x88, 0x55, 0x66, 0x77],
    };

    let bytes = serialize(&regions);
    let round_tripped = deserialize(&bytes).expect("deserialize must succeed");
    assert_eq!(round_tripped.dir, regions.dir);
    assert_eq!(round_tripped.instr, regions.instr);
    assert_eq!(round_tripped.brr, regions.brr);

    let dir = Path::new(
        "/tmp/claude-1000/-home-user-apps-slopgb/e69ca3be-a6b0-48c6-b175-7a64108303a0/scratchpad",
    );
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(format!("serialize-parity-{}.smpl", std::process::id()));

    write_cache(&path, &regions).expect("write must succeed");
    let file_bytes = std::fs::read(&path).expect("read must succeed");
    assert_eq!(file_bytes, bytes);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn rejects_bad_magic() {
    let path = std::env::temp_dir().join(format!("slopgb-sf2-cache-bad-{}.smpl", std::process::id()));
    std::fs::write(&path, b"NOPE").unwrap();
    assert!(read_cache(&path).is_err());
    let _ = std::fs::remove_file(&path);
}
