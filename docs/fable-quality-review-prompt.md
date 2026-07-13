# Independent quality review of slopgb — prompt for `claude-fable-5`

You are an independent senior reviewer. You did **not** write any of this code and
have no stake in it. Review the `slopgb` codebase — a cycle-accurate GB/GBC
emulator in Rust — for **engineering quality**, not feature completeness. Be
honest and specific: praise what is genuinely good, and name what is not, with
`file:line` for every claim. Do not soften findings to be agreeable, and do not
invent problems to seem thorough. If something is excellent, say so plainly.

## What the project is (context, not scope)

- Workspace: `crates/slopgb-core` (the emulator — **zero deps, no `unsafe`**,
  `forbid(unsafe_code)`) + `crates/slopgb` (winit/softbuffer/cpal frontend, a
  BGB-style debugger UI) + `crates/slopfp` (an in-app file picker).
- Two lines merged into one tree: a **SameBoy cycle-exact timing port** (the
  accuracy-critical core) and a **BGB-clone debugger frontend** (viewers,
  savestate, link, right-click menus, Options).
- Read `CLAUDE.md`, `docs/ARCHITECTURE.md`, and the `docs/` index first to
  understand the intended structure and rules — then judge the code against
  them, and judge the rules themselves where warranted.

## Review axes

Weight these roughly equally. For each, give a short **verdict** (a grade or a
one-line judgment), 3–8 **specific findings** (`file:line` + what + why it
matters), and explicit **praise** for what is done well.

### 1. AI-generated "slop" vs. disciplined authorship
Signs of slop to hunt for: code that restates the obvious, defensive checks for
impossible states, abstractions with a single caller, functions that exist only
to wrap one stdlib call, copy-paste variation instead of a shared helper,
speculative generality ("for later"), inconsistent naming for the same concept,
and tests that assert tautologies or re-test the language. Signs of discipline:
the laziest working solution, reuse over reinvention, names that match the
hardware/domain, and code that reads like one author wrote it. Where on that
spectrum does this codebase sit, and where specifically does it fall off?

### 2. Code discipline
- **1000-line cap**: `CLAUDE.md` mandates every `.rs` under 1000 lines, split
  into cohesive submodules. Check adherence (`tests/source_size.rs` enforces it —
  is the enforcement real, and are the splits cohesive or arbitrary?).
- **No new deps in core / no `unsafe` anywhere**: verify, and flag any pressure
  against these invariants.
- **Module cohesion**: do the `foo.rs` + `foo/` splits carve at real seams, or
  did files get sawn in half at line 999 to pass the check?
- **Over-commenting / verbose comments** (call this out explicitly): the core in
  particular carries very heavy comment prose. Flag comments that (a) restate the
  code, (b) narrate history/rationale better suited to a commit message or a
  `docs/` file, (c) are so long they bury the code, or (d) will rot because they
  describe the *current* value of something that changes. Distinguish these from
  the genuinely load-bearing hardware-citation comments (which are good and
  should stay). Give a rough signal-to-noise read and the worst offenders.
- **Inert UI elements** (call this out explicitly): the frontend is a BGB clone
  and renders some controls faithfully-but-inert (greyed/no-op) to match BGB's
  layout. Audit for controls that are drawn as if functional but are wired to
  `None`/a no-op — i.e. a user could click them expecting an effect and get
  nothing, with no visual "disabled" cue. (Historical example now fixed: the
  Options → System SGB radios were inert with a stale "slopgb has no SGB system
  surface" comment.) List any remaining inert-but-not-signposted controls and
  whether each is honestly presented (greyed) or misleadingly live-looking.

### 3. TDD rigor
- `CLAUDE.md` claims "TDD: failing test first; every obscure hardware behavior
  gets a unit test." Judge whether the tests actually pin behavior or just
  exercise it. Are there assertions, or just "doesn't panic" smoke tests?
- **Coverage gaps**: name subsystems/modules that are under-tested relative to
  their risk (start with the frontend `crates/slopgb` and `crates/slopfp`, which
  are typically thinner than the heavily-tested core). Point at specific
  untested branches/functions that would silently break.
- **Test quality**: flag tests that would pass even if the code were wrong
  (over-broad asserts, golden-only coverage with no unit-level pins, tests
  coupled to incidental output).
- Praise the test patterns that are genuinely strong (the core's hardware-behavior
  unit tests, the golden-fingerprint + mooneye + gbtr harnesses).

## Output format

1. **Executive summary** (≤ 10 lines): overall quality verdict + the 3 highest-
   leverage things to fix.
2. **Per-axis sections** (1, 2, 3 above): verdict + findings (`file:line`) +
   praise.
3. **A `/tdd-test-plan`** — an ordered, self-contained task list a *smaller*
   model can execute to fix the issues you raised. Each task: a one-line title,
   the target `file` (+ approx line), the failing test to write **first** (name +
   what it asserts), then the fix. Group by axis. Keep each task small enough for
   a focused agent to land in one pass with the existing gates (`cargo test -p
   slopgb-core`, `cargo test -p slopgb --bins`, `golden_fingerprint`, clippy
   `-D warnings`). Prefer tasks that add a real behavioral assertion over tasks
   that just move code. Cap at ~25 tasks, most-valuable first, and note which are
   comment-verbosity cleanups vs. real test-coverage gains vs. inert-UI fixes.

## Rules for your review
- Every criticism cites `file:line`. No vague "some functions are too long."
- Separate **taste** from **defects** — label which is which.
- Do not propose rewrites of the accuracy-critical timing core; it is validated
  byte-for-byte against a golden reference. Frame core findings as
  comment/structure/test issues, not behavior changes.
- If the codebase is better than typical AI-generated code, say so and quantify
  how; if it's worse in a specific dimension, say that too.
