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
            // Docs "SGB Command $10": lo, hi, bank); the 4 KB payload rides
            // the next frame's screen capture — remember where to land it.
            if p[0] >> 3 == 0x10 {
                self.data_trn_dest =
                    Some(u32::from(p[1]) | u32::from(p[2]) << 8 | u32::from(p[3]) << 16);
                // DATA_TRN completes when its screen payload lands, one
                // frame later — defer the BIOS-runtime variable update until
                // then (see `pending_trn_pkt`).
                self.pending_trn_pkt = Some(p);
            } else if p[0] >> 3 == 0x12 && p[4..7] != [0, 0, 0] {
                // JUMP ($12) bytes 4-6: the SNES NMI handler address — "the
                // NMI handler remains unchanged if all bytes 4-6 are zero"
                // (Pan Docs "SGB Command 12h"). The BIOS lands it in the RAM
                // vector at $00BB-$00BD (fullsnes: JUMP clobbers exactly
                // those bytes), where the resident NMI handler dispatches.
                let cpu = self.cpu.get_mut();
                let _ = cpu.write_ram(u32::from(NMI_RAM_VEC), &p[4..7]);
                let _ = cpu.write_ram(BIOS_PKT_BUF, &p);
                let _ = cpu.write_ram(BIOS_LAST_CMD, &[p[0] >> 3]);
            } else {
                // The BIOS-runtime variables the uploaded code polls: the
                // packet bytes and the command number (see the BIOS_* consts).
                let cpu = self.cpu.get_mut();
                let _ = cpu.write_ram(BIOS_PKT_BUF, &p);
                let _ = cpu.write_ram(BIOS_LAST_CMD, &[p[0] >> 3]);
            }
            while self.pending_packets.len() >= PACKET_QUEUE_CAP {
                self.pending_packets.pop_front();
            }
            self.pending_packets.push_back(p);
        }
        // SOUND ($08): a play request. Deposit the effect id + a trigger in the
        // CPU's mailbox; the 65C816 shim forwards them to the SPC700 driver.
        while let Some(s) = cmds.take_sound_event() {
            self.apply_sound(s);
        }
        // DATA_SND ($0F): a write to SNES work RAM — no longer a no-op. fullsnes:
        // the packet is `dest_lo, dest_hi, len, data…`.
        while let Some(pkt) = cmds.take_data_snd() {
            self.apply_data_snd(&pkt);
        }

        self.poll_ctr = self.poll_ctr.wrapping_add(1);
        if self.poll_ctr & 0x3F != 0 {
            return;
        }
        if let Some(data) = cmds.sou_trn_data() {
            let sig = checksum(data);
            if sig != self.sou_trn_sig {
                self.sou_trn_sig = sig;
                self.upload_transfer(data);
            }
        }
        if let Some(data) = cmds.data_trn_data() {
            let sig = checksum(data);
            if sig != self.data_trn_sig {
                self.data_trn_sig = sig;
                // DATA_TRN is 4 KB into SNES WRAM at the packet's dest — the
                // copy the SGB BIOS performs on real hardware. Without a
                // teed dest packet there is nowhere honest to put it, so it
                // is dropped rather than guessed. The payload is also staged
                // behind the BIOS-runtime pointer for uploaded code that
                // performs the dest copy itself (the pilot does).
                let cpu = self.cpu.get_mut();
                let _ = cpu.write_ram(BIOS_TRN_STAGING, data);
                let _ = cpu.write_ram(
                    BIOS_TRN_PTR,
                    &[BIOS_TRN_STAGING as u8, (BIOS_TRN_STAGING >> 8) as u8],
                );
                if let Some(dest) = self.data_trn_dest {
                    let _ = cpu.write_ram(dest, data);
                }
                // The transfer is now complete: publish the deferred packet
                // to the BIOS-runtime variables the uploaded code polls.
                if let Some(p) = self.pending_trn_pkt.take() {
                    let _ = cpu.write_ram(BIOS_PKT_BUF, &p);
                    let _ = cpu.write_ram(BIOS_LAST_CMD, &[p[0] >> 3]);
                }
            }
        }
        if let Some(flags) = cmds.flags() {
            self.apply_flags(flags);
        }
    }

    /// SOUND ($08): mailbox `note = effect_a`, `trigger = 1` (or the effect-on
    /// flags byte if non-zero), so the shim wakes the SPC700 driver.
    fn apply_sound(&mut self, s: SgbSound) {
        let trig = if s.attenuation != 0 { s.attenuation } else { 1 };
        let _ = self
            .cpu
            .get_mut()
            .write_ram(u32::from(MB_NOTE), &[s.effect_a, trig]);
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
        let spc = self.spc.get_mut();
        let mut off = 0usize;
        let mut entry = None;
        while off + 4 <= data.len() {
            let dest = u16::from_le_bytes([data[off], data[off + 1]]);
            let len = usize::from(u16::from_le_bytes([data[off + 2], data[off + 3]]));
            off += 4;
            if len == 0 || off + len > data.len() {
                break;
            }
            let _ = spc.write_ram(u32::from(dest), &data[off..off + len]);
            entry.get_or_insert(dest);
            off += len;
        }
        if let Some(e) = entry {
            let _ = spc.set_pc(u32::from(e));
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
