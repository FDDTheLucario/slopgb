slopgb — next task: point the differencing rig at the PPU/DMA clusters

## Repo state (verified 2026-08-05)

`main` @ `86ad0c7e`, clean tree, pushed, no open branch. The APU granule-grid
work is merged and the branch is deleted.

gbtr **221/221**; mooneye **93/93** suite tests (439/439 rom×model); core lib
**910**; frontend **676**; clippy + fmt clean; `golden_fingerprint` current.

gambatte baseline **250 keys**, **349** baselined floor cases across all suites.
By cluster:

| cluster | rows | of which `_ds` | | cluster | rows |
|---|---|---|---|---|---|
| `dma` | 44 | 9 | | `enable_display` | 10 |
| `m1` | 20 | 7 | | `scy` | 9 |
| `lcd_offset` | 19 | 5 | | `scx_during_m3` | 8 |
| `lycEnable` | 17 | 11 | | `oamdma` | 8 |
| `speedchange` | 14 | — | | `m2enable` | 8 |
| `sprites` | 13 | — | | `dmgpalette_during_m3` | 8 |
| `halt` | 11 | — | | `bgtilemap` | 8 |
| `sound` | 10 | — | | `window`, `ly0` | 7, 7 |

109 of the 250 are `_ds`. The `dma` cluster is mostly the
`hdma_transition_*` / `hdma_late_m3speedchange_*` families — the speed-switch +
HDMA seam (class B), not the DS pixel pipe.

## READ FIRST

- The floor-class index header in `tests/gbtr/baselines/gambatte.txt` — every
  baselined cluster is an A/B-swept trade; one-sided "fixes" regress the green
  siblings.
- `docs/hardware-state/apu.md` § **"Differential trace against gambatte"** — the
  rig, both build recipes, and the method that closed the APU work.

## THE ASSET: two reference emulators, buildable in one command each

This is what changed last session and it is the reason to pick these clusters up
now. Both references build standalone on this box and can be instrumented:

```sh
# gambatte (GPL-2.0, same terms as this project)
g++ -O1 -fno-exceptions -fno-rtti -DHAVE_STDINT_H \
    -I libgambatte/include -I libgambatte/src -I common -o gbprobe main.cpp \
    $(find libgambatte/src -name '*.cpp' | grep -vE 'cinterface|file_zip')

# SameBoy (needs rgbasm/rgblink for its boot ROMs — present on this box)
make tester -j4 CONF=release        # -> build/bin/tester/sameboy_tester <rom>
```

Clone both OUTSIDE the repo tree; nothing is vendored. `main.cpp` is a dozen
lines against `gambatte.h`. Printing internal state from either turns a floor
row from a fitting problem into a differencing problem — which is how the APU
granule grid was found (+7 rows) and how the duty family was closed.

## TASK

Pick a cluster and difference it, biggest first. `dma` (44) and `lycEnable` (17,
two thirds `_ds`) are the obvious candidates; `m1` and `lcd_offset` sit on the
same double-speed sub-cycle seam the APU grid turned out to be.

**Gate every row before investing in it**: run it through both references first.
A row SameBoy also fails is class G (upstream tie-break needed) and is not
chaseable — the twelve APU duty rows burned most of a session before that check
was run. It costs two commands now.

## Method notes that earned their place

- **Check both references' verdicts BEFORE chasing a row.** The single most
  expensive omission of last session.
- **Build a no-op control first.** Running the matrix with the new mechanism
  pinned inert proved the APU restructure byte-identical before any row moved;
  without it a "+10/−4" is uninterpretable.
- **Compare intervals, not absolutes.** slopgb's post-boot warmup runs ~2.1 M
  APU granules before the ROM starts; align on a shared event and difference.
- **Levers do NOT compose.** `lf_div` rides the APU's granule parity, so a
  one-granule clock change flips it and moves the trigger delay the other way.
  Two separately measured deltas do not add — re-measure every combination.
- **Read the frozen state, not the pass/fail bit.** `ch1.duty_pos` gave the
  distance directly and killed a whole class of candidate fixes in one run.
- **Diff censuses per row.** `SLOPGB_GBTR_CENSUS=<file>` per variant, then diff.
- **Fast iteration**: run the test binary directly
  (`target/<dir>/debug/deps/gbtr-*`) with `SLOPGB_GBTR_FILTER=<substring>` —
  ~30 s versus ~400 s for a full `cargo test` invocation.
- **One `CARGO_TARGET_DIR` per concurrent run**, or they serialize on the lock
  (a 16-point sweep managed one point in 25 minutes this way).
- **Never `pkill` a build sharing a `CARGO_TARGET_DIR`** — repo law, and it was
  violated last session. If it happens: `cargo test --no-run` to rebuild, then
  re-run a known-green baseline before trusting any number.
- **Restore baselines with `git checkout --`, never a `/tmp` copy.**

## Constraints

- **Zero regressions.** Growing a baseline is a regression (harness law).
- **Never drop a row SameBoy passes** for a gambatte-derived change. This
  already cost the single-speed half of the APU edge deferral.
- Verify in order for core changes: unit tests → the affected suite → full gbtr
  + `golden_fingerprint` → mooneye → frontend.
- Standing repo law: no new deps, no unsafe, files <1000 lines, SSH-signed
  commits (`export SSH_AUTH_SOCK=/run/user/1000/ssh-agent.socket`),
  `/rust-diff-review` per iteration.

## Last session

| commit | law | rows |
|---|---|---|
| `8f729584` | the APU observes itself on a 2 MHz granule grid a power-on or a leaving speed switch can offset by a cycle | +7 |

Plus: the twelve `ch1_duty0_pos6_to_pos7_timing` rows closed as **class G** —
SameBoy fails them too, one 2 MHz cycle in the opposite direction from us, with
gambatte's expectation between the two. The sign flip this port drops is
provably the exact compensation for tick-then-access sampling the write at the
machine cycle's end rather than its start. Do not re-open without hardware
evidence; the full lever table is in `apu.md`.
