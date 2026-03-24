use bitflags::bitflags;
use serde::{Deserialize, Serialize};

// =============================================================================
// CPU Types
// =============================================================================

/// ARM7TDMI processor modes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum CpuMode {
    User = 0x10,
    Fiq = 0x11,
    Irq = 0x12,
    Supervisor = 0x13,
    Abort = 0x17,
    Undefined = 0x1B,
    System = 0x1F,
}

impl CpuMode {
    pub fn from_bits(bits: u8) -> Option<Self> {
        match bits & 0x1F {
            0x10 => Some(CpuMode::User),
            0x11 => Some(CpuMode::Fiq),
            0x12 => Some(CpuMode::Irq),
            0x13 => Some(CpuMode::Supervisor),
            0x17 => Some(CpuMode::Abort),
            0x1B => Some(CpuMode::Undefined),
            0x1F => Some(CpuMode::System),
            _ => None,
        }
    }

    /// Returns the bank index for banked registers
    pub fn bank_index(self) -> usize {
        match self {
            CpuMode::User | CpuMode::System => 0,
            CpuMode::Fiq => 1,
            CpuMode::Irq => 2,
            CpuMode::Supervisor => 3,
            CpuMode::Abort => 4,
            CpuMode::Undefined => 5,
        }
    }

    pub fn has_spsr(self) -> bool {
        !matches!(self, CpuMode::User | CpuMode::System)
    }
}

bitflags! {
    /// Current Program Status Register flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Psr: u32 {
        /// Negative / Less Than
        const N = 1 << 31;
        /// Zero
        const Z = 1 << 30;
        /// Carry / Borrow / Extend
        const C = 1 << 29;
        /// Overflow
        const V = 1 << 28;
        /// IRQ disable
        const I = 1 << 7;
        /// FIQ disable
        const F = 1 << 6;
        /// Thumb state (0=ARM, 1=THUMB)
        const T = 1 << 5;
    }
}

impl Serialize for Psr {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.bits().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Psr {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bits = u32::deserialize(deserializer)?;
        Ok(Psr::from_bits_retain(bits))
    }
}

impl Psr {
    /// Get the processor mode from the PSR mode bits
    pub fn mode(self) -> CpuMode {
        CpuMode::from_bits((self.bits() & 0x1F) as u8).unwrap_or(CpuMode::System)
    }

    /// Set the processor mode bits
    pub fn set_mode(&mut self, mode: CpuMode) {
        let bits = (self.bits() & !0x1F) | (mode as u32);
        *self = Psr::from_bits_retain(bits);
    }

    /// Check if CPU is in Thumb state
    pub fn thumb(self) -> bool {
        self.contains(Psr::T)
    }

    /// Check if IRQs are disabled
    pub fn irq_disabled(self) -> bool {
        self.contains(Psr::I)
    }

    /// Check if FIQs are disabled
    pub fn fiq_disabled(self) -> bool {
        self.contains(Psr::F)
    }
}

// =============================================================================
// ARM7TDMI Register File
// =============================================================================

/// ARM7TDMI register file with banked registers for each mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterFile {
    /// General purpose registers R0-R15 (R13=SP, R14=LR, R15=PC)
    pub gpr: [u32; 16],
    /// Current Program Status Register
    pub cpsr: Psr,
    /// Saved Program Status Registers (one per banked mode: FIQ, IRQ, SVC, ABT, UND)
    pub spsr: [Psr; 5],
    /// Banked R13 (SP) for each mode: [User/System, FIQ, IRQ, SVC, ABT, UND]
    pub sp_bank: [u32; 6],
    /// Banked R14 (LR) for each mode
    pub lr_bank: [u32; 6],
    /// Banked R8-R12 for FIQ mode (FIQ has its own R8-R12)
    pub fiq_bank: [u32; 5],
    /// Non-FIQ R8-R12 saved when switching to FIQ
    pub usr_bank_r8_r12: [u32; 5],
}

impl Default for RegisterFile {
    fn default() -> Self {
        Self {
            gpr: [0; 16],
            cpsr: Psr::from_bits_retain(CpuMode::System as u32 | Psr::I.bits() | Psr::F.bits()),
            spsr: [Psr::empty(); 5],
            sp_bank: [0; 6],
            lr_bank: [0; 6],
            fiq_bank: [0; 5],
            usr_bank_r8_r12: [0; 5],
        }
    }
}

impl RegisterFile {
    /// PC register index
    pub const PC: usize = 15;
    /// LR register index
    pub const LR: usize = 14;
    /// SP register index
    pub const SP: usize = 13;

    pub fn pc(&self) -> u32 {
        self.gpr[Self::PC]
    }

    pub fn set_pc(&mut self, value: u32) {
        self.gpr[Self::PC] = value;
    }

    /// Switch to a new CPU mode, banking/restoring registers as needed
    pub fn switch_mode(&mut self, new_mode: CpuMode) {
        let old_mode = self.cpsr.mode();
        if old_mode == new_mode {
            return;
        }

        // Save current SP/LR to the old mode's bank
        let old_bank = old_mode.bank_index();
        self.sp_bank[old_bank] = self.gpr[Self::SP];
        self.lr_bank[old_bank] = self.gpr[Self::LR];

        // Handle FIQ R8-R12 banking
        if old_mode == CpuMode::Fiq {
            for i in 0..5 {
                self.fiq_bank[i] = self.gpr[8 + i];
                self.gpr[8 + i] = self.usr_bank_r8_r12[i];
            }
        } else if new_mode == CpuMode::Fiq {
            for i in 0..5 {
                self.usr_bank_r8_r12[i] = self.gpr[8 + i];
                self.gpr[8 + i] = self.fiq_bank[i];
            }
        }

        // Restore SP/LR from the new mode's bank
        let new_bank = new_mode.bank_index();
        self.gpr[Self::SP] = self.sp_bank[new_bank];
        self.gpr[Self::LR] = self.lr_bank[new_bank];

        // Update mode bits in CPSR
        self.cpsr.set_mode(new_mode);
    }

    /// Get SPSR for the current mode (None for User/System)
    pub fn spsr(&self) -> Option<Psr> {
        let mode = self.cpsr.mode();
        if mode.has_spsr() {
            // SPSR bank index: FIQ=0, IRQ=1, SVC=2, ABT=3, UND=4
            let idx = match mode {
                CpuMode::Fiq => 0,
                CpuMode::Irq => 1,
                CpuMode::Supervisor => 2,
                CpuMode::Abort => 3,
                CpuMode::Undefined => 4,
                _ => unreachable!(),
            };
            Some(self.spsr[idx])
        } else {
            None
        }
    }

    /// Set SPSR for the current mode
    pub fn set_spsr(&mut self, value: Psr) {
        let mode = self.cpsr.mode();
        if mode.has_spsr() {
            let idx = match mode {
                CpuMode::Fiq => 0,
                CpuMode::Irq => 1,
                CpuMode::Supervisor => 2,
                CpuMode::Abort => 3,
                CpuMode::Undefined => 4,
                _ => unreachable!(),
            };
            self.spsr[idx] = value;
        }
    }
}

// =============================================================================
// Memory Access Width
// =============================================================================

/// Access width for memory operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessWidth {
    Byte,
    Halfword,
    Word,
}

/// Memory access type (for wait state calculation)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    /// Non-sequential access (first access or random)
    NonSequential,
    /// Sequential access (contiguous after previous)
    Sequential,
}

// =============================================================================
// Emulator state
// =============================================================================

/// GBA key buttons
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GbaButton {
    A,
    B,
    Select,
    Start,
    Right,
    Left,
    Up,
    Down,
    R,
    L,
}

impl GbaButton {
    /// Bit position in KEYINPUT register (active-low)
    pub fn bit(self) -> u16 {
        match self {
            GbaButton::A => 1 << 0,
            GbaButton::B => 1 << 1,
            GbaButton::Select => 1 << 2,
            GbaButton::Start => 1 << 3,
            GbaButton::Right => 1 << 4,
            GbaButton::Left => 1 << 5,
            GbaButton::Up => 1 << 6,
            GbaButton::Down => 1 << 7,
            GbaButton::R => 1 << 8,
            GbaButton::L => 1 << 9,
        }
    }
}

/// Emulator speed control
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SpeedMode {
    /// Normal speed (1x, ~59.73 fps)
    Normal,
    /// Fast forward at given multiplier
    FastForward(f64),
    /// Unlimited speed (no frame limiter)
    Unlimited,
    /// Paused
    Paused,
}

impl Default for SpeedMode {
    fn default() -> Self {
        SpeedMode::Normal
    }
}

// =============================================================================
// Snapshot (Savestate) support
// =============================================================================

/// Trait for components that can be serialized for savestates
pub trait Snapshot: Serialize + for<'de> Deserialize<'de> {
    /// Create a snapshot of the current state
    fn save_snapshot(&self) -> Vec<u8>
    where
        Self: Sized,
    {
        // Default implementation uses bincode-like serialization
        // For now, we'll use a simple approach; real impl will use serde
        Vec::new()
    }

    /// Restore state from a snapshot
    fn load_snapshot(&mut self, data: &[u8]) -> Result<(), SnapshotError>
    where
        Self: Sized,
    {
        let _ = data;
        Ok(())
    }
}

#[derive(Debug)]
pub enum SnapshotError {
    InvalidData,
    VersionMismatch,
    DeserializationFailed(String),
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::InvalidData => write!(f, "Invalid snapshot data"),
            SnapshotError::VersionMismatch => write!(f, "Snapshot version mismatch"),
            SnapshotError::DeserializationFailed(e) => {
                write!(f, "Deserialization failed: {}", e)
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

// =============================================================================
// Memory inspection / manipulation
// =============================================================================

/// A watched memory address for RAM inspection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryWatch {
    pub address: u32,
    pub label: String,
    pub width: AccessWidth,
    pub last_value: u32,
}

impl serde::Serialize for AccessWidth {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            AccessWidth::Byte => serializer.serialize_u8(1),
            AccessWidth::Halfword => serializer.serialize_u8(2),
            AccessWidth::Word => serializer.serialize_u8(4),
        }
    }
}

impl<'de> serde::Deserialize<'de> for AccessWidth {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v = u8::deserialize(deserializer)?;
        match v {
            1 => Ok(AccessWidth::Byte),
            2 => Ok(AccessWidth::Halfword),
            4 => Ok(AccessWidth::Word),
            _ => Err(serde::de::Error::custom("invalid access width")),
        }
    }
}
