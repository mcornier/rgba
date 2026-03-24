use serde::{Deserialize, Serialize};

/// Cartridge backup type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupType {
    None,
    Sram,        // 32KB or 64KB
    Flash64,     // 64KB flash
    Flash128,    // 128KB flash (bank-switched)
    Eeprom512,   // 512 bytes (4Kbit)
    Eeprom8K,    // 8KB (64Kbit)
}

/// Flash memory state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashState {
    pub data: Vec<u8>,
    pub bank: u8,
    pub command_state: FlashCommand,
    pub chip_id_mode: bool,
    /// Manufacturer + device ID (e.g. Sanyo 128K = 0x1362)
    pub chip_id: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlashCommand {
    Ready,
    Command1,   // 0x5555 = 0xAA
    Command2,   // 0x2AAA = 0x55
    Erase1,     // waiting for erase type
    Write,      // next byte is a write
    BankSelect, // next write to 0x0000 selects bank
}

impl FlashState {
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![0xFF; size],
            bank: 0,
            command_state: FlashCommand::Ready,
            chip_id_mode: false,
            chip_id: if size > 0x10000 { 0x1362 } else { 0x1B32 },
        }
    }

    pub fn read(&self, address: u32) -> u8 {
        let offset = (address & 0xFFFF) as usize;
        if self.chip_id_mode && offset < 2 {
            return if offset == 0 {
                self.chip_id as u8
            } else {
                (self.chip_id >> 8) as u8
            };
        }
        let bank_offset = self.bank as usize * 0x10000;
        let addr = bank_offset + offset;
        if addr < self.data.len() {
            self.data[addr]
        } else {
            0xFF
        }
    }

    pub fn write(&mut self, address: u32, value: u8) {
        let offset = (address & 0xFFFF) as usize;

        match self.command_state {
            FlashCommand::Ready => {
                if offset == 0x5555 && value == 0xAA {
                    self.command_state = FlashCommand::Command1;
                }
            }
            FlashCommand::Command1 => {
                if offset == 0x2AAA && value == 0x55 {
                    self.command_state = FlashCommand::Command2;
                } else {
                    self.command_state = FlashCommand::Ready;
                }
            }
            FlashCommand::Command2 => {
                if offset == 0x5555 {
                    match value {
                        0x90 => {
                            self.chip_id_mode = true;
                            self.command_state = FlashCommand::Ready;
                        }
                        0xF0 => {
                            self.chip_id_mode = false;
                            self.command_state = FlashCommand::Ready;
                        }
                        0x80 => {
                            self.command_state = FlashCommand::Erase1;
                        }
                        0xA0 => {
                            self.command_state = FlashCommand::Write;
                        }
                        0xB0 => {
                            self.command_state = FlashCommand::BankSelect;
                        }
                        _ => {
                            self.command_state = FlashCommand::Ready;
                        }
                    }
                } else {
                    self.command_state = FlashCommand::Ready;
                }
            }
            FlashCommand::Erase1 => {
                if offset == 0x5555 && value == 0xAA {
                    // Continue erase sequence
                } else if offset == 0x2AAA && value == 0x55 {
                    // Continue
                } else if value == 0x10 && offset == 0x5555 {
                    // Full chip erase
                    for byte in self.data.iter_mut() {
                        *byte = 0xFF;
                    }
                    self.command_state = FlashCommand::Ready;
                } else if value == 0x30 {
                    // Sector erase (4KB)
                    let sector = offset & 0xF000;
                    let bank_offset = self.bank as usize * 0x10000;
                    let start = bank_offset + sector;
                    let end = (start + 0x1000).min(self.data.len());
                    for byte in &mut self.data[start..end] {
                        *byte = 0xFF;
                    }
                    self.command_state = FlashCommand::Ready;
                } else {
                    self.command_state = FlashCommand::Ready;
                }
            }
            FlashCommand::Write => {
                let bank_offset = self.bank as usize * 0x10000;
                let addr = bank_offset + offset;
                if addr < self.data.len() {
                    self.data[addr] = value;
                }
                self.command_state = FlashCommand::Ready;
            }
            FlashCommand::BankSelect => {
                if offset == 0x0000 {
                    self.bank = value & 1;
                }
                self.command_state = FlashCommand::Ready;
            }
        }
    }
}

/// Detect backup type from ROM data (by scanning for ID strings)
pub fn detect_backup_type(rom: &[u8]) -> BackupType {
    let rom_str = String::from_utf8_lossy(rom);

    if rom_str.contains("SRAM_V") || rom_str.contains("SRAM_F_V") {
        BackupType::Sram
    } else if rom_str.contains("FLASH1M_V") {
        BackupType::Flash128
    } else if rom_str.contains("FLASH_V") || rom_str.contains("FLASH512_V") {
        BackupType::Flash64
    } else if rom_str.contains("EEPROM_V") {
        // Distinguish EEPROM sizes based on ROM size
        if rom.len() > 16 * 1024 * 1024 {
            BackupType::Eeprom8K
        } else {
            BackupType::Eeprom512
        }
    } else {
        BackupType::None
    }
}

/// Cartridge state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cartridge {
    pub backup_type: BackupType,
    pub flash: Option<FlashState>,
    pub sram: Vec<u8>,
    pub title: String,
    pub game_code: String,
}

impl Cartridge {
    pub fn from_rom(rom: &[u8]) -> Self {
        let backup_type = detect_backup_type(rom);

        // Read header info
        let title = String::from_utf8_lossy(&rom[0xA0..0xAC])
            .trim_end_matches('\0')
            .to_string();
        let game_code = String::from_utf8_lossy(&rom[0xAC..0xB0])
            .trim_end_matches('\0')
            .to_string();

        let flash = match backup_type {
            BackupType::Flash64 => Some(FlashState::new(0x10000)),
            BackupType::Flash128 => Some(FlashState::new(0x20000)),
            _ => None,
        };

        let sram = match backup_type {
            BackupType::Sram => vec![0; 0x10000],
            _ => Vec::new(),
        };

        Self {
            backup_type,
            flash,
            sram,
            title,
            game_code,
        }
    }

    /// Get save data for export
    pub fn get_save_data(&self) -> Vec<u8> {
        match self.backup_type {
            BackupType::Sram => self.sram.clone(),
            BackupType::Flash64 | BackupType::Flash128 => {
                self.flash.as_ref().map(|f| f.data.clone()).unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }

    /// Load save data
    pub fn load_save_data(&mut self, data: &[u8]) {
        match self.backup_type {
            BackupType::Sram => {
                let len = data.len().min(self.sram.len());
                self.sram[..len].copy_from_slice(&data[..len]);
            }
            BackupType::Flash64 | BackupType::Flash128 => {
                if let Some(flash) = &mut self.flash {
                    let len = data.len().min(flash.data.len());
                    flash.data[..len].copy_from_slice(&data[..len]);
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
    fn test_detect_sram() {
        let mut rom = vec![0u8; 256];
        rom[0xA0..0xA0 + 6].copy_from_slice(b"SRAM_V");
        assert_eq!(detect_backup_type(&rom), BackupType::Sram);
    }

    #[test]
    fn test_detect_flash128() {
        let mut rom = vec![0u8; 256];
        rom[0xA0..0xA0 + 9].copy_from_slice(b"FLASH1M_V");
        assert_eq!(detect_backup_type(&rom), BackupType::Flash128);
    }

    #[test]
    fn test_flash_write_read() {
        let mut flash = FlashState::new(0x10000);

        // Write byte via command sequence
        flash.write(0x5555, 0xAA); // Command 1
        flash.write(0x2AAA, 0x55); // Command 2
        flash.write(0x5555, 0xA0); // Write mode
        flash.write(0x0000, 0x42); // Write 0x42 at address 0

        assert_eq!(flash.read(0x0000), 0x42);
    }

    #[test]
    fn test_flash_sector_erase() {
        let mut flash = FlashState::new(0x10000);

        // Write some data first
        flash.write(0x5555, 0xAA);
        flash.write(0x2AAA, 0x55);
        flash.write(0x5555, 0xA0);
        flash.write(0x0000, 0x42);

        // Sector erase
        flash.write(0x5555, 0xAA);
        flash.write(0x2AAA, 0x55);
        flash.write(0x5555, 0x80);
        flash.write(0x0000, 0x30); // erase sector 0

        assert_eq!(flash.read(0x0000), 0xFF); // erased
    }
}
