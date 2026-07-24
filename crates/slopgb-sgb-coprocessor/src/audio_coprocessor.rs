//! `AudioCoprocessor` impl for [`SgbCoprocessor`] — the trait `GameBoy` drives
//! the SGB SNES-side machine through (clock/poll/mix/status), plus the
//! coprocessor's self-describing manifest and the menu-row export it declares.
//!
//! "Export SPC" is declared HERE, by the mediator, not by `spc700.wasm`: the
//! from-start snapshot ([`SgbCoprocessor::song_start_spc`]) is captured by this
//! native mediator watching the resident engine's play command, while the
//! plugin's `dump_spc` is live-only (see [`AudioCoprocessor::export_spc_live`]).
//! Declaring the row on the plugin would silently downgrade the exported file
//! to a mid-song dump.

use super::*;

impl SgbCoprocessor {
    /// This mediator's self-describing manifest: identity plus the one menu
    /// row it contributes. `export_spc` (not `dump_spc`) names the export so
    /// it stays distinct from the SPC700 plugin's live dump.
    pub const MANIFEST: &'static str = concat!(
        "id\tsgb\n",
        "name\tSGB coprocessor\n",
        "menu\tExport SPC\texport_spc\tspc",
    );
}

impl AudioCoprocessor for SgbCoprocessor {
    fn clock(&mut self, gb_cycles: u64) {
        SgbCoprocessor::clock(self, gb_cycles);
    }
    fn poll(&mut self, cmds: &mut dyn SgbCommandSource) {
        SgbCoprocessor::poll(self, cmds);
    }
    fn joypad_feed(&mut self) -> Option<[u8; 4]> {
        // Queued latch writes first, each dwelling long enough for the GB's
        // polls to see it (ordered protocol sequences — an ACK handshake, a
        // one-shot phase trigger). With the queue idle, forward the local
        // matrix — the resident BIOS's continuous pad pass-through, the
        // only way the player's buttons reach a taken-over GB (ICD2 latch
        // encoding: buttons high nibble, d-pad low, active low).
        if self.feed_hold > 0 {
            self.feed_hold -= 1;
            return Some(self.pads_shadow);
        }
        if let Some(pads) = self.feed_queue.pop_front() {
            self.pads_shadow = pads;
            self.feed_hold = FEED_DWELL_STEPS;
            return Some(pads);
        }
        if self.pads_taken {
            let (dpad, buttons) = self.input;
            Some([buttons << 4 | dpad & 0x0F, 0xFF, 0xFF, 0xFF])
        } else {
            None
        }
    }
    fn set_input(&mut self, dpad: u8, buttons: u8) {
        self.input = (dpad, buttons);
    }
    fn set_render_enabled(&mut self, on: bool) {
        self.render_enabled = on;
    }
    fn take_frame(&mut self) -> Option<Vec<u16>> {
        self.take_snes_frame()
    }
    fn mix_into(&mut self, out: &mut [(f32, f32)]) {
        SgbCoprocessor::mix_into(self, out);
    }
    fn set_output_rate(&mut self, hz: u32) {
        SgbCoprocessor::set_output_rate(self, hz);
    }
    fn load_bios(&mut self, _bios: &[u8]) {
        // The resident clean-room firmware is fixed; there is no user BIOS image
        // to install (and slopgb never reads the copyrighted SGB system ROM).
    }
    fn write_state(&self, w: &mut Writer) {
        SgbCoprocessor::write_state(self, w);
    }
    fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), StateError> {
        SgbCoprocessor::read_state(self, r)
    }
    fn clone_box(&self) -> Box<dyn AudioCoprocessor> {
        // Re-instantiating the already-validated plugin wasm can only fail on an
        // allocation error (which aborts anyway), so this is near-unreachable —
        // but a save-state clone must never panic the emulator. Degrade to a
        // silent inert coprocessor and log, rather than `.expect`.
        match self.deep_clone() {
            Ok(fresh) => Box::new(fresh),
            Err(e) => {
                eprintln!(
                    "slopgb: SGB coprocessor clone failed ({e}); audio inert for this snapshot"
                );
                Box::new(InertCoprocessor)
            }
        }
    }

    fn export_spc(&self) -> Option<Vec<u8>> {
        // The from-start snapshot the resident engine's last play command
        // produced (assembled by the SPC700 plugin from its ARAM + registers +
        // DSP). `None` until a recognized song has started.
        self.song_start_spc.clone()
    }

    fn can_export_spc(&self) -> bool {
        self.song_start_spc.is_some()
    }

    fn export_spc_live(&self) -> Option<Vec<u8>> {
        // The SPC700 plugin assembles a `.spc` from its current ARAM + registers
        // + DSP — the live state, whatever is playing now.
        match self.spc.borrow_mut().dump_spc() {
            Ok(spc) if !spc.is_empty() => Some(spc),
            _ => None,
        }
    }

    fn manifest(&self) -> &'static str {
        Self::MANIFEST
    }

    fn export_ready(&self, name: &str) -> bool {
        match name {
            "export_spc" => self.can_export_spc(),
            _ => false,
        }
    }

    fn call_export(&self, name: &str) -> Option<Vec<u8>> {
        match name {
            "export_spc" => self.export_spc(),
            _ => None,
        }
    }

    fn debug_status(&self) -> String {
        // The run-cycle targets grow only while the host clocks the chips, so a
        // zero here means the coprocessor loaded but was never driven (the
        // machine isn't in SGB mode, or the GB is sending nothing) — the exact
        // "SNES side isn't running" case. Non-zero = the chips are executing.
        let running = self.cpu_target > 0 || self.spc_target > 0;
        let ppu = match &self.ppu {
            Some(_) => format!(
                "SNES PPU plugin loaded: {} frames rendered, last INIDISP ${:02X}",
                self.frames_done, self.last_inidisp
            ),
            None => "no SNES PPU plugin (audio-only)".into(),
        };
        let driver = if self.nspc_resident {
            // Live SPC output ports (the engine's echo) + the ARAM entry byte, to
            // diagnose the N-SPC handshake. borrow_mut from &self via the RefCell.
            let mut spc = self.spc.borrow_mut();
            let song = spc
                .read_ram(u32::from(self.dbg_soutrn_dest), 8)
                .unwrap_or_default();
            // The default ROM engine has its own internals; only the clean-room
            // engine (SLOPGB_NSPC_CLEANROOM) has the engine.asm variable layout
            // this decodes ($12 songlp, $14 tempo, $16 tickacc, $19 state, $1B
            // activemask, $48.. per-channel tdurrem).
            let eng = if std::env::var_os("SLOPGB_NSPC_CLEANROOM").is_some() {
                let v = spc.read_ram(0x12, 10).unwrap_or_default();
                let durrem = spc.read_ram(0x48, 8).unwrap_or_default();
                let g = |i: usize| v.get(i).copied().unwrap_or(0);
                let songlp = u16::from(g(0)) | u16::from(g(1)) << 8;
                format!(
                    "; ENG(clean-room) songlp ${songlp:04X} tempo ${:02X} tickacc \
                     ${:02X} state {} activemask ${:02X} tdurrem {durrem:02X?}",
                    g(2),
                    g(4),
                    g(7),
                    g(9),
                )
            } else {
                "; ROM engine (default; SLOPGB_NSPC_CLEANROOM for the clean-room engine)"
                    .to_string()
            };
            format!(
                "N-SPC resident (--sgb-bios): SOUND x{} last {:02X?}, SOU_TRN x{} \
                 (dest ${:04X} len {}), DATA_SND x{}; cmd {:02X?}; DSP peak {}; \
                 song@${:04X} {:02X?}{eng}",
                self.dbg_sound,
                self.dbg_last_sound,
                self.dbg_soutrn,
                self.dbg_soutrn_dest,
                self.dbg_soutrn_len,
                self.dbg_datasnd,
                self.nspc_cmd,
                self.dbg_pcm_peak,
                self.dbg_soutrn_dest,
                song,
            )
        } else {
            "clean-room firmware".to_string()
        };
        format!(
            "wasm SGB coprocessor: SPC700 + 65C816 plugins loaded; {} \
             (65C816 ran to cyc {}, SPC700 to cyc {}); last GB->SPC ports {:02X?}; {}; {}",
            if running {
                "RUNNING"
            } else {
                "NOT yet clocked"
            },
            self.cpu_target,
            self.spc_target,
            self.to_spc,
            driver,
            ppu,
        )
    }
}
