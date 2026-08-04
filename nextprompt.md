slopgb — next cluster: `ch1_duty0_pos6_to_pos7_timing`, 18 rows in one family

## Repo state (verified 2026-08-02)

`main` @ `94218ffa`, clean tree, pushed. No open branch.

gbtr **221/221**; gambatte **4951/5272** cases; gambatte baseline **321 keys**
(814 lines — read the floor-class index header before touching any baselined
row). mooneye **439/439** (93 suite tests), core lib **902**, frontend **676**,
clippy clean.

Census: **421 rows** — 196 BUG, 183 EXCEED, 39 JUNK, 3 CONFLICT
(`docs/hardware-state/floor-census.tsv`, 10 tab-separated columns:
`key suite cluster model ours want sameboy provenance bucket evidence`).

Last session landed four derived laws, all `+N/−0`, 212 → 196 BUG:

| commit | law | rows |
|---|---|---|
| `8fbaf58c` | HBlank window trails the line wrap 2 dots — halt **entry** | +3 |
| `36f55344` | the same window on the halt **wake** (halt wake only) | +2 |
| `765273f6` | post-boot NR13 = `$C1` (2nd chime) + `beep_duty_advance` | +8 |
| `e14a9e89` | post-boot `apu_div_step` (frame sequencer mid-round) | +3 |

Plus three documented obstructions with their numbers: `d9636e26` (dma
halt→wake gap), `94218ffa` (`ch2_init_reset_env_counter` zombie-write race),
and the `m3halt_m2unhalt_ly` ladder inside `d9636e26`.

## READ FIRST

- `docs/hardware-state/apu.md` — the whole "Post-boot warmup" section is new.
  Three subsections: the `$7C1` beep + duty phase, the frame-sequencer step,
  and the `_reset_` zombie-write table. The last one is a **do-not-retry**:
  the tempting uniform "step one event later" lever is refuted there.
- `docs/hardware-state/dma.md` §"The halt window trails the line wrap by two
  dots" (the landed law, both halves) and §"The remaining halt-family rows sit
  behind a halt→wake duration gap" (measured, do not re-sweep the DMA seams).
- The floor-class index header in `crates/slopgb-core/tests/gbtr/baselines/gambatte.txt`.

## TARGET — `ch1_duty0_pos6_to_pos7_timing`, 18 BUG rows

**One family, two clusters, one failure direction.** 44 ROMs in the family, 26
already green; all 18 failures are `[Cgb]`, all `constant-output→sound` (we emit
a constant stream, the reference varies), and all are even-numbered rungs
(`_2`/`_4`/`_6`), whose odd siblings want silence and pass.

| cluster | rows | shape |
|---|---|---|
| `gambatte/speedchange` | 17 | `speedchange{,2,3,4,5}[_nop]_ch1_duty0_pos6_to_pos7_timing[_nop][_ds]_2` |
| `gambatte/sound` | 1 | `ch1_duty0_pos6_to_pos7_timing_ds_6` |

That is 17 of the 20 `gambatte/speedchange` BUG rows — clearing this family
nearly clears the cluster. The remaining three are
`speedchange{,2}_ly44_m3_stat_{2,4}` (`C0`→`C2`) and
`speedchange2_nop_m2int_m3stat_scx1_1` (`0`→`3`), which are PPU rows, not APU.

### Why this is the right next target

The name says the observable: the duty position stepping **6 → 7**. Last
session proved the post-boot duty phase and frequency are now correct
(`ch1_init_pos` 8/8 green, `freq = $7C1`, 63 M-cycles per step), so this family
is measuring the step *event* itself across a speed switch rather than the
initial phase. The `_ds` and non-`_ds` legs both fail, and so do the `_nop`
variants, so it is unlikely to be a double-speed-only cell.

Start by checking whether the plain `gambatte/sound` leg
(`ch1_duty0_pos6_to_pos7_timing_ds_6`) fails for the same reason as the
speedchange legs — if it does, the law is in the duty step and the speed switch
is only carrying it, which makes the sound leg the cheap oracle to derive
against.

## METHOD

`/rom-disasm-gaps`, not a sweep. It was decisive twice last session:

1. **Sanity-check the premise first (step 6).** The `ch1_init_pos` family was
   not a timing law at all — the ROM never writes NR13, so the frequency was a
   post-boot leftover and ours was the boot ROM's *first* chime note instead of
   its second. One wrong constant, eight rows.
2. **`cmp -l` the ladder siblings, then map the knobs.** Dump the actual operand
   bytes per rung (a `ld b,imm`/`ld c,imm` delay pair, the observation offset)
   rather than assuming a linear sweep — `ch2_init_reset_env_counter` turned out
   to be eight pairs over *two* knobs, not sixteen rungs over one.
3. **Probe the observable, then work out what produces it.** For audio rows the
   harness verdict is a boolean over the final frame's raw samples (`check_audio`
   in `tests/gbtr/gambatte.rs`: does any sample differ from the first). Probe the
   channel state that decides it — duty position, `current_sample`, envelope
   volume — not the register you assume is being timed.
4. **Decouple before you fit.** The `+11/−3` measurement was one knob moving two
   physical quantities (the duty phase *and* the frame-sequencer phase); splitting
   them gave `+8/−0` and then `+3/−0`. When a score is `+N/−M` and the −M rows
   are a coherent family, suspect a second quantity, not a wrong constant.

Ground-truth oracle: `~/.cache/sbbuild/SameBoy-1.0.2/build/bin/tester/sameboy_tester`
with `SB_TRACE=1` (stderr), `--cgb --length 4 <rom>`. **The trace rig gained four
tags last session** (`SBHALT` with `allow_hdma_on_wake`, `SBREAD ff55`,
`SBREAD ff44`, and a `GB_display_sync` before each print so `ly`/`cfl` are exact).
`fp = absolute_debugger_ticks − display_cycles` is 2 units per dot and 912 per
line; use it for ordering. `cfl` is only trustworthy at a synced print.

Note SameBoy is **not** authoritative for audio rows — it has no audio verdict in
this harness and the census marks every sound row `sameboy=unknown`. Derive
against the ROM's own ladder.

gambatte protocol: 15 frames + 1, then either an `_out<HEX>` glyph OCR of the top
tile row or an `_outaudio<0|1>` raw-sample verdict. Single ROM:
`cargo run -p slopgb-core --example run_gambatte -- <rom> [dmg|cgb]`.
Model routing from the filename: `_dmg08_cgb04c_outN` = both want N;
`_dmg08_outA_cgb04c_outB` = per-model; `_cgb04c_outN` = CGB only. **Do not
hand-roll this in a shell sweep** — it mis-routes `_ds` (CGB-only) ROMs onto DMG
and reports garbage. Use the gbtr harness for verdicts.

## Other clusters, if this stalls

`gambatte/enable_display` 16, `gambatte/window` 15, `gambatte/dma` 15 (but see
the halt→wake obstruction first), `mealybug/ppu` 12, `gambatte/lcd_offset` 8,
`gambatte/bgtilemap` 8.

## Documented obstructions — measured, do not re-derive

- **dma halt→wake duration gap.** `hdma_transition_{,ei_}halt_late_unhalt_scx1_1`
  and `hdma_late_{ei_,}m3halt_m2unhalt_ly_scx{1,2}_4`: read dots match SameBoy's
  to the frame offset, block *lines* and wake dots do not (ly 2 dot 224 vs ly 2
  cfl 89). Rung 4→5 of the `m2unhalt_ly` ladder moves our read 32 dots for a
  one-NOP change that should move it 4, with an identical post-wake sled. Fix the
  wake instant first; do not sweep the DMA seams. Full tables in `dma.md`.
- **`ch2_init_reset_env_counter_timing`, 4 rows.** Eight pairs over two knobs,
  tabulated both models in `apu.md`. Not the divider phase — DMG pairs 9-12 share
  a power-on and differ only in the post-trigger delay, yet need different step
  times, so the reference's volume at the later lock comes from the locking NR22
  write itself (NRx2 zombie mode). Start on the zombie semantics.
- **`ch1_init_reset_sweep_counter_timing_{4,10}`, 2 rows.** The baseline comment
  already names it: the 128 Hz sweep grid phase needs pinning <4 dots against the
  instruction stream, per model. Same class as the two post-boot constants that
  landed — likely a third one.
- **speedchange `ch2_nr52`, 6 EXCEED.** Demanded off-cycle is exactly the `b`
  read; a uniform DS shift is `+6/−2` and the split does not follow the switch
  count. See `apu.md` §Speed switch.
- **sprites `late_sizechange*_ds_1`, 7 EXCEED.** Needs the DS FF40 commit moved a
  dot earlier — a global render/read-law change, not scan-local.

## HARD RULES

- Never baseline a row the census marks SameBoy-PASS (bucket BUG).
- **A `+N/−M` score is a missing discriminator, not a floor. If the −M row is
  currently GREEN, do not ship it at all.** Four changes scored `+3/−5`, `+2/−2`,
  `+11/−3` and `+3/−0` last session; the first three all had a real discriminator
  and all landed `+N/−0`.
- Never score a trade on a hand-picked row list. Run the whole corpus
  (`SLOPGB_GBTR_CENSUS=<file> cargo test -p slopgb-core --test gbtr --release`,
  then diff pass/fail against the previous dump).
- **Revert every probe before committing, and *read the output* of the revert.**
  `grep -rn "DBG" crates/slopgb-core/src/` must print nothing and
  `git status --porcelain` must show only intended files. Note `git checkout --`
  on a file you also changed intentionally reverts *both* — re-apply and re-check.
- Comments: no process narrative, no A/B-sweep stories. Every named symbol must
  exist (grep it). A "byte-identical if removed" claim must be probed.
- Every landed law carries a red-before-green pin: flip each arm individually and
  confirm the unit test fails.
- `/rust-diff-review` the diff before committing; fix every finding.
- Core: std only, `forbid(unsafe_code)`, every `.rs` under 1000 lines, clippy
  `-D warnings` clean.
- Build: `CARGO_TARGET_DIR=target/<name>`. Never `pkill` a build sharing a target dir.
- Commits: SSH-signed, committer `richard@richardmoch.xyz`,
  `export SSH_AUTH_SOCK=/run/user/1000/ssh-agent.socket`.

## GATES before any commit

```sh
cargo test -p slopgb-core --test gbtr --release   # 221/221 (~4.5 min)
cargo test -p slopgb-core --test mooneye --release # 93 suite tests
cargo test -p slopgb-core --lib                    # 902
cargo test -p slopgb --bins                        # 676
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

On landing, in this order:

1. Ratchet `baselines/gambatte.txt` by **exactly** the recovered keys.
2. Recapture the golden:
   `SLOPGB_GOLDEN=capture cargo test -p slopgb-core --release --test gbtr golden_fingerprint`
   then verify the drift **adds and removes no key**:
   `diff <(cut -d'|' -f1 old|sort) <(cut -d'|' -f1 new|sort)` must be empty.
   Inspect the value drift too — a post-boot change moves audio hashes on
   unrelated ROMs (last session: one `bootrom_dumper` audio hash, frame hash
   unchanged), which is expected; a *frame* hash moving on an unrelated ROM is not.
3. Apply the census delta by **filtering** the old table against the recovered
   keys — do NOT regenerate it; the classifiers silently under-run.
4. Update the baselined-floor-case count in `CLAUDE.md` (currently **406**).
5. Write the law to `docs/hardware-state/<subsystem>.md` with its pinning ladder,
   and add a unit test.
6. Re-run the full gbtr to confirm 221/221 with the ratcheted baseline.
