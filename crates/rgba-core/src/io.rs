use crate::bus::Bus;
use crate::constants::*;

// =============================================================================
// I/O Register Read/Write with side effects
// =============================================================================

impl Bus {
    /// Read a 16-bit I/O register with proper behavior
    pub fn io_read16(&self, offset: u32) -> u16 {
        match offset {
            REG_KEYINPUT => self.keyinput,
            REG_VCOUNT => {
                let lo = self.io[REG_VCOUNT as usize] as u16;
                let hi = self.io[(REG_VCOUNT + 1) as usize] as u16;
                lo | (hi << 8)
            }
            REG_DISPSTAT => {
                let lo = self.io[REG_DISPSTAT as usize] as u16;
                let hi = self.io[(REG_DISPSTAT + 1) as usize] as u16;
                lo | (hi << 8)
            }
            REG_IE => self.io_read16_raw(offset),
            REG_IF => self.io_read16_raw(offset),
            REG_IME => self.io_read16_raw(offset),
            REG_WAITCNT => self.waitcnt,
            // Timers: read counter values
            REG_TM0CNT_L | REG_TM1CNT_L | REG_TM2CNT_L | REG_TM3CNT_L => {
                self.io_read16_raw(offset)
            }
            _ => self.io_read16_raw(offset),
        }
    }

    /// Write a 16-bit I/O register with side effects
    pub fn io_write16(&mut self, offset: u32, value: u16) {
        match offset {
            REG_DISPCNT | REG_GREENSWAP => self.io_write16_raw(offset, value),
            REG_DISPSTAT => {
                // Only bits 3-7 are writable (VCount setting, IRQ enables)
                let old = self.io_read16_raw(offset);
                let new_val = (old & 0x0007) | (value & 0xFFF8);
                self.io_write16_raw(offset, new_val);
            }
            REG_VCOUNT => { /* Read-only */ }

            // Background control
            REG_BG0CNT..=REG_BG3VOFS => self.io_write16_raw(offset, value),

            // BG rotation/scaling
            REG_BG2PA..=REG_BG3PD => self.io_write16_raw(offset, value),

            // Window
            REG_WIN0H..=REG_WINOUT => self.io_write16_raw(offset, value),

            // Mosaic
            REG_MOSAIC => self.io_write16_raw(offset, value),

            // Blending
            REG_BLDCNT..=REG_BLDY => self.io_write16_raw(offset, value),

            // Sound registers
            REG_SOUND1CNT_L..=REG_SOUNDBIAS => self.io_write16_raw(offset, value),

            // FIFO writes (32-bit only, but handle 16-bit)
            REG_FIFO_A | REG_FIFO_B => self.io_write16_raw(offset, value),

            // DMA registers
            REG_DMA0SAD..=REG_DMA3CNT_H => {
                self.io_write16_raw(offset, value);
            }

            // Timer registers
            REG_TM0CNT_L..=REG_TM3CNT_H => {
                self.io_write16_raw(offset, value);
            }

            // Keypad control
            REG_KEYCNT => self.io_write16_raw(offset, value),

            // Interrupt control
            REG_IE => self.io_write16_raw(offset, value),
            REG_IF => {
                // Writing 1 to a bit acknowledges (clears) that interrupt
                let current = self.io_read16_raw(REG_IF);
                self.io_write16_raw(REG_IF, current & !value);
            }
            REG_WAITCNT => {
                self.waitcnt = value;
                self.prefetch_enabled = value & (1 << 14) != 0;
                self.io_write16_raw(offset, value);
            }
            REG_IME => self.io_write16_raw(offset, value & 1),

            _ => self.io_write16_raw(offset, value),
        }
    }

    /// Write a 32-bit value to I/O (for BG2X/Y, BG3X/Y, FIFO, DMA addresses)
    pub fn io_write32(&mut self, offset: u32, value: u32) {
        match offset {
            REG_BG2X | REG_BG2Y | REG_BG3X | REG_BG3Y => {
                self.io_write16_raw(offset, value as u16);
                self.io_write16_raw(offset + 2, (value >> 16) as u16);
            }
            REG_FIFO_A | REG_FIFO_B => {
                // 32-bit FIFO writes push 4 bytes
                self.io_write16_raw(offset, value as u16);
                self.io_write16_raw(offset + 2, (value >> 16) as u16);
            }
            _ => {
                self.io_write16(offset, value as u16);
                self.io_write16(offset + 2, (value >> 16) as u16);
            }
        }
    }

    /// Request an interrupt
    pub fn request_interrupt(&mut self, irq: u16) {
        let current = self.io_read16_raw(REG_IF);
        self.io_write16_raw(REG_IF, current | irq);
    }

    /// Check if there are pending, enabled interrupts
    pub fn has_pending_irq(&self) -> bool {
        let ime = self.io_read16_raw(REG_IME) & 1 != 0;
        if !ime {
            return false;
        }
        let ie = self.io_read16_raw(REG_IE);
        let if_ = self.io_read16_raw(REG_IF);
        ie & if_ != 0
    }

    /// Update DISPSTAT and VCOUNT registers
    pub fn update_display_status(&mut self, vcount: u16, hblank: bool) {
        self.io[REG_VCOUNT as usize] = vcount as u8;
        self.io[(REG_VCOUNT + 1) as usize] = (vcount >> 8) as u8;

        let dispstat = self.io_read16_raw(REG_DISPSTAT);
        let vcount_setting = (dispstat >> 8) as u16;

        let mut new_dispstat = dispstat & 0xFFF8; // clear status bits

        // Bit 0: V-Blank flag
        if vcount >= VISIBLE_LINES as u16 && vcount < TOTAL_LINES as u16 {
            new_dispstat |= 1;
        }
        // Bit 1: H-Blank flag
        if hblank {
            new_dispstat |= 2;
        }
        // Bit 2: V-Counter match flag
        if vcount == vcount_setting {
            new_dispstat |= 4;
        }

        self.io_write16_raw(REG_DISPSTAT, new_dispstat);
    }

    // Raw read/write helpers (no side effects)

    fn io_read16_raw(&self, offset: u32) -> u16 {
        let idx = offset as usize;
        if idx + 1 < self.io.len() {
            self.io[idx] as u16 | ((self.io[idx + 1] as u16) << 8)
        } else {
            0
        }
    }

    fn io_write16_raw(&mut self, offset: u32, value: u16) {
        let idx = offset as usize;
        if idx + 1 < self.io.len() {
            self.io[idx] = value as u8;
            self.io[idx + 1] = (value >> 8) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interrupt_request_and_acknowledge() {
        let mut bus = Bus::new();

        // Enable VBlank interrupt
        bus.io_write16(REG_IE, IRQ_VBLANK);
        bus.io_write16(REG_IME, 1);

        // Request VBlank IRQ
        bus.request_interrupt(IRQ_VBLANK);
        assert!(bus.has_pending_irq());

        // Acknowledge it
        bus.io_write16(REG_IF, IRQ_VBLANK);
        assert!(!bus.has_pending_irq());
    }

    #[test]
    fn test_ime_disable() {
        let mut bus = Bus::new();
        bus.io_write16(REG_IE, IRQ_VBLANK);
        bus.io_write16(REG_IME, 0); // disabled
        bus.request_interrupt(IRQ_VBLANK);
        assert!(!bus.has_pending_irq());
    }

    #[test]
    fn test_dispstat_update() {
        let mut bus = Bus::new();
        bus.update_display_status(160, false); // VBlank line
        let dispstat = bus.io_read16(REG_DISPSTAT);
        assert!(dispstat & 1 != 0); // VBlank flag set

        bus.update_display_status(0, true); // HBlank
        let dispstat = bus.io_read16(REG_DISPSTAT);
        assert!(dispstat & 2 != 0); // HBlank flag set
        assert!(dispstat & 1 == 0); // No VBlank
    }

    #[test]
    fn test_vcount_match() {
        let mut bus = Bus::new();
        // Set VCount trigger to line 100
        bus.io_write16(REG_DISPSTAT, 100 << 8);
        bus.update_display_status(100, false);
        let dispstat = bus.io_read16(REG_DISPSTAT);
        assert!(dispstat & 4 != 0); // VCount match
    }

    #[test]
    fn test_waitcnt() {
        let mut bus = Bus::new();
        bus.io_write16(REG_WAITCNT, 0x4317);
        assert_eq!(bus.waitcnt, 0x4317);
        assert!(bus.prefetch_enabled);
    }
}
