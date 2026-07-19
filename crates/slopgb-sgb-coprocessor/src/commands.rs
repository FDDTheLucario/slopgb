//! SGB command routing: the teed raw packets, SOUND/DATA_SND/DATA_TRN/JUMP
//! commands, and the SOU_TRN driver upload — everything `poll` drains from
//! the core each step and lands on the two chips.

use super::*;

impl SgbCoprocessor {
    pub(crate) fn poll(&mut self, cmds: &mut dyn SgbCommandSource) {
        // Raw packet tee → the ICD2 mailbox deposit queue (bounded like the
        // core-side tee; the flush pump deposits one per guest consume).
        while let Some(p) = cmds.take_packet() {
            // DATA_TRN ($10) names its SNES-WRAM dest in the header (Pan
            // Docs "SGB Command $10": lo, hi, bank). The core captures the
            // 4 KB screen payload one frame after the command (the SNES-side
            // capture window), so it is not yet visible here — queue the
            // packet; the per-poll payload edge below pairs it. A new $10
            // finding one still pending means the previous payload was
            // byte-identical to its predecessor (no signature edge) — the
            // current capture holds those same bytes, so pairing is exact.
            if p[0] >> 3 == 0x10 {
                if !self.pending_trn.is_empty() {
                    if let Some(data) = cmds.data_trn_data() {
                        self.data_trn_sig = checksum(data);
                        let data = data.to_vec();
                        self.apply_pending_trn(&data);
                    }
                }
                while self.pending_trn.len() >= PACKET_QUEUE_CAP {
                    self.pending_trn.pop_front();
                }
                self.pending_trn.push_back(p);
            } else if p[0] >> 3 == 0x12 && p[4..7] != [0, 0, 0] {
                // JUMP ($12) bytes 4-6: the SNES NMI handler address — "the
                // NMI handler remains unchanged if all bytes 4-6 are zero"
                // (Pan Docs "SGB Command 12h"). The BIOS lands it in the RAM
                // vector at $00BB-$00BD (fullsnes: JUMP clobbers exactly
                // those bytes), where the resident NMI handler dispatches.
                let _ = self
                    .cpu
                    .get_mut()
                    .write_ram(u32::from(NMI_RAM_VEC), &p[4..7]);
                self.deliver_packet(&p, None);
            } else {
                self.deliver_packet(&p, None);
            }
            while self.pending_packets.len() >= PACKET_QUEUE_CAP {
                self.pending_packets.pop_front();
            }
            self.pending_packets.push_back(p);
        }
        // ICD2 character rows: collect the GB screen's 8-line bands here;
        // the flush delivers them into the plugin's four rotating `$7800`
        // buffers ONE PER FLUSH — the guest CPU runs a flush behind the GB,
        // so delivering a whole frame's bands at once skips `$6000`
        // write-row values faster than the SNES side can poll them, and
        // the missed bands land stale (Space Invaders' playfield assembled
        // its invader row into the wrong vertical bands).
        while let Some(row) = cmds.take_char_row() {
            if self.char_queue.len() >= CHAR_QUEUE_CAP {
                self.char_queue.pop_front();
            }
            self.char_queue.push_back(row);
        }
        // SOUND ($08): a play request. Deposit the effect id + a trigger in the
        // CPU's mailbox; the 65C816 shim forwards them to the SPC700 driver.
        while let Some(s) = cmds.take_sound_event() {
            self.dbg_sound = self.dbg_sound.wrapping_add(1);
            self.dbg_last_sound = [s.effect_a, s.effect_b, s.attenuation, s.effect_bank];
            self.apply_sound(s);
        }
        // DATA_SND ($0F): a write to SNES work RAM — no longer a no-op. fullsnes:
        // the packet is `dest_lo, dest_hi, len, data…`.
        while let Some(pkt) = cmds.take_data_snd() {
            self.dbg_datasnd = self.dbg_datasnd.wrapping_add(1);
            self.apply_data_snd(&pkt);
        }

        // The DATA_TRN payload check runs every poll (never throttled): the
        // capture lands exactly one frame after its packet, and pairing must
        // beat both the next packet and the next capture. The capture counter
        // is the cheap pre-filter — without it the 4 KB checksum ran once per
        // GB instruction and dominated the whole frame budget after the first
        // DATA_TRN (`data_trn_data` stays `Some` forever). A source without
        // the counter (`None`) hashes every poll as before.
        let seq = cmds.data_trn_seq();
        if seq.is_none() || seq != self.data_trn_seq_seen {
            self.data_trn_seq_seen = seq;
            if let Some(data) = cmds.data_trn_data() {
                let sig = checksum(data);
                if sig != self.data_trn_sig {
                    self.data_trn_sig = sig;
                    let data = data.to_vec();
                    self.apply_pending_trn(&data);
                }
            }
        }
        self.poll_ctr = self.poll_ctr.wrapping_add(1);
        if self.poll_ctr & 0x3F != 0 {
            return;
        }
        if let Some(data) = cmds.sou_trn_data() {
            let sig = checksum(data);
            if sig != self.sou_trn_sig {
                self.sou_trn_sig = sig;
                self.dbg_soutrn = self.dbg_soutrn.wrapping_add(1);
                self.upload_transfer(data);
            }
        }
        if let Some(flags) = cmds.flags() {
            self.apply_flags(flags);
        }
        if self.nspc_resident {
            self.nspc_flush();
        }
    }

    /// Deliver a queued N-SPC command to the resident engine with the SGB BIOS's
    /// echo-ack handshake (ported from program.rom $00:BAC4). The engine echoes
    /// the last command word back on its output ports; only once the echo matches
    /// the shadow do we send the next command — otherwise we resend the shadow
    /// (the BIOS `$BAFB` retry). Ports 0/1 carry the 16-bit command, 2/3 the data.
    /// Runs at the throttled poll cadence so the wasm port crossings stay bounded.
    fn nspc_flush(&mut self) {
        let spc = self.spc.get_mut();
        let echo = [
            spc.port_read(0).unwrap_or(0),
            spc.port_read(1).unwrap_or(0),
            spc.port_read(2).unwrap_or(0),
            spc.port_read(3).unwrap_or(0),
        ];
        if echo == self.nspc_shadow {
            // Engine acked the last word. Send the next if one is queued.
            if self.nspc_pending {
                for (p, &v) in self.nspc_cmd.iter().enumerate() {
                    let _ = spc.port_write(p as u8, v);
                }
                self.nspc_shadow = self.nspc_cmd;
                self.nspc_pending = false;
            }
        } else {
            // Not yet echoed: resend the shadow until the engine catches up.
            for (p, &v) in self.nspc_shadow.iter().enumerate() {
                let _ = spc.port_write(p as u8, v);
            }
        }
    }

    /// Complete the oldest pending DATA_TRN with its just-arrived 4 KB
    /// payload: stage it behind the BIOS-runtime pointer (uploaded code
    /// performs its own copy — the pilot does), copy it to the packet's own
    /// 24-bit dest (the copy the SGB BIOS performs on real hardware), and
    /// publish the packet to the BIOS-runtime variables. Without a pending
    /// packet there is nowhere honest to put the payload, so only the
    /// staging updates.
    fn apply_pending_trn(&mut self, data: &[u8]) {
        let staging = BIOS_TRN_STAGING[usize::from(self.trn_flip)];
        self.trn_flip = !self.trn_flip;
        let _ = self.cpu.get_mut().write_ram(staging, data);
        if let Some(p) = self.pending_trn.pop_front() {
            let dest = u32::from(p[1]) | u32::from(p[2]) << 8 | u32::from(p[3]) << 16;
            let _ = self.cpu.get_mut().write_ram(dest, data);
            self.deliver_packet(&p, Some(staging));
        }
    }

    /// Hand a packet to the resident BIOS through the delivery mailbox: the
    /// main-service body publishes it to the BIOS-runtime variables and
    /// then calls the hook — all inside one guest-side service call, so a
    /// hook mid-flight (e.g. across its aux vblank wait) never observes a
    /// half-delivered update (the real BIOS is single-threaded). `staging`
    /// carries the DATA_TRN payload buffer the pointer should publish;
    /// other packets leave the pointer bytes as-is (the body republishes
    /// the previous value, which is what the variables already hold).
    fn deliver_packet(&mut self, p: &[u8; 16], staging: Option<u32>) {
        let cpu = self.cpu.get_mut();
        let _ = cpu.write_ram(BIOS_DELIVERY, p);
        let _ = cpu.write_ram(BIOS_DELIVERY + 0x10, &[p[0] >> 3]);
        if let Some(st) = staging {
            let _ = cpu.write_ram(BIOS_DELIVERY + 0x11, &[st as u8, (st >> 8) as u8]);
        }
        let _ = cpu.write_ram(BIOS_DELIVERY + 0x16, &[1]);
    }

    /// SOUND ($08): mailbox `note = effect_a`, `trigger = 1` (or the effect-on
    /// flags byte if non-zero), so the shim wakes the SPC700 driver.
    fn apply_sound(&mut self, s: SgbSound) {
        if self.nspc_resident {
            // Real N-SPC engine resident: queue the command word exactly as the
            // SGB BIOS SOUND handler forms it (program.rom $00:C554) — command =
            // (Music Score Code | effect_a<<8), data = (effect_b | attr<<8) — and
            // let nspc_flush deliver it with the engine's echo-ack handshake.
            // `effect_bank` carries the Music Score Code (Pan Docs "SOUND" byte 4;
            // sgb-music-test `PlaySPC: SOUND 0,0,0,1`).
            self.nspc_cmd = [s.effect_bank, s.effect_a, s.effect_b, s.attenuation];
            self.nspc_pending = true;
            return;
        }
        let trig = if s.attenuation != 0 { s.attenuation } else { 1 };
        let _ = self
            .cpu
            .get_mut()
            .write_ram(u32::from(MB_NOTE), &[s.effect_a, trig]);
        // Enter the resident square driver (the SPC otherwise idles in its
        // IPL boot ROM waiting for an upload) — the host-side stand-in for
        // the sound engine the real BIOS uploads at power-on. Never while a
        // SOU_TRN game driver is installed: that driver owns the chip and
        // handles sound itself.
        if self.sou_trn_sig == 0 {
            let _ = self.spc.get_mut().set_pc(u32::from(SPC_PROG_ORG));
        }
    }

    /// DATA_SND ($0F): copy the packet's inline data into SNES work RAM at
    /// its 24-bit target. Pan Docs "SGB Command 0Fh — DATA_SND": dest low,
    /// dest high, dest bank, count (1-11), data bytes.
    fn apply_data_snd(&mut self, pkt: &[u8]) {
        if pkt.len() < 5 {
            return;
        }
        let dest = u32::from(pkt[0]) | u32::from(pkt[1]) << 8 | u32::from(pkt[2]) << 16;
        let len = usize::from(pkt[3]).min(11);
        let data: Vec<u8> = pkt[4..].iter().take(len).copied().collect();
        let _ = self.cpu.get_mut().write_ram(dest, &data);
    }

    /// JUMP ($12): redirect the 65C816 to the SNES program target — no longer a
    /// no-op now that a real SNES CPU is present.
    fn apply_flags(&mut self, flags: SgbFlags) {
        if let Some(target) = flags.jump {
            if self.jump != Some(target) {
                self.jump = Some(target);
                // The BIOS hands a JUMP target control in native mode (the
                // pilot's dispatcher REP #$30 pins it): CLC / XCE / JML.
                let cpu = self.cpu.get_mut();
                let tramp = [
                    0x18, // CLC
                    0xFB, // XCE -> native
                    0x5C, // JML target
                    target as u8,
                    (target >> 8) as u8,
                    (target >> 16) as u8,
                ];
                let _ = cpu.write_ram(JUMP_TRAMP, &tramp);
                let _ = cpu.set_pc(JUMP_TRAMP);
            }
        }
    }

    /// Copy a SOU_TRN self-describing `(dest, len, data…)` transfer block into
    /// APU RAM (fullsnes: SGB sound transfers begin with a destination/length
    /// pair) and point the SPC700 at the first load address. Same shape as the
    /// built-in `SgbApu` uploader, so a `SOU_TRN` game driver runs identically.
    fn upload_transfer(&mut self, data: &[u8]) {
        let resident = self.nspc_resident;
        if resident {
            let dst = u16::from_le_bytes([*data.get(2).unwrap_or(&0), *data.get(3).unwrap_or(&0)]);
            self.dbg_soutrn_dest = dst;
            self.dbg_soutrn_len = data.len() as u32;
            self.dbg_soutrn_nonzero = data.iter().filter(|&&b| b != 0).count() as u32;
            for (i, slot) in self.dbg_soutrn_head.iter_mut().enumerate() {
                *slot = *data.get(i).unwrap_or(&0);
            }
        }
        let spc = self.spc.get_mut();
        let mut off = 0usize;
        let mut entry = None;
        while off + 4 <= data.len() {
            // SBN / SNES APU block header is `[u16 len, u16 dest]` (SBN2SPC;
            // the SGB loader at program.rom $00:AC6C reads len into $9C/$9D and
            // dest into $9E/$9F) — length first, destination second.
            let len = usize::from(u16::from_le_bytes([data[off], data[off + 1]]));
            let dest = u16::from_le_bytes([data[off + 2], data[off + 3]]);
            off += 4;
            if len == 0 || off + len > data.len() {
                break;
            }
            let _ = spc.write_ram(u32::from(dest), &data[off..off + len]);
            entry.get_or_insert(dest);
            off += len;
        }
        // With the real N-SPC engine resident, SOU_TRN carries only song data
        // (loaded to $2B00) for the already-running engine — re-entering its PC
        // would restart the chip and drop the driver state. Only a self-uploaded
        // driver (clean-room path) owns the entry point.
        if let Some(e) = entry {
            if !resident {
                let _ = spc.set_pc(u32::from(e));
            }
        }
    }
}

/// A cheap order-sensitive checksum for edge-detecting transfer uploads (FNV-1a).
fn checksum(data: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}
