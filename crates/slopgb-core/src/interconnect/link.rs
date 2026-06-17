//! Serial link-cable plumbing on [`Interconnect`]: thin delegators to the
//! [`crate::serial::Serial`] byte-exchange hook (frontend TCP peer). Every one
//! is inert when no peer is attached, so the link is golden-safe — the
//! fingerprint stays byte-identical on every path that never connects. Serial
//! work package.

use super::*;

impl Interconnect {
    /// Attach/detach a serial link peer (frontend path only).
    pub(crate) fn link_set_connected(&mut self, on: bool) {
        self.serial.set_link_connected(on);
    }

    /// Whether a link peer is attached.
    pub(crate) fn link_connected(&self) -> bool {
        self.serial.link_connected()
    }

    /// Provide the peer byte the next master transfer shifts in.
    pub(crate) fn link_push_recv(&mut self, byte: u8) {
        self.serial.push_link_in(byte);
    }

    /// Drain the byte a completed master transfer shifted out, for the peer.
    pub(crate) fn link_take_send(&mut self) -> Option<u8> {
        self.serial.take_link_send()
    }

    /// Complete a pending external-clock (slave) transfer with the peer's
    /// byte, folding the resulting serial interrupt into IF. Returns the
    /// slave's outgoing byte if it was armed, else `None` (a no-op).
    pub(crate) fn link_slave_transfer(&mut self, master_byte: u8) -> Option<u8> {
        let (out, iff) = self.serial.link_slave_transfer(master_byte);
        self.intf |= iff & IF_MASK;
        out
    }
}
