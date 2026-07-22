# Save states + serial link cable

## State submenu (`SubKind::State`)

- **Quick Save / Quick Load** — snapshot/restore the whole machine in memory.
  `GameBoy: Clone` (a transitive derive across the core, runtime-inert / golden-safe
  — gbtr fingerprint byte-identical) feeds `Session.quick_state: Option<Box<GameBoy>>`
  (`quick_save`/`quick_load`). Survives reset (rewind past it); auto-cleared by a ROM
  change (fresh `Session`). **Click-only** — bgb's F2/F4/F3 accelerator labels are
  dropped (slopgb binds game-window F2/F3/F4 to the debugger/VRAM/iomap windows).
- **Load state...** restores an on-disk save state via the shared path modal
  (`PathPurpose::LoadState`). Select / Load-recovery stay greyed.

## On-disk save states (golden-safe)

A manual std-only binary serializer (`slopgb_core::state` Writer/Reader, no
serde/no unsafe):

- `GameBoy::save_state(&self) → Vec<u8>` — magic+version+ROM-fingerprint header then
  every peripheral's `write_state`.
- `load_state(&mut self, &[u8]) → Result<(), StateError>` — validates the header +
  ROM key vs the loaded cart, then restores **atomically into a clone**, so a
  bad/foreign/truncated file leaves the machine intact.

The header carries a `bool` has-SGB-audio-tail flag (v7, right after the
ROM-fingerprint): the same ROM legally runs as SGB (with the ~64 KB SPC700+S-DSP
tail) or DMG/CGB (without). On load a mismatch vs the target machine's model is a
clear `StateError::ModelMismatch` — never a silent tail-drop (SGB→DMG) nor an
opaque `Truncated` (DMG→SGB).

ROM bytes + the debugger fields (watch/prof/exc mask) are **not** serialized.
`App.path_purpose` routes the shared modal (Load ROM / Save state / Load state);
`Session::save_state_to`/`load_state_from` do the fs + logging. Verified by a
whole-machine round-trip oracle (save→fresh→load→run-both byte-identical across
frame/cycles/regs/memory/audio) + gbtr golden byte-identical.

## Rewind ring + reverse execution

`Session::rewind` is a bounded ring of `(cycle, save_state())` blobs
(`REWIND_INTERVAL_FRAMES` cadence, capped by `REWIND_MAX_STATES` /
`REWIND_MAX_BYTES`; cleared on reset / ROM change). It fills while playing forward
when System → "Rewind enabled" is on *or* the debugger is open. It backs both the
player rewind (held Backspace) and the debugger's reverse controls, via the
replay engine in `session/reverse.rs` — the cycle key lets it pick the nearest
checkpoint before a target, then `step()` forward to land the exact instruction
(`reverse_step`), the previous frame (`reverse_frame`, the frame-exact player
rewind), or the previous breakpoint (`reverse_to_breakpoint`). Full model +
ceilings: [`debugger.md`](debugger.md) § Reverse execution.

## Link submenu (`SubKind::Link`) — serial link cable over TCP

Rows (`main-sub-link.png`) grey by state via `link_items(active, listening)`:
Listen/Connect while idle, Disconnect while a socket is active, Cancel listen while
listening. Title bar shows the link status (`linked`/`listening :port`/`connecting
:port`). Connect opens a `host:port` modal (`PathPurpose::LinkConnect`,
bracket-stripped IPv6).

### Core: byte-level lockstep hook on `Serial` (golden-safe)

`GameBoy::link_connect`/`link_push_recv`/`link_take_send`/`link_slave_transfer`/
`link_stalled` — all inert when disconnected (`link_in`/`link_out` + the
`link_master_waiting` stall flag are transient, NOT serialized; every branch gated on
`link_connected` so a disconnected port is fingerprint byte-identical).

- **Why lockstep:** the old 1-byte-latency model corrupted Pokémon trades — the pump
  ran once per *frame* but a master clocks many transfers *within* a frame, each
  reading an empty `link_in` → shifting in `0xFF` → uniform garbage.
- **Master:** clocks 8 bits, ships its outgoing byte once, then **stalls** at
  completion (SC bit7 held, IF withheld, DIV clocking gated, `link_stalled()` true)
  until `link_push_recv` delivers the peer byte (SB←byte, clear bit7, raise serial
  IF). `run_frame`/`run_frame_until_breakpoint` return early on the stall so the
  frontend pumps mid-frame. A SC rewrite clears the stall; disconnecting mid-stall
  completes with the cable-open `0xFF`+IF so the CPU can't hang.
- **Slave** (external clock, SC bit7 set + bit0 clear) completes via
  `link_slave_transfer` when the frontend delivers the master's byte.

### Speed: sub-frame chunking

A trade was ~10 s (the slave pumped once per frame ⇒ ~60 B/s). The frontend runs a
connected frame in `LINK_CHUNK_CYCLES`=4096 slices (`GameBoy::run_slice`), pumping
the link between each — the slave exchanges ~17 bytes/frame while still advancing a
full slice of emulated cycles per byte.

- **Don't** yield to the pump the instant the slave arms — that starves it of
  cycles/byte → it answers `0xFE` garbage and a real Crystal trade livelocks. **Do**
  give it cycles/byte via chunking (a per-byte slave yield was tried and reverted).

### Frontend (`link.rs`)

`std::net` TCP, no Cargo dep; bgb 8-byte `Packet` framing, port 8765; a socket
**reader thread + a dedicated writer thread** that sends each queued packet
immediately via `out_rx.recv_timeout` — UI over mpsc, bounded inbound
`sync_channel`+`try_send`, unbounded outbound; bounded
`connect_timeout`/`set_write_timeout`/stop-flag so a `drop`-join can't hang on a
black-holed peer. Protocol:

- `pump` ships master bytes as SYNC1; routes an incoming SYNC1 to an armed slave
  (reply SYNC2) **or** a bounded frontend `pending_master` buffer (never the core
  `link_in` — no cross-contamination); SYNC2→`push_recv` only when a master is
  stalled (stale replies dropped).
- `drain_pending` dispatches buffered bytes once the port is ready (slave arms /
  master stalls).
- `run_one_frame` runs the frame in 4096-cycle slices pumping between each — the
  master stall breaks a slice early (`pump_blocking` waits ≤16 ms `poll_blocking`/
  `recv_timeout`), the slave runs full slices; no peer ⇒ a plain `run_frame`
  (golden-safe), debugger ⇒ a single breakpoint-aware frame; a dead peer times out →
  pacers yield the partial frame (never garbage); tears down on a peer disconnect.

**Scope:** slopgb↔slopgb linking with the bgb packet *format* + byte-level lockstep
(verified by 8- and 64-byte real-socket exchanges + a 16-byte no-socket loopback + a
full live Crystal trade — 2281 byte exchanges). Next step: timestamp-precise bgb-wire
lockstep (SYNC3 keep-alive + cycle-accurate completion) for real-bgb interop.
