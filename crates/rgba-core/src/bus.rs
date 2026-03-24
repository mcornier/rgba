use serde::{Deserialize, Serialize};

use crate::types::{AccessType, AccessWidth};

/// Trait for components that can be accessed via the memory bus
pub trait BusAccess {
    /// Read a byte from the given address
    fn read_byte(&self, address: u32) -> u8;

    /// Read a halfword (16-bit) from the given address (must be aligned to 2)
    fn read_halfword(&self, address: u32) -> u16 {
        let lo = self.read_byte(address) as u16;
        let hi = self.read_byte(address.wrapping_add(1)) as u16;
        lo | (hi << 8)
    }

    /// Read a word (32-bit) from the given address (must be aligned to 4)
    fn read_word(&self, address: u32) -> u32 {
        let lo = self.read_halfword(address) as u32;
        let hi = self.read_halfword(address.wrapping_add(2)) as u32;
        lo | (hi << 16)
    }

    /// Write a byte to the given address
    fn write_byte(&mut self, address: u32, value: u8);

    /// Write a halfword (16-bit) to the given address (must be aligned to 2)
    fn write_halfword(&mut self, address: u32, value: u16) {
        self.write_byte(address, value as u8);
        self.write_byte(address.wrapping_add(1), (value >> 8) as u8);
    }

    /// Write a word (32-bit) to the given address (must be aligned to 4)
    fn write_word(&mut self, address: u32, value: u32) {
        self.write_halfword(address, value as u16);
        self.write_halfword(address.wrapping_add(2), (value >> 16) as u16);
    }
}

/// The main GBA memory bus that dispatches reads/writes to the correct region
#[derive(Clone, Serialize, Deserialize)]
pub struct Bus {
    pub bios: Vec<u8>,
    pub ewram: Vec<u8>,
    pub iwram: Vec<u8>,
    pub io: Vec<u8>,
    pub palette: Vec<u8>,
    pub vram: Vec<u8>,
    pub oam: Vec<u8>,
    pub rom: Vec<u8>,
    pub sram: Vec<u8>,

    /// Key input register (active-low: 0 = pressed)
    pub keyinput: u16,

    /// Tracks the last BIOS read value for open bus behavior
    pub bios_latch: u32,

    /// Wait state configuration
    pub waitcnt: u16,

    /// Prefetch buffer enabled
    pub prefetch_enabled: bool,
}

impl Bus {
    pub fn new() -> Self {
        Self {
            bios: vec![0; crate::BIOS_SIZE],
            ewram: vec![0; crate::EWRAM_SIZE],
            iwram: vec![0; crate::IWRAM_SIZE],
            io: vec![0; crate::IO_SIZE],
            palette: vec![0; crate::PALETTE_SIZE],
            vram: vec![0; crate::VRAM_SIZE],
            oam: vec![0; crate::OAM_SIZE],
            rom: Vec::new(),
            sram: vec![0; crate::SRAM_SIZE],
            keyinput: 0x03FF, // all buttons released
            bios_latch: 0,
            waitcnt: 0,
            prefetch_enabled: false,
        }
    }

    /// Load a ROM into the cartridge slot
    pub fn load_rom(&mut self, data: &[u8]) {
        self.rom = data.to_vec();
    }

    /// Load BIOS data
    pub fn load_bios(&mut self, data: &[u8]) {
        let len = data.len().min(crate::BIOS_SIZE);
        self.bios[..len].copy_from_slice(&data[..len]);
    }

    /// Calculate wait states for a memory access
    pub fn wait_cycles(&self, address: u32, access: AccessType, width: AccessWidth) -> u32 {
        match address >> 24 {
            // BIOS, IWRAM: 0 wait states
            0x00 | 0x03 | 0x07 => 1,
            // EWRAM: 2 wait states (16-bit bus)
            0x02 => match width {
                AccessWidth::Word => 6,
                _ => 3,
            },
            // I/O: 0 wait states
            0x04 => 1,
            // Palette, VRAM, OAM
            0x05 | 0x06 => match width {
                AccessWidth::Word => 2,
                _ => 1,
            },
            // ROM wait state 0
            0x08 | 0x09 => {
                let n = ((self.waitcnt >> 2) & 3) as u32;
                let s = if self.waitcnt & (1 << 4) != 0 { 1 } else { 2 };
                let first = match access {
                    AccessType::NonSequential => [4, 3, 2, 8][n as usize],
                    AccessType::Sequential => s,
                };
                match width {
                    AccessWidth::Word => first + s + 1,
                    _ => first + 1,
                }
            }
            // ROM wait state 1
            0x0A | 0x0B => {
                let n = ((self.waitcnt >> 5) & 3) as u32;
                let s = if self.waitcnt & (1 << 7) != 0 { 1 } else { 4 };
                let first = match access {
                    AccessType::NonSequential => [4, 3, 2, 8][n as usize],
                    AccessType::Sequential => s,
                };
                match width {
                    AccessWidth::Word => first + s + 1,
                    _ => first + 1,
                }
            }
            // ROM wait state 2
            0x0C | 0x0D => {
                let n = ((self.waitcnt >> 8) & 3) as u32;
                let s = if self.waitcnt & (1 << 10) != 0 { 1 } else { 8 };
                let first = match access {
                    AccessType::NonSequential => [4, 3, 2, 8][n as usize],
                    AccessType::Sequential => s,
                };
                match width {
                    AccessWidth::Word => first + s + 1,
                    _ => first + 1,
                }
            }
            // SRAM: 8-bit bus, slow
            0x0E | 0x0F => {
                let n = (self.waitcnt & 3) as u32;
                [4, 3, 2, 8][n as usize] + 1
            }
            _ => 1,
        }
    }

    /// Read a byte for memory inspection (no side effects)
    pub fn peek_byte(&self, address: u32) -> u8 {
        self.read_byte(address)
    }

    /// Write a byte for memory manipulation (bypasses I/O side effects)
    pub fn poke_byte(&mut self, address: u32, value: u8) {
        match address >> 24 {
            0x02 => {
                let offset = (address & 0x3FFFF) as usize;
                if offset < self.ewram.len() {
                    self.ewram[offset] = value;
                }
            }
            0x03 => {
                let offset = (address & 0x7FFF) as usize;
                if offset < self.iwram.len() {
                    self.iwram[offset] = value;
                }
            }
            0x05 => {
                let offset = (address & 0x3FF) as usize;
                if offset < self.palette.len() {
                    self.palette[offset] = value;
                }
            }
            0x06 => {
                let offset = (address & 0x1FFFF) as usize;
                let offset = if offset >= crate::VRAM_SIZE {
                    offset - 0x8000
                } else {
                    offset
                };
                if offset < self.vram.len() {
                    self.vram[offset] = value;
                }
            }
            0x07 => {
                let offset = (address & 0x3FF) as usize;
                if offset < self.oam.len() {
                    self.oam[offset] = value;
                }
            }
            0x0E | 0x0F => {
                let offset = (address & 0xFFFF) as usize;
                if offset < self.sram.len() {
                    self.sram[offset] = value;
                }
            }
            _ => {}
        }
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

impl BusAccess for Bus {
    fn read_byte(&self, address: u32) -> u8 {
        match address >> 24 {
            0x00 => {
                // BIOS
                let offset = (address & 0x3FFF) as usize;
                if offset < self.bios.len() {
                    self.bios[offset]
                } else {
                    0
                }
            }
            0x02 => {
                // EWRAM (mirrored every 256KB)
                let offset = (address & 0x3FFFF) as usize;
                self.ewram[offset % self.ewram.len()]
            }
            0x03 => {
                // IWRAM (mirrored every 32KB)
                let offset = (address & 0x7FFF) as usize;
                self.iwram[offset % self.iwram.len()]
            }
            0x04 => {
                // I/O registers
                let offset = (address & 0x3FF) as usize;
                if offset == crate::REG_KEYINPUT as usize {
                    self.keyinput as u8
                } else if offset == (crate::REG_KEYINPUT + 1) as usize {
                    (self.keyinput >> 8) as u8
                } else if offset < self.io.len() {
                    self.io[offset]
                } else {
                    0
                }
            }
            0x05 => {
                // Palette RAM (mirrored every 1KB)
                let offset = (address & 0x3FF) as usize;
                self.palette[offset]
            }
            0x06 => {
                // VRAM (mirrored, with 96KB -> 128KB mapping)
                let offset = (address & 0x1FFFF) as usize;
                let offset = if offset >= crate::VRAM_SIZE {
                    offset - 0x8000
                } else {
                    offset
                };
                self.vram[offset % self.vram.len()]
            }
            0x07 => {
                // OAM (mirrored every 1KB)
                let offset = (address & 0x3FF) as usize;
                self.oam[offset]
            }
            0x08..=0x0D => {
                // ROM (mirrored across 3 wait state regions)
                let offset = (address & 0x01FF_FFFF) as usize;
                if offset < self.rom.len() {
                    self.rom[offset]
                } else {
                    // Open bus: return (address >> 1) for 16-bit reads
                    ((address >> 1) >> ((address & 1) * 8)) as u8
                }
            }
            0x0E | 0x0F => {
                // SRAM (8-bit only)
                let offset = (address & 0xFFFF) as usize;
                if offset < self.sram.len() {
                    self.sram[offset]
                } else {
                    0
                }
            }
            _ => {
                // Open bus / unused
                0
            }
        }
    }

    fn write_byte(&mut self, address: u32, value: u8) {
        match address >> 24 {
            0x00 => { /* BIOS is read-only */ }
            0x02 => {
                let offset = (address & 0x3FFFF) as usize;
                let len = self.ewram.len();
                self.ewram[offset % len] = value;
            }
            0x03 => {
                let offset = (address & 0x7FFF) as usize;
                let len = self.iwram.len();
                self.iwram[offset % len] = value;
            }
            0x04 => {
                let offset = (address & 0x3FF) as usize;
                if offset < self.io.len() {
                    self.io[offset] = value;
                }
                // TODO: I/O side effects (trigger DMA, update display, etc.)
            }
            0x05 => {
                // Palette: byte writes write the byte to both halves of the halfword
                let offset = (address & 0x3FE) as usize;
                self.palette[offset] = value;
                self.palette[offset + 1] = value;
            }
            0x06 => {
                // VRAM: byte writes are special (write to halfword)
                let offset = (address & 0x1FFFE) as usize;
                let offset = if offset >= crate::VRAM_SIZE {
                    offset - 0x8000
                } else {
                    offset
                };
                if offset < self.vram.len() {
                    self.vram[offset] = value;
                    if offset + 1 < self.vram.len() {
                        self.vram[offset + 1] = value;
                    }
                }
            }
            0x07 => { /* OAM: byte writes are ignored */ }
            0x08..=0x0D => { /* ROM is read-only */ }
            0x0E | 0x0F => {
                let offset = (address & 0xFFFF) as usize;
                if offset < self.sram.len() {
                    self.sram[offset] = value;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bus_ewram_read_write() {
        let mut bus = Bus::new();
        bus.write_byte(0x0200_0000, 0x42);
        assert_eq!(bus.read_byte(0x0200_0000), 0x42);

        // Test mirroring
        assert_eq!(bus.read_byte(0x0204_0000), 0x42);
    }

    #[test]
    fn test_bus_iwram_read_write() {
        let mut bus = Bus::new();
        bus.write_byte(0x0300_0000, 0xAB);
        assert_eq!(bus.read_byte(0x0300_0000), 0xAB);

        // Test mirroring
        assert_eq!(bus.read_byte(0x0300_8000), 0xAB);
    }

    #[test]
    fn test_bus_word_access() {
        let mut bus = Bus::new();
        bus.write_word(0x0200_0000, 0xDEAD_BEEF);
        assert_eq!(bus.read_word(0x0200_0000), 0xDEAD_BEEF);
        assert_eq!(bus.read_halfword(0x0200_0000), 0xBEEF);
        assert_eq!(bus.read_halfword(0x0200_0002), 0xDEAD);
    }

    #[test]
    fn test_bus_rom_read() {
        let mut bus = Bus::new();
        bus.load_rom(&[0x01, 0x02, 0x03, 0x04]);
        assert_eq!(bus.read_byte(0x0800_0000), 0x01);
        assert_eq!(bus.read_byte(0x0800_0003), 0x04);
    }

    #[test]
    fn test_bus_sram() {
        let mut bus = Bus::new();
        bus.write_byte(0x0E00_0000, 0xFF);
        assert_eq!(bus.read_byte(0x0E00_0000), 0xFF);
    }

    #[test]
    fn test_bus_bios_readonly() {
        let mut bus = Bus::new();
        bus.load_bios(&[0xAA; 16]);
        bus.write_byte(0x0000_0000, 0x55); // should be ignored
        assert_eq!(bus.read_byte(0x0000_0000), 0xAA);
    }

    #[test]
    fn test_keyinput_default() {
        let bus = Bus::new();
        // All buttons released = all bits set
        assert_eq!(bus.keyinput, 0x03FF);
    }
}
