# Test status & harness

## Mooneye

- All mooneye tests green — 439/439 rom×model combos (mts-20240926 bundle), CI-verified on linux/windows/macos.
- Pass-detection protocol per category:

| Category | Pass detection |
|---|---|
| acceptance / emulator-only / misc | Breakpoint protocol |
| sprite_priority | Frame compare |
| madness/mgb_oam_dma_halt_sprites | Frame compare (this ROM halts forever, never executes `LD B,B`) |

- Reference frames for the frame-compare cases are vendored under `crates/slopgb-core/tests/expected/`.

## game-boy-test-roms v7.0 battery

- Battery green (`tests/gbtr`: 10 suite modules).
- Each suite is ratcheted against an exact known-failure baseline:
  - unlisted failure = regression
  - passing/orphaned entry = stale
  - both fail the run.
- A whole-collection inventory guard pins every on-disk ROM claimed-or-exempt exactly once.
- 7041 rom×model cases = 6387 pass + 654 baselined floor.

### Per-suite breakdown (cases/baselined)

Measured from a full run, not asserted — regenerate with the census command
below rather than editing these by hand.

| Suite | Cases | Baselined |
|---|---|---|
| acid | 4 | 1 |
| age | 49 | 33 |
| blargg | 82 | 1 |
| gambatte | 5272 | 548 |
| gbmicrotest | 483 | 7 |
| mealybug | 55 | 23 |
| mooneye2022 | 439 | 1 |
| same-suite | 72 | 3 |
| smallsuites | 24 | 0 |
| wilbertpol | 561 | 37 |

### The floor census

`floor-census.tsv` (this directory) is the live, per-row tally: for each of the
654 baselined rows it records our value, the ROM's wanted value, SameBoy's
value, and the provenance of that want (which silicon the expectation was
captured on, or that it is a known-defective asset). Regenerate:

```sh
SLOPGB_GBTR_CENSUS=/tmp/dump.tsv cargo test -p slopgb-core --test gbtr
python3 docs/sameboy-port/tools/census.py /tmp/dump.tsv
```

The prose floor-class index (A–H) in `tests/gbtr/baselines/gambatte.txt` still
carries the *lift conditions* and the dated do-not-retry results, but its row
counts are a pre-eager-flip census — trust the TSV for counts.

### Runtime

- Full gbtr run ≈330 s debug.
- Dominated by gambatte_matrix's 5272 frame-rendered cases (dev/test profiles already build core at opt-level 2).

## Unit tests & ROM availability

- All subsystems implemented; 597 unit tests.
- Missing test ROMs skip silently unless `SLOPGB_REQUIRE_ROMS=1` (set in CI) — run `test-roms/download.sh` first.
