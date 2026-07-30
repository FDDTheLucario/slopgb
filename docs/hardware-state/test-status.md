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
- 7041 rom×model cases = 6435 pass + 606 baselined floor.

### Per-suite breakdown (cases/baselined)

Measured from a full run, not asserted — regenerate with the census command
below rather than editing these by hand.

| Suite | Cases | Baselined |
|---|---|---|
| acid | 4 | 1 |
| age | 49 | 29 |
| blargg | 82 | 1 |
| gambatte | 5272 | 504 |
| gbmicrotest | 483 | 7 |
| mealybug | 55 | 23 |
| mooneye2022 | 439 | 1 |
| same-suite | 72 | 3 |
| smallsuites | 24 | 0 |
| wilbertpol | 561 | 37 |

### The floor census

`floor-census.tsv` (this directory) is the live, per-row tally: for each of the
606 baselined rows it records our value, the ROM's wanted value, SameBoy's
value, and the provenance of that want (which silicon the expectation was
captured on, or that it is a known-defective asset). Regenerate:

```sh
SLOPGB_GBTR_CENSUS=/tmp/dump.tsv cargo test -p slopgb-core --test gbtr
python3 docs/sameboy-port/tools/census.py /tmp/dump.tsv
```

The prose floor-class index (A–H) in `tests/gbtr/baselines/gambatte.txt` still
carries the *lift conditions* and the dated do-not-retry results, but its row
counts are a pre-eager-flip census — trust the TSV for counts.

#### What the first census found (2026-07-28)

| Bucket | Rows | Meaning |
|---|---|---|
| BUG | 409 | hardware-captured want we miss (333 of them SameBoy-PASS, 76 SameBoy-unmeasured) |
| EXCEED | 203 | hardware-captured want that SameBoy misses too — fixing these puts us ahead of SameBoy |
| JUNK | 39 | 3 known-defective assets + 36 wilbertpol 2016 rows |
| CONFLICT | 3 | same-suite NR43 rows upstream documents as revision-specific |

Two results worth carrying forward:

- **The floor is not padded with junk.** 612 of 654 rows carry hardware-backed
  provenance. Nothing is currently safe to exempt: the 3 defective assets are
  deliberately kept baselined under class-F policy, and the 36 wilbertpol rows
  are `sameboy=unknown` (no classifier reads their `0xED` protocol), so
  dropping them on provenance alone would be exempting an unmeasured row.
- **Half the floor is confirmed bugs, not structural limits.** 333 rows are
  SameBoy-PASS: SameBoy reproduces the captured value on the same ROM where we
  do not. The largest such cluster is `gambatte/scx_during_m3` (116 rows),
  whose failures are coherent sub-pixel geometry differences against the
  `_cgb04c`/`_dmg08` references, not noise. Treat any "class A/H needs a
  sub-dot rewrite" note as unproven until re-probed with a discriminated
  lever — the `rom-diff-weld` skill exists because that verdict has been
  wrong before.

### Runtime

- Full gbtr run ≈330 s debug.
- Dominated by gambatte_matrix's 5272 frame-rendered cases (dev/test profiles already build core at opt-level 2).

## Unit tests & ROM availability

- All subsystems implemented; 597 unit tests.
- Missing test ROMs skip silently unless `SLOPGB_REQUIRE_ROMS=1` (set in CI) — run `test-roms/download.sh` first.
