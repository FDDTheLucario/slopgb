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
- 533 baselined floor cases (per-suite below), all 218 suite tests green. The
  harness emits no total case count, so re-derive one before citing it.

### Per-suite breakdown (cases/baselined)

Baselined counts are the live entry counts in `tests/gbtr/baselines/*.txt` plus
each suite's inline `BASELINE` const — one entry is one rom×model case, so they
sum to the 533 above. The Cases column predates the current tree and is *not*
verified; re-count it before citing.

| Suite | Cases (unverified) | Baselined |
|---|---|---|
| acid | 4 | 1 |
| age | 49 | 33 |
| blargg | 82 | 1 |
| gambatte | 5330 | 427 |
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

## The age ladders — decoded (2026-08-06)

An age ROM's exit registers say nothing: every failing row reports the same
`B=00 C=6B D=14 E=06 H=98 L=10`. But the suite's shared checker compares a
MEASURED buffer (`$C600+`) against an EXPECTED ROM table byte by byte, and at
its `cp b` both values are live — so `docs/sameboy-port/tools/age_decode.py`
dumps the whole ladder and names the failing rungs. The checker is library code
at `$136F` (`$1382` in the `ncm` builds); the tool finds it by pattern.

Current state of the 18 chaseable age rows (SameBoy passes all of them):

```
oam/oam-read-dmgC-cgbBC.gb                           [Dmg]  64 rungs,  4 bad  #10:wantFF/got01 #26:wantFF/got09 #43:wantFF/got01 #59:wantFF/got09
oam/oam-write-cgbBCE.gb                              [Cgb]   0 rungs,  0 bad  (passes, or the ladder exited early)
oam/oam-write-dmgC.gb                                [Dmg]  64 rungs, 10 bad  #2:wantFF/got00 #10:wantFF/got00 #18:wantFF/got00 #26:wantFF/got00 #34:wantFF/got00 #42:wantFF/got00 …
oam/oam-write-ncmBCE.gb                              [Cgb]  64 rungs, 10 bad  #2:wantFF/got06 #10:wantFF/got01 #18:wantFF/got06 #26:wantFF/got06 #34:wantFF/got06 #42:wantFF/got06 …
speed-switch/spsw-mode0-cgbBCE.gb                    [Cgb] 120 rungs,  7 bad  #25:want0E/got0D #100:want83/got80 #102:want83/got80 #108:want83/got80 #110:want83/got80 #116:want83/got80 …
stat-interrupt/stat-int-dmgC-cgbBCE.gb               [Cgb]  80 rungs,  5 bad  #43:want82/got80 #47:want82/got80 #51:want82/got80 #55:want82/got80 #59:want82/got80
stat-mode-sprites/stat-mode-sprites-dmgC-cgbBCE.gb   [Cgb]  80 rungs,  2 bad  #32:wantFF/got01 #48:wantFF/got05
stat-mode-sprites/stat-mode-sprites-dmgC-cgbBCE.gb   [Dmg]  80 rungs,  2 bad  #32:wantFF/got01 #48:wantFF/got05
stat-mode-sprites/stat-mode-sprites-ds-cgbBCE.gb     [Cgb] 128 rungs, 47 bad  #21:wantFF/got00 #22:wantFF/got00 #23:wantFF/got00 #29:wantFF/got00 #30:wantFF/got00 #31:wantFF/got00 …
stat-mode-window/stat-mode-window-cgbBCE.gb          [Cgb] 128 rungs, 10 bad  #81:wantFF/got01 #82:wantFF/got05 #89:wantFF/got01 #90:wantFF/got05 #97:wantFF/got01 #98:wantFF/got05 …
stat-mode-window/stat-mode-window-dmgC.gb            [Dmg] 128 rungs,  2 bad  #120:wantFF/got02 #121:wantFF/got06
stat-mode-window/stat-mode-window-ds-cgbBCE.gb       [Cgb] 128 rungs,  3 bad  #120:wantFF/got02 #121:wantFF/got04 #122:wantFF/got06
stat-mode-window/stat-mode-window-ncmBCE.gb          [Cgb] 128 rungs, 10 bad  #81:wantFF/got01 #82:wantFF/got05 #89:wantFF/got01 #90:wantFF/got05 #97:wantFF/got01 #98:wantFF/got05 …
stat-mode/stat-mode-dmgC-cgbBC.gb                    [Dmg] 112 rungs,  3 bad  #9:wantFF/got00 #42:wantFF/got00 #73:wantFF/got00
stat-mode/stat-mode-ds-cgbBCE.gb                     [Cgb]   0 rungs,  0 bad  (passes, or the ladder exited early)
vram/vram-read-cgbBCE.gb                             [Cgb]   0 rungs,  0 bad  (passes, or the ladder exited early)
vram/vram-read-dmgC.gb                               [Dmg]  64 rungs,  4 bad  #10:wantFF/got01 #26:wantFF/got09 #43:wantFF/got01 #59:wantFF/got09
vram/vram-read-ncmBCE.gb                             [Cgb]  64 rungs,  4 bad  #10:wantFF/got01 #26:wantFF/got09 #43:wantFF/got01 #59:wantFF/got09
```

Reading it:

* **one defect covers three rows** — `vram-read-dmgC` [Dmg],
  `vram-read-ncmBCE` [Cgb] and `oam-read-dmgC-cgbBC` [Dmg] share the exact
  fingerprint (rungs 10/26/43/59 want `$FF`, get `01`/`09`), so our VRAM/OAM
  read-block window is a dot narrow in one specific configuration, not a
  per-suite mess;
* the expected pattern is `FF` while blocked and the real byte when readable,
  so every `wantFF/got<data>` is us UNBLOCKING TOO EARLY (or sampling a dot
  late) — the failures are one-sided, which a whole-window shift would not be;
* `oam-write-dmgC`/`-ncmBCE` fail every 8th rung (#2, #10, #18 …) — periodic,
  so it is one edge repeated per block rather than a scatter;
* `stat-int` wants `$82` and gets `$80` on five rungs: the mode-2 flag missing
  from a STAT read, a different mechanism from the accessibility rows;
* three rows (`oam-write-cgbBCE`, `stat-mode-ds-cgbBCE`, `vram-read-cgbBCE`)
  reach the checker ZERO times — their ladder bails before it, so they need
  the earlier phase traced, not a rung fixed.

Do NOT move an accessibility edge for one ladder: mooneye `lcdon_timing-GS` and
the gambatte `vram_m3` / `oam_access` rows pin the same edges from the other
side. The value here is that a fix can now be aimed at a named rung and scored
against this table.
