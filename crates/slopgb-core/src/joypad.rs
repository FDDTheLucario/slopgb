//! Joypad matrix (FF00 P1). Timer/serial/joypad work package.
//!
//! P1 exposes a 2x4 key matrix: bit 4 selects the d-pad column, bit 5 the
//! button column (both active low). The low nibble is the AND of all
//! selected columns (pressed = 0). The joypad interrupt fires on any
//! high-to-low transition of the P10-P13 input lines — whether caused by a
//! button press or by a select-line write that exposes an already-held
//! button (Pan Docs "Joypad Input" / "INT $60").

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

impl Button {
    /// (is_dpad_column, active-low line mask within the low nibble).
    fn line(self) -> (bool, u8) {
        match self {
            Button::Right => (true, 0x01),
            Button::Left => (true, 0x02),
            Button::Up => (true, 0x04),
            Button::Down => (true, 0x08),
            Button::A => (false, 0x01),
            Button::B => (false, 0x02),
            Button::Select => (false, 0x04),
            Button::Start => (false, 0x08),
        }
    }
}

pub struct Joypad {
    /// P1 bits 4-5 as last written (active low; 1 = column not selected).
    select: u8,
    /// D-pad column, active low (bit 0 Right, 1 Left, 2 Up, 3 Down).
    dpad: u8,
    /// Button column, active low (bit 0 A, 1 B, 2 Select, 3 Start).
    buttons: u8,
    /// Latched IF bits not yet collected by `take_irq`.
    irq: u8,
}

impl Joypad {
    pub fn new() -> Self {
        Self {
            // Both columns selected: P1 reads 0xCF with nothing pressed,
            // the DMG/CGB post-boot value.
            select: 0x00,
            dpad: 0x0F,
            buttons: 0x0F,
            irq: 0,
        }
    }

    /// The P10-P13 input lines: AND of every selected column, 1 when idle.
    fn input_lines(&self) -> u8 {
        let mut lines = 0x0F;
        if self.select & 0x10 == 0 {
            lines &= self.dpad;
        }
        if self.select & 0x20 == 0 {
            lines &= self.buttons;
        }
        lines
    }

    /// Latch the joypad interrupt on any 1 -> 0 input line transition.
    /// `before` is a prior `input_lines()` value, so both operands are
    /// already confined to the low nibble.
    fn latch_edges(&mut self, before: u8) {
        if before & !self.input_lines() != 0 {
            self.irq |= 0x10;
        }
    }

    pub fn press(&mut self, b: Button) {
        let before = self.input_lines();
        let (dpad, mask) = b.line();
        if dpad {
            self.dpad &= !mask;
        } else {
            self.buttons &= !mask;
        }
        self.latch_edges(before);
    }

    pub fn release(&mut self, b: Button) {
        let (dpad, mask) = b.line();
        if dpad {
            self.dpad |= mask;
        } else {
            self.buttons |= mask;
        }
        // Releases only produce rising edges; no interrupt.
    }

    /// IF bits requested since the last call (bit 4 = joypad), then clears.
    pub fn take_irq(&mut self) -> u8 {
        let irq = self.irq;
        self.irq = 0;
        irq
    }

    /// Read FF00. Unselected/unused bits read 1.
    pub fn read(&self) -> u8 {
        0xC0 | self.select | self.input_lines()
    }

    /// Write FF00 (select lines only).
    pub fn write(&mut self, value: u8) {
        let before = self.input_lines();
        self.select = value & 0x30;
        // Newly exposing a held button drops a P1 line: interrupt.
        self.latch_edges(before);
    }
}

impl Default for Joypad {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_boot_read_is_cf() {
        // Both columns selected, nothing pressed (DMG/CGB post-boot P1).
        assert_eq!(Joypad::new().read(), 0xCF);
    }

    #[test]
    fn deselected_columns_read_all_ones() {
        let mut j = Joypad::new();
        j.write(0x30);
        j.press(Button::A);
        j.press(Button::Down);
        assert_eq!(j.read(), 0xFF);
    }

    #[test]
    fn only_select_bits_are_writable() {
        let mut j = Joypad::new();
        j.write(0xCF); // bits 0-3 and 6-7 ignored
        assert_eq!(j.read(), 0xCF);
        j.write(0xFF);
        assert_eq!(j.read(), 0xFF);
    }

    #[test]
    fn dpad_press_reads_active_low_and_raises_irq() {
        let mut j = Joypad::new();
        j.write(0x20); // select d-pad column only
        j.press(Button::Right);
        assert_eq!(j.read(), 0xEE); // bit 0 low
        assert_eq!(j.take_irq(), 0x10);
        assert_eq!(j.take_irq(), 0, "take_irq clears the latch");
    }

    #[test]
    fn button_press_reads_active_low_and_raises_irq() {
        let mut j = Joypad::new();
        j.write(0x10); // select button column only
        j.press(Button::Start);
        assert_eq!(j.read(), 0xD7); // bit 3 low
        assert_eq!(j.take_irq(), 0x10);
    }

    #[test]
    fn unselected_press_no_irq_until_column_selected() {
        let mut j = Joypad::new();
        j.write(0x30); // nothing selected
        j.press(Button::A);
        assert_eq!(j.read(), 0xFF);
        assert_eq!(j.take_irq(), 0);
        // Selecting the button column exposes the held A: line falls -> IRQ.
        j.write(0x10);
        assert_eq!(j.read(), 0xDE);
        assert_eq!(j.take_irq(), 0x10);
    }

    #[test]
    fn release_restores_line_without_irq() {
        let mut j = Joypad::new();
        j.write(0x20);
        j.press(Button::Up);
        j.take_irq();
        j.release(Button::Up);
        assert_eq!(j.read(), 0xEF);
        assert_eq!(j.take_irq(), 0);
    }

    #[test]
    fn both_columns_selected_are_anded() {
        let mut j = Joypad::new();
        j.write(0x00);
        j.press(Button::Right); // d-pad bit 0
        j.press(Button::B); // button bit 1
        assert_eq!(j.read(), 0xCC); // 0b1110 & 0b1101 = 0b1100
    }

    #[test]
    fn repeated_press_does_not_relatch_irq() {
        let mut j = Joypad::new();
        j.write(0x10);
        j.press(Button::A);
        assert_eq!(j.take_irq(), 0x10);
        j.press(Button::A); // line already low: no new edge
        assert_eq!(j.take_irq(), 0);
    }

    #[test]
    fn deselecting_produces_no_irq() {
        let mut j = Joypad::new();
        j.write(0x10);
        j.press(Button::A);
        j.take_irq();
        j.write(0x30); // line rises: no IRQ
        assert_eq!(j.take_irq(), 0);
    }

    #[test]
    fn impossible_dpad_combo_passes_through() {
        // Hardware cannot reject Left+Right; the frontend may send it and
        // the matrix reports it honestly.
        let mut j = Joypad::new();
        j.write(0x20);
        j.press(Button::Left);
        j.press(Button::Right);
        assert_eq!(j.read(), 0xEC);
    }
}
