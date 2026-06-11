//! Joypad matrix (FF00 P1). Timer/serial/joypad work package.

/// A Game Boy button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    Right,
    Left,
    Up,
    Down,
    A,
    B,
    Select,
    Start,
}

pub struct Joypad {
    // Work package owns state (select lines, held-button matrix, pending IRQ).
}

impl Joypad {
    pub fn new() -> Self {
        Self {}
    }

    pub fn press(&mut self, b: Button) {
        let _ = b;
        todo!("joypad work package")
    }

    pub fn release(&mut self, b: Button) {
        let _ = b;
        todo!("joypad work package")
    }

    /// IF bits requested since the last call (bit 4 = joypad), then clears.
    pub fn take_irq(&mut self) -> u8 {
        todo!("joypad work package")
    }

    /// Read FF00. Unselected/unused bits read 1.
    pub fn read(&self) -> u8 {
        todo!("joypad work package")
    }

    /// Write FF00 (select lines only).
    pub fn write(&mut self, value: u8) {
        let _ = value;
        todo!("joypad work package")
    }
}

impl Default for Joypad {
    fn default() -> Self {
        Self::new()
    }
}
