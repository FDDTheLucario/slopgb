//! Building the backend: reading the plugin `.wasm` files (the two required
//! chips plus the optional SNES-PPU and MSU-1 plugins), attaching an MSU-1
//! track pack, and installing the resident clean-room firmware into both
//! chips' RAM before either runs an instruction.

use super::*;

impl SgbCoprocessor {
    /// Load the two coprocessor plugins from `dir` (`spc700.wasm` + `w65c816.wasm`)
    /// and build the backend at `output_rate` Hz. Errors (missing / bad wasm) are
    /// returned so the frontend can log them and leave the coprocessor slot
    /// empty.
    pub fn load(dir: &Path, output_rate: u32) -> Result<Self, String> {
        let spc_path = dir.join(SPC_WASM);
        let cpu_path = dir.join(CPU_WASM);
        let spc_bytes = fs::read(&spc_path)
            .map_err(|e| format!("cannot read SGB plugin '{}': {e}", spc_path.display()))?;
        let cpu_bytes = fs::read(&cpu_path)
            .map_err(|e| format!("cannot read SGB plugin '{}': {e}", cpu_path.display()))?;
        // The PPU plugin is optional: absent keeps the audio-only backend.
        let ppu_bytes = fs::read(dir.join(PPU_WASM)).ok();
        let mut me =
            Self::from_wasm_full(&spc_bytes, &cpu_bytes, ppu_bytes.as_deref(), output_rate)
                .map_err(|e| format!("cannot load SGB coprocessor plugins: {e}"))?;
        // The MSU-1 plugin is optional and loads from the *same* plugins dir as
        // the other coprocessor chips (its `.pcm` pack comes separately via
        // [`Self::set_msu_pack`]). Absent = no MSU-1.
        if let Ok(bytes) = fs::read(dir.join(MSU_WASM)) {
            if let Err(e) = me.attach_msu(&bytes) {
                eprintln!("slopgb: MSU-1 plugin '{MSU_WASM}' present but failed to load: {e}");
            }
        }
        Ok(me)
    }

    /// Load the MSU-1 streaming-audio plugin from its `.wasm` bytes (absent when
    /// `msu1.wasm` is not in the plugins dir). A game's SGB driver detects it at
    /// SNES `$2000-$2007` and drives it there; no track pack is advertised until
    /// [`Self::set_msu_pack`] finds `.pcm` files.
    pub fn attach_msu(&mut self, wasm: &[u8]) -> Result<(), LoadError> {
        let mut cop = LoadedCoprocessor::load(wasm)?;
        cop.reset()?;
        self.msu = Some(RefCell::new(cop));
        self.msu_wasm = Some(wasm.to_vec());
        self.msu_present = false;
        Ok(())
    }

    /// Point the loaded MSU-1 plugin at a pack directory: every `game-N.pcm`
    /// track (keyed by its trailing number, the index the game's `MSU_TRACK`
    /// write selects) and an optional `.msu` data ROM are streamed to the chip
    /// through the plugin's bulk-file channel. Presence (`S-MSU1`) is advertised
    /// only when ≥1 track loads, so a game finds the chip exactly when a pack is
    /// there. No-op without a loaded plugin.
    pub fn set_msu_pack(&mut self, dir: &Path) {
        let Some(msu) = &self.msu else { return };
        let mut tracks = 0usize;
        if let Ok(entries) = fs::read_dir(dir) {
            let mut cop = msu.borrow_mut();
            // ponytail: every track's full bytes are read into the plugin up
            // front (the `set_file` model). Fine for a handful of tracks; stream
            // a track on demand if a large pack's memory ever matters.
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if let Some(track) = track_number(&name) {
                    if let Ok(data) = fs::read(&path) {
                        cop.set_file(u32::from(track), data);
                        tracks += 1;
                    }
                } else if name.ends_with(".msu") {
                    if let Ok(data) = fs::read(&path) {
                        cop.set_file(MSU_DATA_FILE_KEY, data);
                    }
                }
            }
        }
        self.msu_pack_dir = Some(dir.to_path_buf());
        self.msu_present = tracks > 0;
        if tracks > 0 {
            eprintln!(
                "slopgb: MSU-1 pack '{}' loaded ({tracks} track(s)); served on the SGB SNES $2000-$2007 bus",
                dir.display(),
            );
        }
    }

    /// Build the backend from the two plugins' wasm bytes: instantiate, reset,
    /// install the resident clean-room firmware, and point both chips at their
    /// entry. The bytes are kept for [`Self::clone_box`].
    pub fn from_wasm(
        spc_bytes: &[u8],
        cpu_bytes: &[u8],
        output_rate: u32,
    ) -> Result<Self, LoadError> {
        Self::from_wasm_full(spc_bytes, cpu_bytes, None, output_rate)
    }

    /// [`Self::from_wasm`] plus the optional SNES-PPU plugin.
    pub fn from_wasm_full(
        spc_bytes: &[u8],
        cpu_bytes: &[u8],
        ppu_bytes: Option<&[u8]>,
        output_rate: u32,
    ) -> Result<Self, LoadError> {
        let mut spc = LoadedCoprocessor::load(spc_bytes)?;
        let mut cpu = LoadedCoprocessor::load(cpu_bytes)?;
        spc.reset()?;
        cpu.reset()?;
        let ppu = match ppu_bytes {
            Some(b) => {
                let mut p = LoadedCoprocessor::load(b)?;
                p.reset()?;
                Some(RefCell::new(p))
            }
            None => None,
        };
        let rate = output_rate.max(1);
        let mut me = SgbCoprocessor {
            spc: RefCell::new(spc),
            cpu: RefCell::new(cpu),
            spc_wasm: spc_bytes.to_vec(),
            cpu_wasm: cpu_bytes.to_vec(),
            spc_target: 0,
            cpu_target: 0,
            spc_pos: 0,
            cpu_pos: 0,
            char_write_row: 0,
            char_queue: VecDeque::new(),
            spc_acc: 0,
            cpu_acc: 0,
            pending_gb: 0,
            to_spc: [0; N_PORTS],
            src: VecDeque::new(),
            src_acc: 0.0,
            cur: (0, 0),
            samp_acc: 0.0,
            cycles_per_sample: f64::from(GB_CLOCK_HZ) / f64::from(rate),
            out_rate: rate,
            out: Vec::new(),
            max_out: rate as usize,
            msu: None,
            msu_wasm: None,
            msu_pack_dir: None,
            msu_present: false,
            msu_cycle: 0,
            msu_acc: 0,
            msu_src: VecDeque::new(),
            msu_src_acc: 0.0,
            msu_cur: (0, 0),
            msu_playing: false,
            poll_ctr: 0,
            sou_trn_sig: 0,
            data_trn_sig: 0,
            data_trn_seq_seen: None,
            jump: None,
            pending_packets: VecDeque::new(),
            pads_taken: false,
            pads_shadow: [0xFF; 4],
            feed_queue: VecDeque::new(),
            feed_hold: 0,
            gb_pos: 0,
            pending_trn: VecDeque::new(),
            trn_flip: false,
            nmitimen: 0,
            in_vblank: false,
            dma_regs: [[0; 7]; 8],
            wmadd: 0,
            input: (0x0F, 0x0F),
            joy_busy: false,
            ppu,
            ppu_wasm: ppu_bytes.map(<[u8]>::to_vec),
            ppu_row: 0,
            render_enabled: true,
            frame_ready: false,
            snes_live: false,
            frames_done: 0,
            last_inidisp: 0,
            nspc_resident: false,
            nspc_cmd: [0; 4],
            nspc_shadow: [0; 4],
            nspc_pending: false,
            dbg_sound: 0,
            dbg_soutrn: 0,
            dbg_datasnd: 0,
            dbg_last_sound: [0; 4],
            dbg_soutrn_dest: 0,
            dbg_soutrn_len: 0,
            dbg_soutrn_nonzero: 0,
            dbg_soutrn_head: [0; 16],
            dbg_pcm_peak: 0.0,
            song_start_spc: None,
            capture_at: None,
        };
        me.install_firmware()?;
        Ok(me)
    }

    /// Install the resident clean-room firmware into both chips: the 65C816 shim
    /// into SNES RAM (+ reset vector + entry PC), and the SPC700 driver + one-
    /// entry sample directory + a square BRR sample into APU RAM (+ entry PC). A
    /// failure aborts the load, so `from_wasm` reports it and the caller leaves
    /// the slot empty rather than running a chip with no firmware.
    fn install_firmware(&mut self) -> Result<(), LoadError> {
        {
            let cpu = self.cpu.get_mut();
            // Model the entire unshipped BIOS ROM as inert returns: an RTS
            // sled across the whole program area, so an uploaded program
            // JSR-ing any service entry slopgb has not (yet) pinned returns
            // harmlessly instead of executing zeroes. Specific resident
            // routines overwrite their spots below.
            cpu.write_ram(0x8000, &[0x60u8; 0x8000])?;
            // Keep the documented revision byte: $FFDB = 0 selects the first
            // entry of each BIOS service pair (sgb-arcade-takeover.md).
            cpu.write_ram(0xFFDB, &[0x00])?;
            cpu.write_ram(u32::from(SHIM_ORG), &SNES_SHIM)?;
            cpu.write_ram(
                u32::from(RESET_VEC),
                &[SHIM_ORG as u8, (SHIM_ORG >> 8) as u8],
            )?;
            cpu.set_pc(u32::from(SHIM_ORG))?;
            // Resident BIOS service entries (JMP thunks; the entries sit 3
            // bytes apart, too tight for inline bodies). Opcodes per the WDC
            // datasheet: 4C = JMP abs, AD = LDA abs, F0 = BEQ, 20 = JSR,
            // 60 = RTS.
            for entry in BIOS_MAIN_ENTRIES {
                cpu.write_ram(
                    entry,
                    &[0x4C, BIOS_MAIN_BODY as u8, (BIOS_MAIN_BODY >> 8) as u8],
                )?;
            }
            for entry in BIOS_AUX_ENTRIES {
                cpu.write_ram(
                    entry,
                    &[0x4C, BIOS_AUX_BODY as u8, (BIOS_AUX_BODY >> 8) as u8],
                )?;
            }
            // Main body (see BIOS_MAIN_BODY): consume the delivery mailbox
            // with long addressing (caller's DBR unknown), then the guarded
            // hook call. PLP precedes the JSR so the hook sees exactly the
            // caller's two return addresses (its PLA PLA / RTS fixup).
            let hook_lo = BIOS_HOOK_SLOT as u8;
            let hook_hi = (BIOS_HOOK_SLOT >> 8) as u8;
            let mb = |off: u32| {
                let a = BIOS_DELIVERY + off;
                [a as u8, (a >> 8) as u8, (a >> 16) as u8]
            };
            let [d0, d1, d2] = mb(0);
            let [c0, c1, c2] = mb(0x10);
            let [p0, p1, p2] = mb(0x11);
            let [q0, q1, q2] = mb(0x12);
            let [f0, f1, f2] = mb(0x16);
            cpu.write_ram(
                BIOS_MAIN_BODY,
                &[
                    0x08, // BE80 PHP
                    0xE2,
                    0x30, // BE81 SEP #$30
                    0xAF,
                    f0,
                    f1,
                    f2, // BE83 LDA long flag
                    // No delivery pending: skip the publish, still call the
                    // hook — the BIOS invokes it every service loop, and
                    // the pilot's hook re-ACKs on stale $02C2 (its own
                    // pacing protocol with the GB).
                    0xF0,
                    0x2B, // BE87 BEQ hookcall (BEB4)
                    0xA2,
                    0x0F, // BE89 LDX #$0F
                    0xBF,
                    d0,
                    d1,
                    d2, // BE8B loop: LDA long packet,X
                    0x9F,
                    BIOS_PKT_BUF as u8,
                    (BIOS_PKT_BUF >> 8) as u8,
                    0x00, // BE8F STA long $0600,X
                    0xCA, // BE93 DEX
                    0x10,
                    0xF5, // BE94 BPL loop
                    0xAF,
                    c0,
                    c1,
                    c2, // BE96 LDA long cmd
                    0x8F,
                    BIOS_LAST_CMD as u8,
                    (BIOS_LAST_CMD >> 8) as u8,
                    0x00, // BE9A STA long $02C2
                    0xAF,
                    p0,
                    p1,
                    p2, // BE9E LDA long ptr lo
                    0x8F,
                    BIOS_TRN_PTR as u8,
                    (BIOS_TRN_PTR >> 8) as u8,
                    0x00, // BEA2 STA long $0284
                    0xAF,
                    q0,
                    q1,
                    q2, // BEA6 LDA long ptr hi
                    0x8F,
                    (BIOS_TRN_PTR + 1) as u8,
                    ((BIOS_TRN_PTR + 1) >> 8) as u8,
                    0x00, // BEAA
                    0xA9,
                    0x00, // BEAE LDA #$00
                    0x8F,
                    f0,
                    f1,
                    f2, // BEB0 STA long flag (consumed)
                    0xAF,
                    hook_lo,
                    hook_hi,
                    0x00, // BEB4 LDA long $0800 (hook?)
                    0xF0,
                    0x05, // BEB8 BEQ exit (BEBF: the PLP)
                    0x28, // BEBA PLP
                    0x20,
                    hook_lo,
                    hook_hi, // BEBB JSR $0800
                    0x60,    // BEBE RTS
                    0x28,    // BEBF exit: PLP
                    0x60,    // BEC0 RTS
                ],
            )?;
            // Aux body (see BIOS_AUX_BODY): PHP / SEP #$20 /
            // wait: LDA $4210 / BPL wait / PLP / RTS — the $4210 reads ride
            // the host-fed RDNMI shadow (set at every vblank edge,
            // read-clear guest-side), so the wait spans to the next edge.
            cpu.write_ram(
                BIOS_AUX_BODY,
                &[
                    0x08, // PHP (caller's register widths preserved)
                    0xE2, 0x20, // SEP #$20
                    0xAD, 0x10, 0x42, // wait: LDA $4210 (RDNMI)
                    0x10, 0xFB, // BPL wait
                    0x28, // PLP
                    0x60, // RTS
                ],
            )?;
            // The resident NMI handler + both CPU-mode NMI vectors.
            cpu.write_ram(
                NMI_HANDLER,
                &[
                    0x48, // PHA (interrupted width)
                    0x08, // PHP
                    0xE2,
                    0x20, // SEP #$20 (8-bit A for the guard)
                    0xAF,
                    NMI_RAM_VEC,
                    0x00,
                    0x00, // LDA $0000BB (long)
                    0x0F,
                    NMI_RAM_VEC + 1,
                    0x00,
                    0x00, // ORA $0000BC
                    0x0F,
                    NMI_RAM_VEC + 2,
                    0x00,
                    0x00, // ORA $0000BD
                    0xF0,
                    0x05, // BEQ +5 -> the empty-vector PLP/PLA/RTI
                    0x28, // PLP (width back to the interrupted M)
                    0x68, // PLA (original A restored for the hook)
                    0xDC,
                    NMI_RAM_VEC,
                    0x00, // JML [$00BB]
                    0x28, // PLP
                    0x68, // PLA
                    0x40, // RTI
                ],
            )?;
            let nmi_vec = [NMI_HANDLER as u8, (NMI_HANDLER >> 8) as u8];
            cpu.write_ram(0xFFEA, &nmi_vec)?; // native NMI vector
            cpu.write_ram(0xFFFA, &nmi_vec)?; // emulation NMI vector
            // Break/interrupt vectors -> the resident RTI (see RTI_STUB).
            cpu.write_ram(RTI_STUB, &[0x40])?;
            let rti = [RTI_STUB as u8, (RTI_STUB >> 8) as u8];
            cpu.write_ram(0xFFE4, &rti)?; // native COP
            cpu.write_ram(0xFFE6, &rti)?; // native BRK
            cpu.write_ram(0xFFEE, &rti)?; // native IRQ
            cpu.write_ram(0xFFF4, &rti)?; // emulation COP
            cpu.write_ram(0xFFFE, &rti)?; // emulation IRQ/BRK
            // RDNMI reads the CPU version bits from power-on (fullsnes 4210h).
            cpu.write_ram(HW_SHADOW + SH_RDNMI, &[0x02])?;
        }
        {
            let (prog, dir, brr) = spc_firmware();
            let spc = self.spc.get_mut();
            spc.write_ram(u32::from(SPC_PROG_ORG), &prog)?;
            spc.write_ram(u32::from(SPC_DIR_ORG), &dir)?;
            spc.write_ram(u32::from(SPC_BRR_ORG), &brr)?;
            // No set_pc: the SPC700 boots its own IPL ROM (the chip
            // ships the documented 64-byte boot loader at $FFC0). The
            // pilot's loader speaks exactly the standard uploader protocol
            // ($2142/43 dest, $2141 nonzero cmd = its length bytes ORed,
            // kick chain, terminator with cmd 0 + entry dest) — and its
            // own entry-jumped APU driver re-announces $AA/$BB to serve
            // the next upload round. The square driver above is entered
            // host-side on a SOUND command instead (see apply_sound).
        }
        Ok(())
    }
}
