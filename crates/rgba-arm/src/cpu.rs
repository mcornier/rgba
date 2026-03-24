use rgba_core::bus::BusAccess;
use rgba_core::types::{CpuMode, Psr, RegisterFile};
use serde::{Deserialize, Serialize};

/// ARM7TDMI CPU state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arm7Tdmi {
    pub regs: RegisterFile,
    /// CPU is halted (waiting for interrupt)
    pub halted: bool,
    /// Pipeline: fetched instruction
    pub pipeline: [u32; 2],
    /// Total cycles executed
    pub cycles: u64,
}

impl Arm7Tdmi {
    pub fn new() -> Self {
        let mut cpu = Self {
            regs: RegisterFile::default(),
            halted: false,
            pipeline: [0; 2],
            cycles: 0,
        };
        // Power-on state: ARM mode, supervisor, IRQ+FIQ disabled
        cpu.regs.cpsr = Psr::from_bits_retain(
            CpuMode::Supervisor as u32 | Psr::I.bits() | Psr::F.bits(),
        );
        cpu.regs.set_pc(0x0800_0000); // Entry point (skip BIOS by default)
        cpu
    }

    /// Initialize CPU to boot from BIOS
    pub fn reset_to_bios(&mut self) {
        self.regs = RegisterFile::default();
        self.regs.cpsr = Psr::from_bits_retain(
            CpuMode::Supervisor as u32 | Psr::I.bits() | Psr::F.bits(),
        );
        self.regs.set_pc(0x0000_0000);
        self.halted = false;
        self.pipeline = [0; 2];
        self.cycles = 0;
    }

    /// Initialize CPU to skip BIOS (direct boot)
    pub fn reset_skip_bios(&mut self) {
        self.regs = RegisterFile::default();
        self.regs.cpsr = Psr::from_bits_retain(CpuMode::System as u32);
        self.regs.set_pc(0x0800_0000);
        // Set up stack pointers like BIOS would
        self.regs.gpr[RegisterFile::SP] = 0x0300_7F00; // System/User SP
        self.regs.sp_bank[CpuMode::Irq.bank_index()] = 0x0300_7FA0; // IRQ SP
        self.regs.sp_bank[CpuMode::Supervisor.bank_index()] = 0x0300_7FE0; // SVC SP
        self.halted = false;
        self.pipeline = [0; 2];
        self.cycles = 0;
    }

    /// Step the CPU by one instruction, returns cycles consumed
    pub fn step(&mut self, bus: &mut impl BusAccess) -> u32 {
        if self.halted {
            return 1;
        }

        if self.regs.cpsr.thumb() {
            self.step_thumb(bus)
        } else {
            self.step_arm(bus)
        }
    }

    /// Execute one ARM instruction
    fn step_arm(&mut self, bus: &mut impl BusAccess) -> u32 {
        let pc = self.regs.pc();
        let instruction = bus.read_word(pc);
        self.regs.set_pc(pc.wrapping_add(4));

        let cycles = self.execute_arm(instruction, bus);
        self.cycles += cycles as u64;
        cycles
    }

    /// Execute one THUMB instruction
    fn step_thumb(&mut self, bus: &mut impl BusAccess) -> u32 {
        let pc = self.regs.pc();
        let instruction = bus.read_halfword(pc) as u32;
        self.regs.set_pc(pc.wrapping_add(2));

        let cycles = self.execute_thumb(instruction, bus);
        self.cycles += cycles as u64;
        cycles
    }

    /// Check ARM condition code
    pub fn check_condition(&self, cond: u32) -> bool {
        let cpsr = self.regs.cpsr;
        match cond {
            0x0 => cpsr.contains(Psr::Z),                              // EQ
            0x1 => !cpsr.contains(Psr::Z),                             // NE
            0x2 => cpsr.contains(Psr::C),                              // CS/HS
            0x3 => !cpsr.contains(Psr::C),                             // CC/LO
            0x4 => cpsr.contains(Psr::N),                              // MI
            0x5 => !cpsr.contains(Psr::N),                             // PL
            0x6 => cpsr.contains(Psr::V),                              // VS
            0x7 => !cpsr.contains(Psr::V),                             // VC
            0x8 => cpsr.contains(Psr::C) && !cpsr.contains(Psr::Z),   // HI
            0x9 => !cpsr.contains(Psr::C) || cpsr.contains(Psr::Z),   // LS
            0xA => cpsr.contains(Psr::N) == cpsr.contains(Psr::V),    // GE
            0xB => cpsr.contains(Psr::N) != cpsr.contains(Psr::V),    // LT
            0xC => {                                                    // GT
                !cpsr.contains(Psr::Z)
                    && (cpsr.contains(Psr::N) == cpsr.contains(Psr::V))
            }
            0xD => {                                                    // LE
                cpsr.contains(Psr::Z)
                    || (cpsr.contains(Psr::N) != cpsr.contains(Psr::V))
            }
            0xE => true,                                                // AL (always)
            0xF => true,                                                // NV (ARMv4: never, treated as always)
            _ => unreachable!(),
        }
    }

    /// Trigger an IRQ exception
    pub fn trigger_irq(&mut self) {
        if self.regs.cpsr.irq_disabled() {
            return;
        }

        let cpsr = self.regs.cpsr;
        let return_addr = if cpsr.thumb() {
            self.regs.pc().wrapping_add(2)
        } else {
            self.regs.pc().wrapping_add(4)
        };

        // Switch to IRQ mode
        self.regs.switch_mode(CpuMode::Irq);
        self.regs.set_spsr(cpsr);
        self.regs.gpr[RegisterFile::LR] = return_addr;

        // Set IRQ disable, clear Thumb
        self.regs.cpsr.insert(Psr::I);
        self.regs.cpsr.remove(Psr::T);

        // Jump to IRQ vector
        self.regs.set_pc(0x0000_0018);
        self.halted = false;
    }

    /// Execute an ARM instruction
    fn execute_arm(&mut self, instruction: u32, bus: &mut impl BusAccess) -> u32 {
        self.execute_arm_instruction(instruction, bus)
    }

    /// Execute a THUMB instruction (stub — implemented in thumb.rs)
    fn execute_thumb(&mut self, _instruction: u32, _bus: &mut impl BusAccess) -> u32 {
        // TODO: Full THUMB instruction execution (US-05)
        1
    }
}

impl Default for Arm7Tdmi {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_init() {
        let cpu = Arm7Tdmi::new();
        assert_eq!(cpu.regs.pc(), 0x0800_0000);
        assert!(!cpu.regs.cpsr.thumb());
        assert!(cpu.regs.cpsr.irq_disabled());
    }

    #[test]
    fn test_skip_bios_init() {
        let mut cpu = Arm7Tdmi::new();
        cpu.reset_skip_bios();
        assert_eq!(cpu.regs.pc(), 0x0800_0000);
        assert_eq!(cpu.regs.gpr[RegisterFile::SP], 0x0300_7F00);
        assert!(!cpu.regs.cpsr.irq_disabled());
    }

    #[test]
    fn test_condition_codes() {
        let cpu = Arm7Tdmi::new();
        assert!(cpu.check_condition(0xE)); // Always

        // Default CPSR has no flags set
        assert!(!cpu.check_condition(0x0)); // EQ (Z=0)
        assert!(cpu.check_condition(0x1));  // NE (Z=0)
    }
}
