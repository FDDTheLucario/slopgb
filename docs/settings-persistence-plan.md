# Settings persistence — two-phase TDD plan

`Settings` (`crates/slopgb/src/windows/options.rs`) is **in-memory only** today:
seeded from `Settings::default()` + the CLI `--model` at startup (`main.rs`), never
written to disk, reset every launch. Same for `recent` ROMs (main.rs:249 "on-disk
persistence deferred") and window geometry. This plan adds persistence in two
phases the user asked for:

1. **bgb.ini compatibility** — read/write the same file bgb uses, so the config is
   interoperable (a user can share `bgb.ini` between bgb and slopgb).
2. **A modern native format** — a versioned, sectioned, std-only text format that
   supersedes bgb.ini as the default store, with bgb.ini kept as import/export.

## The one hard constraint: no new deps

`crates/slopgb` is **winit/softbuffer/cpal only** (CLAUDE.md). So **no serde, no
`toml`/`ini`/`serde_json` crate** — every parser and serializer is hand-rolled
std. This is the dominant design driver and it rules out the obvious `#[derive]`
path. It's tractable: `Settings` is ~30 flat scalars (bools, ints, a float, four
colors, three path strings), and the repo already hand-rolls codecs (the CDL RLE,
the `.sym` parser), so a line-based config codec is on-brand.

## Format choice for phase 2 (my call, since you delegated it)

**A versioned, sectioned key=value text format** (a small TOML subset), *not*
binary. Rationale:
- Settings are <1 KB of scalars — parse cost is noise, so binary compactness buys
  nothing real; human-editability + `git diff`-ability + "open it in a text editor
  to debug" are worth far more.
- Sections (`[graphics]`, `[sound]`, `[debug]`, `[system]`) group the fields;
  a `version = N` header enables migrations; unknown keys are **preserved + warned**
  (forward-compat) and missing keys **default** (back-compat).
- std-only, and the line-based parser is a superset of phase-1's `bgb.ini` reader,
  so phase 2 reuses that foundation instead of pulling a TOML crate.

If you'd rather have strict TOML (via a vendored mini-parser) or a binary format,
say so — this is the reversible call.

## Shared foundation (built in phase 1, reused in phase 2)

- **Config-dir resolver** (std env only): `$XDG_CONFIG_HOME/slopgb/` else
  `~/.config/slopgb/` on Linux; `%APPDATA%\slopgb\` on Windows. One function.
- **Atomic write**: write to `*.tmp` then `rename` (no partial file on crash).
- **Load-on-startup + save-on-change hooks**: seed `App.settings` from disk in
  `main`, save in `apply_settings` (Options OK/Apply) and on quit. Debounce is
  unnecessary (saves are user-initiated).
- **Ordered-line model with unknown-key preservation**: parse a config file into
  `Vec<Line>` (`KeyVal{key,val,raw}` | `Comment(raw)` | `Blank`), mutate only the
  keys we model, re-serialize preserving every other line byte-for-byte. This is
  what makes "don't clobber the user's other bgb settings" possible and is the
  crux of both phases.

---

## Phase 1 — bgb.ini compatibility

```xml
<plan goal="Read + write bgb.ini as the settings store, byte-preserving unknown keys, so config interops with real bgb">

  <analysis id="1" model="opus" deps="none">
    <do>Capture a real bgb.ini via the bgb-reference rig (docs/bgb-reference/README.md §capture) and CATALOG every key: name, type (0/1 bool / int / string / COLORREF), and which Settings field (if any) it maps to. Record the file's exact shape (sections? CRLF? key order, COLORREF encoding decimal-vs-hex, BGR byte order). Produce the authoritative key↔field mapping table + the list of bgb keys we don't model (round-trip-only) and our slopgb-extra fields with no bgb key.</do>
    <done>docs/settings-persistence-plan.md gains a "bgb.ini key map" appendix; a sample bgb.ini committed under docs/bgb-reference/ as a fixture.</done>
    <why>Every later task depends on the real key names + encodings; guessing them is the main risk. No bgb.ini exists in-repo yet.</why>
  </analysis>

  <task id="2" model="sonnet" deps="1">
    <do>std-only ordered-line INI model in a new `settings_file/ini.rs`: parse(&amp;str)->Vec&lt;Line&gt; (KeyVal/Comment/Blank, preserving raw text + trailing CRLF), serialize(&amp;[Line])->String byte-identical for untouched lines; get(key)/set(key,val) that edits in place or appends.</do>
    <test>Unit: parse→serialize of the captured bgb.ini fixture is byte-identical; set() on an existing key changes only that line; set() on a new key appends; a comment/blank line survives round-trip.</test>
    <done>bgb.ini round-trips byte-for-byte; edits are surgical.</done>
  </task>

  <task id="3" model="sonnet" deps="2">
    <do>Typed accessors over the model: bool as "0"/"1", i32/u32, f32 (volume), String, and COLORREF color with the BGR↔our-XRGB(0x00RRGGBB) byte swap (docs already note Color0..3 are BGR). One encode+decode fn per type.</do>
    <test>Unit: `1`↔true, `0`↔false; a COLORREF like bgb's `Color0` decodes to the known E8FCCC palette entry and re-encodes to the original bytes (the BGR swap is symmetric); an out-of-range/garbage value falls back without panic.</test>
    <done>Every bgb value type round-trips, colors included.</done>
  </task>

  <task id="4" model="opus" deps="1,3">
    <do>Settings↔bgb.ini mapping in `settings_file/bgb.rs`: from_bgb_ini(model)->Settings (recognized keys → fields, missing → Settings::default value); write_bgb_ini(settings, existing_model)->Vec&lt;Line&gt; that updates ONLY mapped keys and leaves every unmodelled bgb key untouched. slopgb-extra fields (tile_hex_8bit, framerate_limit, esc_shows_debugger, …) written under a `Slopgb`-prefixed key bgb ignores.</do>
    <test>Round-trip: Settings→write→parse→from == Settings on the mapped subset; a fixture bgb.ini with keys we don't model still has those keys present after a save; a `SlopgbTileHex8bit=1` survives our round-trip and is absent from bgb's own keys.</test>
    <done>Full-fidelity read/write; bgb's unmodelled settings are never lost.</done>
    <why>The correctness core — the mapping + the preserve-unknown invariant is where "complete compatibility" lives or dies.</why>
  </task>

  <task id="5" model="sonnet" deps="4">
    <do>Config path + IO glue in `settings_file.rs`: resolve the config dir (std env), locate bgb.ini (default = config dir; overridable via a CLI flag / env for pointing at a real bgb install), load_settings()->Settings (defaults if absent/unreadable), save_settings(&amp;Settings) with atomic temp+rename. Malformed file → defaults + a logged warning, never a crash.</do>
    <test>Round-trip through a tempdir: save_settings then load_settings reconstructs the fields; a missing file yields Settings::default(); a truncated/garbage file yields defaults without panic.</test>
    <done>Settings survive a save/load cycle on disk; corruption degrades gracefully.</done>
  </task>

  <task id="6" model="sonnet" deps="5">
    <do>Wire into the App lifecycle: seed App.settings from load_settings() at startup (main.rs, after the CLI --model override precedence is decided — CLI wins for the session but isn't persisted); call save_settings() in apply_settings() (Options OK/Apply) and on quit (Action::Quit / window close).</do>
    <test>App-level test (mirroring session/save tests): construct the App with a temp config dir, flip a setting via apply, assert the on-disk file now reflects it; relaunch (new App, same dir) and assert the setting is restored.</test>
    <done>Settings persist across launches through the real Options flow.</done>
  </task>

  <task id="7" model="sonnet" deps="1,4">
    <do>Extend the round-trip to bgb's non-Settings state we can honor: window geometry (`*WinX/Y`, `*WinShowOnStart`) and recent ROMs if bgb stores them — map the ones we already have App state for (recent list, tool-window open flags), preserve the rest. Gate behind what task 1 actually found; skip keys with no slopgb analogue (still preserved as unknown).</do>
    <test>A bgb.ini with DebugWinShowOnStart=1 opens the debugger on load; recent-ROM keys (if present) populate the Recent menu; unknown geometry keys survive a save.</test>
    <done>bgb's window/recent state interops where slopgb has an equivalent, and is preserved where it doesn't.</done>
  </task>

</plan>
```

---

## Phase 2 — modern native format

```xml
<plan goal="A versioned sectioned std-only settings format as the default store; bgb.ini demoted to import/export">

  <analysis id="1" model="opus" deps="none">
    <do>Spec the format in this doc: a `[section] / key = value / # comment` subset with a top `version = 1`; sections graphics/sound/debug/system/misc/paths; value types string, int, float, bool (true/false), hex color `0xRRGGBB`, and list (palette). Define precedence (native file wins; bgb.ini used only when native is absent, then migrated) and the migration/version-bump policy (unknown keys preserved+warned, missing keys defaulted, version drives future migrations).</do>
    <done>Format grammar + the Settings→section/key schema table recorded here.</done>
    <why>Design-and-tradeoff task; the grammar + precedence choices gate the parser and the migration.</why>
  </analysis>

  <task id="2" model="sonnet" deps="1">
    <do>std-only parser/serializer in `settings_file/native.rs`: parse(&amp;str)->Doc (sections → ordered Vec&lt;Line&gt;, reusing the phase-1 Line model with section headers added), serialize byte-preserving untouched lines; typed get/set (str/int/float/bool/hexcolor/list). Reject a wrong/absent `version` header gracefully.</do>
    <test>Unit: round-trip a sample native file byte-identical; a `[section]` groups keys; a hex color `0x112233` decodes to our XRGB and re-encodes; an unknown key in a known section survives; a missing version → treated as v1 with a warning.</test>
    <done>The native format round-trips with sections + types + unknown-key preservation.</done>
  </task>

  <task id="3" model="sonnet" deps="2">
    <do>Settings↔Doc schema in `native.rs`: one typed field↔(section,key) row per Settings field; from_doc defaults missing keys, to_doc updates only known keys and preserves the rest; a version-migration hook (v1→v2 stub) for future field renames.</do>
    <test>Round-trip Settings→to_doc→from_doc == Settings (all fields, palette included); a doc missing a newly-added field loads it at its default; an extra future key is preserved through a save.</test>
    <done>Full-fidelity native serialization with forward/backward compatibility.</done>
  </task>

  <task id="4" model="sonnet" deps="3">
    <do>Precedence + migration in `settings_file.rs`: load_settings prefers the native file; if it's absent but a bgb.ini exists, import it (phase-1 from_bgb_ini) and write the native file; save_settings writes native. Add Options / File menu "Import bgb.ini…" and "Export bgb.ini…" so interop is explicit and retained.</do>
    <test>Tempdir: with only a bgb.ini present, load imports it and creates the native file; with both present, the native file wins; Export writes a bgb-readable file (phase-1 write) from the current Settings.</test>
    <done>Native is the source of truth; bgb.ini is a first-class import/export path.</done>
  </task>

  <task id="5" model="haiku" deps="4">
    <do>Reuse the phase-1 config-dir + atomic-write for the native file (`settings.conf` or `slopgb.toml` in the config dir); no new IO code, just the new filename + a corrupt-file backup (`.bak`) on parse failure before falling back to defaults.</do>
    <test>A corrupt native file is renamed to `.bak` and defaults load, without data loss or panic.</test>
    <done>Native file uses the shared IO; corruption is recoverable.</done>
  </task>

  <task id="6" model="sonnet" deps="4">
    <do>Options polish: a "settings file: &lt;path&gt;" line + "Reset all to defaults" (writes defaults) + "Open config folder"; document the format + precedence in docs/ui-state/options.md.</do>
    <test>Reset-all writes a defaults doc; the path shown matches the resolver; options.md updated (clean-docs).</test>
    <done>Users can see + reset + locate their config; docs current.</done>
  </task>

</plan>
```

---

## Sequencing + risks

- **Phase 1 gates on task 1** (capture a real bgb.ini) — do that first; the mapping
  is guesswork without it. The bgb-reference capture rig already exists.
- **Phase 2 builds on phase 1's Line model + IO** — don't start it until phase 1's
  parser/round-trip is green, or you'll fork two parsers.
- **The preserve-unknown-keys invariant** is the highest-risk correctness property
  in both phases (it's what "don't corrupt the user's file" means). It gets a
  dedicated round-trip test in each phase (P1 task 4, P2 tasks 2-3).
- **Precedence** (CLI --model vs file, native vs bgb.ini) is a deliberate policy,
  spelled out in P1 task 6 / P2 task 1 — not left implicit.
- **Slopgb-only fields** (tile_hex_8bit, framerate, esc_shows_debugger, …) have no
  bgb key; phase 1 stores them under a `Slopgb`-prefix bgb ignores, phase 2 gives
  them real sectioned keys.

**Totals:** Phase 1 = 7 tasks (2 opus incl. the capture/mapping, 4 sonnet, 0 haiku +
1 analysis). Phase 2 = 6 tasks (1 opus analysis, 4 sonnet, 1 haiku). Ship phase 1
end-to-end (interop + persistence) before starting phase 2 (native + migration).
