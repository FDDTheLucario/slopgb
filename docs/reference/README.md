# Vendored hardware references

External documentation pinned for offline, reproducible citation. The clones
themselves are gitignored — this file records what to fetch and at which commit,
so a citation made today resolves to the same text later.

## Pan Docs

The referee oracle for the test-ROM floor census
(`docs/hardware-state/floor-census.tsv`): when two test suites disagree about
hardware behavior, Pan Docs settles it.

| | |
|---|---|
| Source | <https://github.com/gbdev/pandocs> |
| Pinned commit | `fe246067b695b5404a4a6a47efb4fd6d921ececb` |
| Commit date | 2026-06-09 |
| Fetched | 2026-07-28 |
| Path | `docs/reference/pandocs/` |

```sh
git clone --depth 1 https://github.com/gbdev/pandocs docs/reference/pandocs
```

To pin a different revision, fetch unshallowed and check the hash out, then
update the table above in the same commit as whatever citation depends on it.

### Citing

Cite the mdBook **source**, not the rendered site — gbdev.io is not reachable
from every environment this repo builds in, and the source is what is pinned
here. Anchor form:

```
pandocs src/Rendering.md#mode-3-length
```

75 chapters live under `docs/reference/pandocs/src/`. The ones the census leans
on: `Rendering.md`, `STAT.md`, `Accessing_VRAM_and_OAM.md`, `LCDC.md`,
`Scrolling.md`, `OAM_DMA_Transfer.md`, `CGB_Registers.md` (HDMA), `Audio*.md`,
`Interrupt_Sources.md`, `halt.md`.

Pan Docs is a referee, not an authority on sub-dot timing — it documents *what*
the hardware does, rarely *at which dot*. Where it is silent, the ROM's own
capture provenance decides (see the oracle table in the census plan), and a row
whose expectation Pan Docs neither supports nor contradicts stays baselined
rather than being exempted.
