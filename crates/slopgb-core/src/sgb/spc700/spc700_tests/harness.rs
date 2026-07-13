//! Two conformance harnesses:
//!
//! 1. [`opcode_cycle_table_smoke`] — always runs. Executes all 256 opcodes and
//!    asserts each consumes exactly its documented base cycle count (or base+2
//!    for the conditional ops), proving the whole table is wired and the
//!    dispatch never panics.
//!
//! 2. [`singlestep_conformance`] — `#[ignore]`d. Runs the `SingleStepTests/spc700`
//!    suite (MIT, github.com/SingleStepTests/spc700): 1000 hardware-traced
//!    randomised cases per opcode, full pre/post register + RAM state + cycle
//!    count. Point `SPC700_TESTS_DIR` at the unpacked `v1/` JSON directory:
//!
//!    ```text
//!    SPC700_TESTS_DIR=/path/to/spc700/v1 \
//!      cargo test -p slopgb-core --lib sgb::spc700 -- --ignored singlestep
//!    ```

use super::*;

/// Opcodes whose cycle count is conditional (branch-taken adds +2): the eight
/// `Bcc`, `BBS`/`BBC` (low nibble 3), `CBNE` (`2E`/`DE`), `DBNZ` (`6E`/`FE`).
fn is_conditional(op: u8) -> bool {
    matches!(op & 0x1F, 0x10) // Bcc: 10,30,50,70,90,B0,D0,F0
        || op & 0x0F == 0x03 // BBS/BBC
        || matches!(op, 0x2E | 0xDE | 0x6E | 0xFE)
}

#[test]
fn opcode_cycle_table_smoke() {
    for op in 0u16..=255 {
        let op = op as u8;
        let (_, cyc) = run1(&[op, 0x00, 0x00], |s| {
            s.a = 0x10;
            s.x = 0x10;
            s.y = 0x10;
            s.sp = 0x80;
        });
        let base = super::CYCLES[op as usize] as u32;
        if is_conditional(op) {
            assert!(
                cyc == base || cyc == base + 2,
                "op {op:#04X}: cycles {cyc} not in {{{base}, {}}}",
                base + 2
            );
        } else {
            assert_eq!(cyc, base, "op {op:#04X}: cycles {cyc} != table {base}");
        }
    }
}

// ---- minimal JSON parser (std-only; tailored to the SingleStepTests shape) --

#[derive(Debug)]
enum Json {
    Null,
    Num(i64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> &Json {
        match self {
            Json::Obj(v) => v
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v)
                .unwrap_or_else(|| panic!("missing key {key}")),
            _ => panic!("not an object"),
        }
    }
    fn num(&self) -> i64 {
        match self {
            Json::Num(n) => *n,
            _ => panic!("not a number: {self:?}"),
        }
    }
    fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(v) => v,
            _ => panic!("not an array"),
        }
    }
    fn text(&self) -> &str {
        match self {
            Json::Str(s) => s,
            _ => panic!("not a string"),
        }
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    /// Skip whitespace and the structural `,`/`:` (the shape is regular, so we
    /// parse values positionally and treat separators as skippable).
    fn ws(&mut self) {
        while self.i < self.b.len() {
            match self.b[self.i] {
                b' ' | b'\n' | b'\r' | b'\t' | b',' | b':' => self.i += 1,
                _ => break,
            }
        }
    }

    fn value(&mut self) -> Json {
        self.ws();
        match self.b[self.i] {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => Json::Str(self.string()),
            b'n' => {
                self.i += 4; // null
                Json::Null
            }
            b't' => {
                self.i += 4; // true
                Json::Num(1)
            }
            b'f' => {
                self.i += 5; // false
                Json::Num(0)
            }
            _ => self.number(),
        }
    }

    fn object(&mut self) -> Json {
        self.i += 1; // '{'
        let mut v = Vec::new();
        loop {
            self.ws();
            if self.b[self.i] == b'}' {
                self.i += 1;
                break;
            }
            let key = self.string();
            self.ws();
            let val = self.value();
            v.push((key, val));
        }
        Json::Obj(v)
    }

    fn array(&mut self) -> Json {
        self.i += 1; // '['
        let mut v = Vec::new();
        loop {
            self.ws();
            if self.b[self.i] == b']' {
                self.i += 1;
                break;
            }
            v.push(self.value());
        }
        Json::Arr(v)
    }

    fn string(&mut self) -> String {
        // The dataset has no escaped characters inside strings.
        self.i += 1; // opening quote
        let start = self.i;
        while self.b[self.i] != b'"' {
            self.i += 1;
        }
        let s = std::str::from_utf8(&self.b[start..self.i])
            .unwrap()
            .to_string();
        self.i += 1; // closing quote
        s
    }

    fn number(&mut self) -> Json {
        let start = self.i;
        if self.b[self.i] == b'-' {
            self.i += 1;
        }
        while self.i < self.b.len() && self.b[self.i].is_ascii_digit() {
            self.i += 1;
        }
        let n = std::str::from_utf8(&self.b[start..self.i])
            .unwrap()
            .parse::<i64>()
            .unwrap();
        Json::Num(n)
    }
}

fn parse(data: &[u8]) -> Json {
    Parser { b: data, i: 0 }.value()
}

/// Run one SingleStepTests case; `Ok` on an exact match, `Err(details)` else.
fn run_case(t: &Json) -> Result<(), String> {
    let init = t.get("initial");
    let fin = t.get("final");
    let cycles = t.get("cycles").arr().len();

    let mut s = cpu_flat();
    s.pc = init.get("pc").num() as u16;
    s.a = init.get("a").num() as u8;
    s.x = init.get("x").num() as u8;
    s.y = init.get("y").num() as u8;
    s.sp = init.get("sp").num() as u8;
    s.psw = Psw::from_byte(init.get("psw").num() as u8);
    for e in init.get("ram").arr() {
        let p = e.arr();
        s.ram[p[0].num() as usize] = p[1].num() as u8;
    }

    let cyc = s.step() as usize;

    let mut errs = Vec::new();
    let want = |field: &str, got: u32| {
        let w = fin.get(field).num() as u32;
        if got != w {
            Some(format!("{field}: got {got:#X} want {w:#X}"))
        } else {
            None
        }
    };
    errs.extend(want("pc", s.pc as u32));
    errs.extend(want("a", s.a as u32));
    errs.extend(want("x", s.x as u32));
    errs.extend(want("y", s.y as u32));
    errs.extend(want("sp", s.sp as u32));
    errs.extend(want("psw", s.psw.to_byte() as u32));
    for e in fin.get("ram").arr() {
        let p = e.arr();
        let addr = p[0].num() as usize;
        let w = p[1].num() as u8;
        if s.ram[addr] != w {
            errs.push(format!(
                "ram[{addr:#06X}]: got {:#X} want {w:#X}",
                s.ram[addr]
            ));
        }
    }
    if cyc != cycles {
        errs.push(format!("cycles: got {cyc} want {cycles}"));
    }

    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs.join(", "))
    }
}

#[test]
#[ignore = "set SPC700_TESTS_DIR to the SingleStepTests/spc700 v1 JSON directory"]
fn singlestep_conformance() {
    let dir = std::env::var("SPC700_TESTS_DIR")
        .expect("SPC700_TESTS_DIR must point at the SingleStepTests/spc700 v1/ dir");

    let mut total = 0usize;
    let mut fails = 0usize;
    let mut per_op = [0u32; 256];
    let mut samples: Vec<String> = Vec::new();
    // A skipped opcode is NOT a pass. Setting `SPC700_TESTS_DIR` is an explicit
    // request to run the conformance suite, so a missing or truncated dataset
    // must fail loudly rather than green-light zero cases.
    let mut missing: Vec<u8> = Vec::new();

    for op in 0u16..=255 {
        let op = op as u8;
        let path = format!("{dir}/{op:02x}.json");
        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(_) => {
                eprintln!("warning: missing {path}, skipping opcode {op:#04X}");
                missing.push(op);
                continue;
            }
        };
        let json = parse(&data);
        for t in json.arr() {
            total += 1;
            if let Err(e) = run_case(t) {
                fails += 1;
                per_op[op as usize] += 1;
                if samples.len() < 60 {
                    samples.push(format!("op {op:#04X} [{}]: {e}", t.get("name").text()));
                }
            }
        }
    }

    for (op, n) in per_op.iter().enumerate() {
        if *n > 0 {
            eprintln!("op {op:#04X}: {n} failures");
        }
    }
    for s in &samples {
        eprintln!("  {s}");
    }
    eprintln!("SingleStepTests: {}/{} passed", total - fails, total);
    assert!(
        missing.is_empty(),
        "{} of 256 opcode files missing from {dir} (first: {:#04X}) — an incomplete \
         dataset cannot certify the core",
        missing.len(),
        missing[0]
    );
    assert!(
        total > 0,
        "SPC700_TESTS_DIR={dir} yielded 0 cases — a vacuous pass is not a pass"
    );
    assert_eq!(fails, 0, "{fails}/{total} SingleStepTests cases failed");
}
