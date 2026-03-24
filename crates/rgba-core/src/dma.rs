use serde::{Deserialize, Serialize};

use crate::bus::Bus;
use crate::constants::*;

/// DMA timing modes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DmaTiming {
    Immediate = 0,
    VBlank = 1,
    HBlank = 2,
    Special = 3, // DMA1/2: Sound FIFO, DMA3: Video Capture
}

/// DMA address control
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DmaAddrCtrl {
    Increment = 0,
    Decrement = 1,
    Fixed = 2,
    IncrReload = 3, // Increment + reload after transfer
}

/// State for a single DMA channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmaChannel {
    pub source: u32,
    pub dest: u32,
    pub count: u32,
    /// Internal source address (latched on enable)
    pub internal_source: u32,
    /// Internal destination address (latched on enable)
    pub internal_dest: u32,
    /// Internal count (latched on enable)
    pub internal_count: u32,
    pub enabled: bool,
}

impl Default for DmaChannel {
    fn default() -> Self {
        Self {
            source: 0,
            dest: 0,
            count: 0,
            internal_source: 0,
            internal_dest: 0,
            internal_count: 0,
            enabled: false,
        }
    }
}

/// DMA controller with 4 channels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmaController {
    pub channels: [DmaChannel; 4],
}

impl DmaController {
    pub fn new() -> Self {
        Self {
            channels: Default::default(),
        }
    }

    /// Update DMA channel registers from I/O writes
    pub fn update_from_io(&mut self, bus: &Bus) {
        for ch in 0..4 {
            let base = REG_DMA0SAD + (ch as u32 * 12);

            let sad_lo = bus.io[base as usize] as u32
                | ((bus.io[(base + 1) as usize] as u32) << 8);
            let sad_hi = bus.io[(base + 2) as usize] as u32
                | ((bus.io[(base + 3) as usize] as u32) << 8);
            self.channels[ch].source = sad_lo | (sad_hi << 16);

            let dad_lo = bus.io[(base + 4) as usize] as u32
                | ((bus.io[(base + 5) as usize] as u32) << 8);
            let dad_hi = bus.io[(base + 6) as usize] as u32
                | ((bus.io[(base + 7) as usize] as u32) << 8);
            self.channels[ch].dest = dad_lo | (dad_hi << 16);

            let cnt_l = bus.io[(base + 8) as usize] as u32
                | ((bus.io[(base + 9) as usize] as u32) << 8);
            self.channels[ch].count = cnt_l;

            let cnt_h = bus.io[(base + 10) as usize] as u16
                | ((bus.io[(base + 11) as usize] as u16) << 8);

            let newly_enabled = cnt_h & (1 << 15) != 0;

            if newly_enabled && !self.channels[ch].enabled {
                // Latch internal registers on first enable
                self.channels[ch].internal_source = self.channels[ch].source;
                self.channels[ch].internal_dest = self.channels[ch].dest;
                self.channels[ch].internal_count = if self.channels[ch].count == 0 {
                    if ch == 3 { 0x10000 } else { 0x4000 }
                } else {
                    self.channels[ch].count
                };
            }

            self.channels[ch].enabled = newly_enabled;
        }
    }

    /// Get the control halfword for a channel
    fn get_control(bus: &Bus, ch: usize) -> u16 {
        let base = REG_DMA0SAD + (ch as u32 * 12) + 10;
        bus.io[base as usize] as u16 | ((bus.io[(base + 1) as usize] as u16) << 8)
    }

    /// Execute a DMA transfer for the given channel, returns cycles consumed
    pub fn execute_channel(&mut self, ch: usize, bus: &mut Bus) -> u32 {
        if !self.channels[ch].enabled {
            return 0;
        }

        let control = Self::get_control(bus, ch);
        let word_transfer = control & (1 << 10) != 0;
        let src_ctrl = DmaAddrCtrl::from_bits((control >> 7) & 3);
        let dst_ctrl = DmaAddrCtrl::from_bits((control >> 5) & 3);

        let step = if word_transfer { 4 } else { 2 };
        let count = self.channels[ch].internal_count;
        let mut src = self.channels[ch].internal_source;
        let mut dst = self.channels[ch].internal_dest;

        let mut cycles = 0u32;

        for _ in 0..count {
            if word_transfer {
                let val = bus.read_word(src & !3);
                bus.write_word(dst & !3, val);
            } else {
                let val = bus.read_halfword(src & !1);
                bus.write_halfword(dst & !1, val);
            }

            src = match src_ctrl {
                DmaAddrCtrl::Increment | DmaAddrCtrl::IncrReload => src.wrapping_add(step),
                DmaAddrCtrl::Decrement => src.wrapping_sub(step),
                DmaAddrCtrl::Fixed => src,
            };
            dst = match dst_ctrl {
                DmaAddrCtrl::Increment | DmaAddrCtrl::IncrReload => dst.wrapping_add(step),
                DmaAddrCtrl::Decrement => dst.wrapping_sub(step),
                DmaAddrCtrl::Fixed => dst,
            };
            cycles += 2;
        }

        self.channels[ch].internal_source = src;
        self.channels[ch].internal_dest = if dst_ctrl == DmaAddrCtrl::IncrReload {
            self.channels[ch].dest
        } else {
            dst
        };

        let repeat = control & (1 << 9) != 0;
        let irq = control & (1 << 14) != 0;

        if !repeat {
            self.channels[ch].enabled = false;
            // Clear enable bit in I/O
            let base = REG_DMA0SAD + (ch as u32 * 12) + 10;
            let old = bus.io[base as usize] as u16 | ((bus.io[(base + 1) as usize] as u16) << 8);
            let new_val = old & !(1 << 15);
            bus.io[base as usize] = new_val as u8;
            bus.io[(base + 1) as usize] = (new_val >> 8) as u8;
        }

        if irq {
            let irq_bit = match ch {
                0 => IRQ_DMA0,
                1 => IRQ_DMA1,
                2 => IRQ_DMA2,
                3 => IRQ_DMA3,
                _ => 0,
            };
            bus.request_interrupt(irq_bit);
        }

        cycles
    }

    /// Check and execute DMA channels triggered by the given timing
    pub fn check_timing(&mut self, timing: DmaTiming, bus: &mut Bus) -> u32 {
        let mut total_cycles = 0;
        for ch in 0..4 {
            if !self.channels[ch].enabled {
                continue;
            }
            let control = Self::get_control(bus, ch);
            let ch_timing = (control >> 12) & 3;
            if ch_timing == timing as u16 {
                total_cycles += self.execute_channel(ch, bus);
            }
        }
        total_cycles
    }
}

impl Default for DmaController {
    fn default() -> Self {
        Self::new()
    }
}

impl DmaAddrCtrl {
    fn from_bits(bits: u16) -> Self {
        match bits & 3 {
            0 => DmaAddrCtrl::Increment,
            1 => DmaAddrCtrl::Decrement,
            2 => DmaAddrCtrl::Fixed,
            3 => DmaAddrCtrl::IncrReload,
            _ => unreachable!(),
        }
    }
}

use crate::bus::BusAccess;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dma_immediate_transfer() {
        let mut bus = Bus::new();
        let mut dma = DmaController::new();

        // Write test data to EWRAM
        for i in 0..8u32 {
            bus.write_word(0x0200_0000 + i * 4, 0x1000 + i);
        }

        // Set up DMA0: copy 8 words from 0x02000000 to 0x02001000
        let base = REG_DMA0SAD as usize;
        // Source: 0x02000000
        bus.io[base] = 0x00;
        bus.io[base + 1] = 0x00;
        bus.io[base + 2] = 0x00;
        bus.io[base + 3] = 0x02;
        // Dest: 0x02001000
        bus.io[base + 4] = 0x00;
        bus.io[base + 5] = 0x10;
        bus.io[base + 6] = 0x00;
        bus.io[base + 7] = 0x02;
        // Count: 8
        bus.io[base + 8] = 8;
        bus.io[base + 9] = 0;
        // Control: Enable, Word transfer, Immediate
        let ctrl: u16 = (1 << 15) | (1 << 10); // enable + word
        bus.io[base + 10] = ctrl as u8;
        bus.io[base + 11] = (ctrl >> 8) as u8;

        dma.update_from_io(&bus);
        assert!(dma.channels[0].enabled);

        let cycles = dma.check_timing(DmaTiming::Immediate, &mut bus);
        assert!(cycles > 0);

        // Verify data was copied
        for i in 0..8u32 {
            assert_eq!(bus.read_word(0x0200_1000 + i * 4), 0x1000 + i);
        }
    }

    #[test]
    fn test_dma_halfword_transfer() {
        let mut bus = Bus::new();
        let mut dma = DmaController::new();

        bus.write_halfword(0x0200_0000, 0xABCD);

        let base = REG_DMA0SAD as usize;
        bus.io[base] = 0x00;
        bus.io[base + 1] = 0x00;
        bus.io[base + 2] = 0x00;
        bus.io[base + 3] = 0x02;
        bus.io[base + 4] = 0x00;
        bus.io[base + 5] = 0x10;
        bus.io[base + 6] = 0x00;
        bus.io[base + 7] = 0x02;
        bus.io[base + 8] = 1;
        bus.io[base + 9] = 0;
        // Control: Enable, Halfword transfer
        let ctrl: u16 = 1 << 15;
        bus.io[base + 10] = ctrl as u8;
        bus.io[base + 11] = (ctrl >> 8) as u8;

        dma.update_from_io(&bus);
        dma.check_timing(DmaTiming::Immediate, &mut bus);

        assert_eq!(bus.read_halfword(0x0200_1000), 0xABCD);
    }
}
