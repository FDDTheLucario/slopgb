//! The single-step harness: apply a vector's `initial` state, run one `step`,
//! then diff registers, RAM and the per-cycle bus activity against `final` +
//! `cycles`. The first mismatch panics with the offending field.

use std::path::PathBuf;

use slopgb_w65c816::{Cpu, Regs};

use super::bus::{Access, VecBus};
use super::json::{self, J};

/// The directory holding `v1/<op>.<mode>.json`, or `None` if the vectors have
/// not been downloaded (test-roms/download-65816-tests.sh).
fn vectors_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-roms/65816-tests/v1");
    dir.is_dir().then_some(dir)
}

/// Run every vector for `opcode` (two hex digits) in both emulation and native
/// mode. Absent vectors skip (unless `SLOPGB_REQUIRE_ROMS` is set). Panics on the
/// first mismatch.
pub fn run_opcode(opcode: &str) {
    run_opcodes(&[opcode]);
}

/// Run [`run_opcode`] for each opcode in `opcodes`.
pub fn run_opcodes(opcodes: &[&str]) {
    let Some(dir) = vectors_dir() else {
        assert!(
            std::env::var_os("SLOPGB_REQUIRE_ROMS").is_none(),
            "65816 vectors missing; run test-roms/download-65816-tests.sh"
        );
        eprintln!("skipping: 65816 vectors not downloaded");
        return;
    };
    for op in opcodes {
        for mode in ['e', 'n'] {
            let path = dir.join(format!("{op}.{mode}.json"));
            if !path.is_file() {
                assert!(
                    std::env::var_os("SLOPGB_REQUIRE_ROMS").is_none(),
                    "missing vector file {}",
                    path.display()
                );
                eprintln!("skipping absent {}", path.display());
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read vector file");
            let doc = json::parse(&text);
            for (i, test) in doc.arr().iter().enumerate() {
                run_one(test).unwrap_or_else(|msg| {
                    panic!(
                        "{op}.{mode} test #{i} ({}): {msg}",
                        test.get("name").unwrap().str()
                    )
                });
            }
        }
    }
}

fn ram_pairs(v: &J) -> Vec<(u32, u8)> {
    v.get("ram")
        .unwrap()
        .arr()
        .iter()
        .map(|e| {
            let p = e.arr();
            (p[0].int() as u32, p[1].int() as u8)
        })
        .collect()
}

fn regs_of(v: &J) -> Regs {
    let g = |k: &str| v.get(k).unwrap().int();
    Regs {
        a: g("a") as u16,
        x: g("x") as u16,
        y: g("y") as u16,
        s: g("s") as u16,
        d: g("d") as u16,
        pc: g("pc") as u16,
        pbr: g("pbr") as u8,
        dbr: g("dbr") as u8,
        p: g("p") as u8,
        e: g("e") != 0,
    }
}

/// The subset of the `cycles` array that is real bus activity (non-null value).
fn expected_accesses(cycles: &[J]) -> Vec<Access> {
    cycles
        .iter()
        .filter_map(|c| {
            let f = c.arr();
            let val = f[1].opt_int()?;
            let write = f[2].str().as_bytes()[3] == b'w';
            Some(Access {
                addr: f[0].int() as u32,
                val: val as u8,
                write,
            })
        })
        .collect()
}

fn run_one(test: &J) -> Result<(), String> {
    let initial = test.get("initial").unwrap();
    let expect = test.get("final").unwrap();
    let cycles = test.get("cycles").unwrap().arr();

    let mut cpu = Cpu::from_regs(regs_of(initial));
    let mut bus = VecBus::default();
    bus.seed(&ram_pairs(initial));

    let ran = cpu.step(&mut bus);

    if ran as usize != cycles.len() {
        return Err(format!("cycles: got {ran}, want {}", cycles.len()));
    }

    let want_acc = expected_accesses(cycles);
    if bus.log != want_acc {
        return Err(format!(
            "bus access mismatch:\n got  {:x?}\n want {want_acc:x?}",
            bus.log
        ));
    }

    let want_regs = regs_of(expect);
    if cpu.regs != want_regs {
        return Err(format!(
            "regs:\n got  {:x?}\n want {want_regs:x?}",
            cpu.regs
        ));
    }

    for (addr, val) in ram_pairs(expect) {
        let got = bus.mem.get(&addr).copied().unwrap_or(0);
        if got != val {
            return Err(format!("ram[{addr:#08x}]: got {got:#04x}, want {val:#04x}"));
        }
    }
    Ok(())
}
