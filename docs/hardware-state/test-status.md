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
- 654 baselined floor cases (per-suite below), all 215 suite tests green. The
  harness emits no total case count, so re-derive one before citing it.

### Per-suite breakdown (cases/baselined)

Baselined counts are the live entry counts in `tests/gbtr/baselines/*.txt` plus
each suite's inline `BASELINE` const — one entry is one rom×model case, so they
sum to the 654 above. The Cases column predates the current tree and is *not*
verified; re-count it before citing.

| Suite | Cases (unverified) | Baselined |
|---|---|---|
| acid | 4 | 1 |
| age | 49 | 33 |
| blargg | 82 | 1 |
| gambatte | 5330 | 548 |
| gbmicrotest | 483 | 7 |
| mealybug | 55 | 23 |
| mooneye2022 | 439 | 1 |
| same-suite | 72 | 3 |
| smallsuites | 30 | 0 |
| wilbertpol | 561 | 37 |

- Floor classes A–C and E–H with lift conditions are indexed in
  `tests/gbtr/baselines/gambatte.txt`; class D (dot-serial OAM scan) was lifted.

### Runtime

- Full gbtr run ≈230 s debug / ≈350 s release. Heavily machine-dependent — a slow
  box runs several times longer; treat these as a fast-workstation figure, not a
  budget.
- Dominated by gambatte_matrix's 5272 frame-rendered cases (dev/test profiles already build core at opt-level 2).

## Unit tests & ROM availability

- All subsystems implemented; 890 core unit tests (`cargo test -p slopgb-core --lib`).
- Missing test ROMs skip silently unless `SLOPGB_REQUIRE_ROMS=1` (set in CI) — run `test-roms/download.sh` first.
