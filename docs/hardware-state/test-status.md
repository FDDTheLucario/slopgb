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
- 7047 rom×model cases = 5941 pass + 1106 baselined floor.

### Per-suite breakdown (cases/baselined)

| Suite | Cases | Baselined |
|---|---|---|
| acid | 4 | 1 |
| age | 49 | 38 |
| blargg | 82 | 1 |
| gambatte | 5330 | 918 |
| gbmicrotest | 483 | 32 |
| mealybug | 55 | 26 |
| mooneye2022 | 439 | 1 |
| same-suite | 72 | 4 |
| smallsuites | 30 | 6 |
| wilbertpol | 561 | 79 |

- Floor classes A–H with lift conditions are indexed in `tests/gbtr/baselines/gambatte.txt`.

### Runtime

- Full gbtr run ≈230 s debug / ≈350 s release.
- Dominated by gambatte_matrix's 5272 frame-rendered cases (dev/test profiles already build core at opt-level 2).

## Unit tests & ROM availability

- All subsystems implemented; 597 unit tests.
- Missing test ROMs skip silently unless `SLOPGB_REQUIRE_ROMS=1` (set in CI) — run `test-roms/download.sh` first.
