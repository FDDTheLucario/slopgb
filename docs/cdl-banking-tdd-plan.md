# CDL bank-awareness — TDD task plan

Supersedes the flat-64K store in [`cdl-tdd-plan.md`](cdl-tdd-plan.md) (that plan
shipped v1 deliberately CPU-address-space, not physical offset — see its intro).
QA is right: the flat `Box<[u8;65536]>` collapses every ROM bank onto the single
`0x4000-0x7FFF` window and never sees SRAM/WRAM banking, so the tints don't map to
real ROM/RAM usage.

## Design (the lazy correct shape)

Replace the flat buffer with **one linear physical buffer** sized to the actual
machine, and **one shared translation function** `cdl_index(addr) -> Option<usize>`
routed through by *both* the mark sites and the read side — the exact pattern
`rom_bank_for` already uses so the debug view and the real bus can't disagree.

Buffer layout (bases are cumulative, region lens are fixed for a machine's life):

| Region | GB addr | phys len | source |
|---|---|---|---|
| ROM | `0000-7FFF` | `rom.len()` (pow2) | `cart.rom_offset(addr)` = `(rom_bank_for(addr<0x4000)&mask)*0x4000 + addr&0x3FFF` |
| VRAM | `8000-9FFF` | `0x4000` | `vbk*0x2000 + addr&0x1FFF` |
| SRAM | `A000-BFFF` | `ram.len()` (pow2, may be 0) | `cart.ram_offset(addr)` — `None` when disabled / RTC mapped |
| WRAM | `C000-FDFF` | `0x2000` DMG / `0x8000` CGB | `wram_index(addr)` (already handles echo + SVBK) |
| tail | `FE00-FFFF` | `0x200` | `addr-0xFE00` — OAM/IO/HRAM/IE, unbanked (keeps the HRAM-exec tint) |

`total = rom_len + 0x4000 + ram_len + wram_len + 0x200`. This is exactly QA's
"as big as the ROM plus RAM" (plus a tiny fixed tail so HRAM-executed DMA
routines still tint).

**Translation returns `Option`** so disabled-RAM / RTC-register accesses log
*nothing* (there is no physical byte) instead of the flat store's phantom
`A000-BFFF` mark — strictly more correct.

**Golden-safe is unchanged.** `cdl: Option<..>` stays `None` by default; every
mark keeps its `is_none()` early-out *first*, before any translation, so an off
CDL is the same no-op it is today. Borrow dance: compute the index via
`&self` *then* take `&mut self.cdl` (sequential, no overlap).

**Viewer needs almost nothing.** It already reads live-banked bytes via
`debug_read` and tints via `cdl_flag(addr)`; once `cdl_flag` translates, the tint
follows the currently-mapped bank for free. Only add-on: a bank label in the
status bar so you know which bank the tint belongs to.

**Save format** gains a small header (magic + version + the 4 region lens) so a
`.cdl` from a different cart is *rejected* on load instead of silently
mis-mapped. `load_cdl` becomes `&[u8] -> bool`.

### Ponytail cuts (named, not hidden)
- **No arbitrary-bank browser in the viewer.** You see the CDL for the
  *live-mapped* bank only (same as bgb's default mem view). Add a bank
  dropdown when someone actually needs to inspect an unmapped bank.
- **Length-header validation, not a ROM hash.** Two different same-size ROMs
  could false-accept a `.cdl`. `// ponytail:` the ceiling; upgrade to embedding
  the cart header checksum (2 bytes) if it ever bites.
- **VRAM/OAM/IO tinting kept** only for no-regression with v1's full-space
  coverage; it's banked-correct now regardless.

```xml
<plan goal="CDL keyed by physical ROM+RAM offset (bank-aware), one shared translation, golden-safe, viewer + save/load follow">

  <task id="1" model="sonnet" deps="none">
    <do>Cartridge phys-offset accessors (cartridge.rs): pub fn rom_offset(&amp;self,addr:u16)-&gt;usize = (rom_bank_for(addr&lt;0x4000)&amp;rom_bank_mask())*ROM_BANK_SIZE + (addr&amp;0x3FFF); pub fn ram_offset(&amp;self,addr:u16)-&gt;Option&lt;usize&gt; matching ram_target (Ram(bank)-&gt;ram_index, Mbc2-&gt;Some(addr&amp;0x1FF), Rtc/disabled/empty-&gt;None); pub fn rom_len(&amp;self)/ram_len(&amp;self).</do>
    <test>cartridge_tests: rom_offset agrees with rom_at across low+high area incl. MBC1 mode-1 (bank 0x20 into 0x0000); ram_offset is None when RAMG off and when an MBC3 RTC reg is mapped, Some(bank*0x2000+off) when enabled, Some(addr&amp;0x1FF) for MBC2; rom_len/ram_len are powers of two (or 0).</test>
    <done>Accessors return the same physical index the real read path computes.</done>
  </task>

  <task id="2" model="haiku" deps="none">
    <do>PPU VRAM bank accessor: pub fn vram_bank(&amp;self)-&gt;usize returning usize::from(self.vbk&amp;1) (ppu/mod.rs).</do>
    <test>ppu render/cgb test: after writing FF4F bit0, vram_bank()==1; 0 on DMG.</test>
    <done>Live VBK bank readable by the interconnect.</done>
    <why>One-line getter over an existing field.</why>
  </task>

  <task id="3" model="opus" deps="1,2">
    <do>Interconnect store rework (interconnect.rs + interconnect/debug.rs + memory.rs): change cdl to Option&lt;Box&lt;[u8]&gt;&gt;; set_cdl(true) allocates total = rom_len+0x4000+ram_len+wram_len+0x200; add cdl_index(&amp;self,addr:u16)-&gt;Option&lt;usize&gt; per the layout table (ROM|VRAM|SRAM|WRAM|tail), pub(super) wram_index already covers echo+SVBK; add cdl_mark(&amp;mut self,addr:u16,flag:u8){ if self.cdl.is_none(){return} if let Some(i)=self.cdl_index(addr){ if let Some(b)=&amp;mut self.cdl{ b[i]|=flag } } }; route check_access R/W and profile_pc X and cdl_flag(addr) through it.</do>
    <test>core interconnect_tests: with a &gt;1-bank ROM, execute the same 0x4000-region code under bank A then bank B and assert the X flag lands at two DISTINCT phys indexes (cdl_flag reads bank A vs B independently); a write to SRAM while RAMG off logs nothing; a WRAM write under SVBK=2 vs SVBK=1 lands at distinct indexes; cdl_flag defaults 0 / is 0 when off.</test>
    <done>Marks + reads key on physical bank; banks no longer alias.</done>
    <why>The crux: full translation table correctness + borrow-checker sequencing + it sits on the hot CPU read/write path, so the is_none() no-op must be provably byte-identical.</why>
  </task>

  <task id="4" model="sonnet" deps="3">
    <do>Golden-safety regression: assert set_cdl(true) never perturbs emulation (frame-hash on == off over N stepped frames on a banked ROM) and confirm the default-off gbtr golden_fingerprint path is untouched.</do>
    <test>core test: frame-hash(cdl on) == frame-hash(cdl off) same ROM+steps; run cargo test -p slopgb-core --test gbtr (golden_fingerprint) green.</test>
    <done>CDL-on output byte-identical to CDL-off; golden unchanged.</done>
  </task>

  <task id="5" model="sonnet" deps="3">
    <do>Save/load format (interconnect/debug.rs + lib.rs + slopgb/src/cdl.rs): change load_cdl to (&amp;[u8])-&gt;bool that validates len==expected total and copies in (else false); keep cdl_flags()-&gt;Option&lt;&amp;[u8]&gt; (now variable len); add a header codec in cdl.rs — cdl_file_encode(regions,&amp;flags)-&gt;Vec&lt;u8&gt; ("SLCD"+ver+rom/vram/sram/wram u32 LE + RLE body) and cdl_file_decode(&amp;bytes)-&gt;Option&lt;(header,Vec&lt;u8&gt;)&gt;.</do>
    <test>cdl.rs unit: file_encode-&gt;file_decode round-trips flags+region lens; a body whose region lens differ from the live machine is rejected (None/false); load_cdl rejects a wrong-length slice; update cdl_save_load_pipeline (cdl.rs:101 try_from), lib_tests.rs (:509,:556 Some(65536) asserts), windows_tests.rs (:115) to the machine-sized buffer — a bare 65536 fixture now mismatches a non-64K cart and load_cdl returns false.</test>
    <done>A .cdl carries its layout and only loads onto a matching cart.</done>
  </task>

  <task id="6" model="sonnet" deps="5">
    <do>App wiring (app_path.rs): CdlSave writes cdl_file_encode(current region lens, cdl_flags()); CdlLoad reads-&gt;cdl_file_decode-&gt;validate-&gt;load_cdl, logging "bad/mismatched CDL file" on reject; delete the &lt;[u8;65536]&gt;::try_from path (line 209).</do>
    <test>windows/dbg round-trip test through the fs seam (tempdir): save a logged buffer, reload, assert cdl_flag matches; a truncated/foreign-cart file is rejected without panic.</test>
    <done>Menu Save/Load CDL round-trips per-cart; mismatch is refused.</done>
  </task>

  <task id="7" model="haiku" deps="1,2,3">
    <do>Viewer bank label (windows.rs render_memory_window status bar): append the live bank for the region mem_base falls in — ROM(rom_bank), SRAM(ram_bank), WRAM(svbk), VRAM(vbk) — e.g. "4000:XX  Name+Y". Needs GameBoy wram_bank()/vram_bank() forwarders.</do>
    <test>windows_tests headless render: status bar for mem_base 0x4000 contains the current rom_bank; 0xD000 contains the wram bank.</test>
    <done>Status bar names which bank the tint belongs to.</done>
    <why>Small string format over accessors that already exist / are one-line forwarders.</why>
  </task>

</plan>
```

**7 tasks: 2 haiku, 4 sonnet, 1 opus.** Critical path:
`1 (cart offsets) + 2 (vbk) → 3 (translation+store) → 4 (golden verify)`; save
chain `3 → 5 → 6`; viewer `3 → 7`. Task 3 is the only hard one — everything else
exposes or consumes offsets it already knows how to compute.
