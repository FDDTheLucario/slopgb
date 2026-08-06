slopgb — next task: keep differencing the PPU read frame against SameBoy

## Repo state (verified 2026-08-06)

`main` @ `c007bdd5`, clean tree, no open branch.

gbtr **221/221** with **339** baselined floor cases (was 349); mooneye
**93/93** suite tests (439/439 rom×model); core lib **911**; frontend **676**;
clippy + fmt clean; `golden_fingerprint` recaptured (13 cases drifted, all in
the CGB STAT-read cluster, no verdict changes).

`docs/hardware-state/floor-census.tsv` is current: 338 rows, **245** with a
SameBoy verdict (the CGB classifier was writing where nobody read — fixed in
`bb755f63`, which is what raised coverage from 78). Chaseable = SameBoy PASS +
we fail:

| cluster | chaseable | | cluster | chaseable |
|---|---|---|---|---|
| `dma` | 15 | | `window` | 6 |
| `mealybug/ppu` | 12 | | `lycEnable` | 6 |
| `lcd_offset` | 8 | | `m1` | 5 |
| `enable_display` | 8 | | `sprites`, `m2enable` | 4, 4 |
| `bgtilemap` | 8 | | `bgtiledata`, `ly0` | 4, 4 |
| `scx_during_m3` | 7 | | `speedchange` | 3 |
| `halt` | 7 | | | |

## READ FIRST

- `docs/hardware-state/ppu-timing.md` § **"The FF41 read frame"** — the law just
  landed, its two load-bearing scopes, and the measurement that produced it.
- The floor-class index header in `tests/gbtr/baselines/gambatte.txt`.
- `docs/sameboy-port/tools/README.md` — the SameBoy ground-truth rig.

## THE METHOD THAT WORKED (repeat it)

Do not sweep a constant. Difference the read against SameBoy:

1. `docs/sameboy-port/tools/build_sameboy_tracers.sh` (cached at
   `~/.cache/sbbuild`), then `SB_TRACE=1 sameboy_tester --cgb --length 4 <rom>`.
   `SBMODE` = visible mode change, `SBREAD ff41` = the read instant. **Difference
   `fp=` (absolute 8 MHz), never `cfl`/`dc`** — those reset per line and gave a
   1-dot phantom jitter here.
2. Anchor both sides on the same event (mode-3 entry works; the line-start event
   carries a 4-dot ambiguity). The anchor cancels out of the resulting
   inequality, so a wrong anchor is survivable but a mixed one is not.
3. Our side: `cargo run -p slopgb-core --example probe_statread -- <rom> <pc>`
   prints the value the read actually latched (register A), which is the only
   thing that matters — a post-step `debug_read(0xFF41)` is a DIFFERENT read one
   or two M-cycles later and disagreed with A on exactly the failing rows.
4. Find the read PC by `cmp -l` on the `_1`/`_2` ROM pair: they differ by one
   inserted NOP, so the read instruction moves by one byte between rungs.
5. A/B with `SLOPGB_GBTR_CENSUS=<file>` per variant and diff the two dumps per
   row; the suite's own pass/fail summary hides which rows traded.

gambatte builds the same way and is worth instrumenting for a second opinion
(`LCD::getStat`, `m0TimeOfCurrentLine`) but it is NOT the oracle: it passed
these rows with an m0 time 2 dots off SameBoy's edge and its own `cc + 2`
read lead cancelling it.

## TASK

Take the next chaseable cluster. `dma` still has 15, but they are now the
HDMA-seam / speed-switch families (class B), not the read frame. The read-frame
vein that just paid: `lcd_offset` (8) and `enable_display` (8) are the same
shape of read on a shifted/enable frame, and `bgtilemap`/`bgtiledata`/
`scx_during_m3` (19 together) are render-length rows a pixel diff against
SameBoy's framebuffer would localize the same way.

Gate every row through both references before investing in it — a row SameBoy
also fails is class G and is not chaseable.

## Constraints

- **Zero regressions.** Growing a baseline is a regression.
- Verify in order: unit tests → the affected suite → full gbtr +
  `golden_fingerprint` → mooneye → frontend.
- Recapture golden with `SLOPGB_GOLDEN=capture` only after reading the drift
  list and confirming every drifted case is in the cluster you touched.
- Standing repo law: no new deps, no unsafe, files <1000 lines, SSH-signed
  commits (`export SSH_AUTH_SOCK=/run/user/1000/ssh-agent.socket`),
  `/rust-diff-review` per iteration.
- One `CARGO_TARGET_DIR` per concurrent run; never `pkill` a build sharing one.

## Last session

| commit | law | rows |
|---|---|---|
| `c007bdd5` | a polled CGB FF41 read sees mode 0 from `flip - 5`, not `flip - 3` | +10 |
| `bb755f63` | the CGB classifier now writes where `census.py` reads | +167 measured |
