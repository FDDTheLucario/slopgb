# slopgb hardware-state notes

Per-subsystem implementation-state + hardware-behavior notes (timing laws, quirks, the test ROMs that pin each, the parked/disproven approaches). **Read the relevant file before touching that subsystem** — every baselined cluster is an A/B-swept trade, and most notes name a dead-end not to re-chase. The floor-class index with lift conditions lives in `crates/slopgb-core/tests/gbtr/baselines/gambatte.txt`; `docs/ARCHITECTURE.md` has the timing contract and module ownership.

State date: 2026-06-14.

| File | Covers |
|---|---|
| [test-status.md](test-status.md) | mooneye/gbtr scoreboard, harness, ROM-bundle requirement |
| [cpu-interrupts.md](cpu-interrupts.md) | dispatch-ack sync-ahead, interrupt sampling, halt wake, HALT/STOP gate, CGB speed switch |
| [dma.md](dma.md) | OAM DMA bus conflicts, OAM×VRAM DMA composition, CGB VRAM (HBlank) DMA |
| [ppu-timing.md](ppu-timing.md) | mode-3 write strobe, SCX hunt, STAT IRQ events, post-boot LCD phase, CGB-C LY/STAT timeline, mode-0 end-of-line grid |
| [ppu-render.md](ppu-render.md) | OAM scan, window machine, mode-3 fetch, mealybug, DMG OAM bug, boot VRAM, frame skip, IRQ drain |
| [apu.md](apu.md) | post-boot warmup, SameBoy countdown model, ch1 sweep |
| [io-misc.md](io-misc.md) | serial clock, SGB joypad, MBC30, public API facade, audio frontend |
| [sgb.md](sgb.md) | SGB presentation: palette/attr/mask commands (PAL01-12, ATTR_BLK, MASK_EN), colorization wiring, deferred commands |
| [sgb-audio.md](sgb-audio.md) | SGB audio: SNES S-DSP (BRR/envelope/Gaussian/echo/noise), SPC700↔DSP clocking, SGB sound-command routing, BIOS gating (what works without it) |
| [sgb-icd2.md](sgb-icd2.md) | ICD2 bridge: SNES-side `$6000-$7FFF` register spec (fullsnes), packet mailbox / pad-latch semantics, the host-window plugin crossing design |
