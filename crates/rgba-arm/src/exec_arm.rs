use rgba_core::bus::BusAccess;
use rgba_core::types::{CpuMode, Psr, RegisterFile};

use crate::arm::*;
use crate::cpu::Arm7Tdmi;

// =============================================================================
// ARM Instruction Execution
// =============================================================================

impl Arm7Tdmi {
    /// Execute a decoded ARM instruction, returns cycles consumed
    pub fn execute_arm_instruction(
        &mut self,
        instruction: u32,
        bus: &mut impl BusAccess,
    ) -> u32 {
        // Check condition code
        let cond = instruction >> 28;
        if !self.check_condition(cond) {
            return 1; // 1S cycle for failed condition
        }

        let decoded = Self::decode_arm(instruction);
        match decoded {
            ArmInstruction::BranchExchange { rn } => self.exec_bx(rn),
            ArmInstruction::Branch { link, offset } => self.exec_branch(link, offset),
            ArmInstruction::DataProcessing {
                opcode,
                set_flags,
                rn,
                rd,
                operand2,
            } => self.exec_data_processing(opcode, set_flags, rn, rd, operand2),
            ArmInstruction::Multiply {
                accumulate,
                set_flags,
                rd,
                rn,
                rs,
                rm,
            } => self.exec_multiply(accumulate, set_flags, rd, rn, rs, rm),
            ArmInstruction::MultiplyLong {
                signed,
                accumulate,
                set_flags,
                rd_hi,
                rd_lo,
                rs,
                rm,
            } => self.exec_multiply_long(signed, accumulate, set_flags, rd_hi, rd_lo, rs, rm),
            ArmInstruction::SingleTransfer {
                load,
                byte,
                write_back,
                up,
                pre,
                rn,
                rd,
                offset,
            } => self.exec_single_transfer(load, byte, write_back, up, pre, rn, rd, offset, bus),
            ArmInstruction::HalfwordTransfer {
                load,
                write_back,
                up,
                pre,
                signed,
                half,
                rn,
                rd,
                offset,
            } => self.exec_halfword_transfer(
                load, write_back, up, pre, signed, half, rn, rd, offset, bus,
            ),
            ArmInstruction::BlockTransfer {
                load,
                write_back,
                up,
                pre,
                s_bit,
                rn,
                register_list,
            } => self.exec_block_transfer(load, write_back, up, pre, s_bit, rn, register_list, bus),
            ArmInstruction::Swap {
                byte,
                rn,
                rd,
                rm,
            } => self.exec_swap(byte, rn, rd, rm, bus),
            ArmInstruction::Mrs { spsr, rd } => self.exec_mrs(spsr, rd),
            ArmInstruction::Msr {
                spsr,
                field_mask,
                operand,
            } => self.exec_msr(spsr, field_mask, operand),
            ArmInstruction::Swi { comment } => self.exec_swi(comment),
            ArmInstruction::Undefined => self.exec_undefined(),
        }
    }

    // =========================================================================
    // Branch
    // =========================================================================

    fn exec_bx(&mut self, rn: usize) -> u32 {
        let addr = self.regs.gpr[rn];
        if addr & 1 != 0 {
            // Switch to THUMB
            self.regs.cpsr.insert(Psr::T);
            self.regs.set_pc(addr & !1);
        } else {
            self.regs.cpsr.remove(Psr::T);
            self.regs.set_pc(addr & !3);
        }
        3 // 2S + 1N
    }

    fn exec_branch(&mut self, link: bool, offset: i32) -> u32 {
        if link {
            // BL: save return address
            self.regs.gpr[RegisterFile::LR] = self.regs.pc().wrapping_sub(4);
        }
        let pc = self.regs.pc();
        self.regs.set_pc((pc as i32).wrapping_add(offset) as u32);
        3 // 2S + 1N
    }

    // =========================================================================
    // Data Processing (ALU)
    // =========================================================================

    fn exec_data_processing(
        &mut self,
        opcode: AluOp,
        set_flags: bool,
        rn: usize,
        rd: usize,
        operand2: ShifterOperand,
    ) -> u32 {
        let (op2, shifter_carry) = self.resolve_operand(operand2);
        let op1 = self.reg_value(rn);
        let carry = self.regs.cpsr.contains(Psr::C);

        let (result, new_carry, new_overflow) = match opcode {
            AluOp::And | AluOp::Tst => (op1 & op2, shifter_carry, false),
            AluOp::Eor | AluOp::Teq => (op1 ^ op2, shifter_carry, false),
            AluOp::Sub | AluOp::Cmp => {
                let (r, borrow) = op1.overflowing_sub(op2);
                let v = ((op1 ^ op2) & (op1 ^ r)) >> 31 != 0;
                (r, !borrow, v)
            }
            AluOp::Rsb => {
                let (r, borrow) = op2.overflowing_sub(op1);
                let v = ((op2 ^ op1) & (op2 ^ r)) >> 31 != 0;
                (r, !borrow, v)
            }
            AluOp::Add | AluOp::Cmn => {
                let (r, overflow) = op1.overflowing_add(op2);
                let v = (!(op1 ^ op2) & (op1 ^ r)) >> 31 != 0;
                (r, overflow, v)
            }
            AluOp::Adc => {
                let c = carry as u32;
                let (r1, c1) = op1.overflowing_add(op2);
                let (r2, c2) = r1.overflowing_add(c);
                let v = (!(op1 ^ op2) & (op1 ^ r2)) >> 31 != 0;
                (r2, c1 || c2, v)
            }
            AluOp::Sbc => {
                let c = carry as u32;
                let (r1, b1) = op1.overflowing_sub(op2);
                let (r2, b2) = r1.overflowing_sub(1 - c);
                let v = ((op1 ^ op2) & (op1 ^ r2)) >> 31 != 0;
                (r2, !(b1 || b2), v)
            }
            AluOp::Rsc => {
                let c = carry as u32;
                let (r1, b1) = op2.overflowing_sub(op1);
                let (r2, b2) = r1.overflowing_sub(1 - c);
                let v = ((op2 ^ op1) & (op2 ^ r2)) >> 31 != 0;
                (r2, !(b1 || b2), v)
            }
            AluOp::Orr => (op1 | op2, shifter_carry, false),
            AluOp::Mov => (op2, shifter_carry, false),
            AluOp::Bic => (op1 & !op2, shifter_carry, false),
            AluOp::Mvn => (!op2, shifter_carry, false),
        };

        // Write result (not for test/compare ops)
        if !opcode.is_test() {
            if rd == 15 {
                if set_flags {
                    // Restore CPSR from SPSR
                    if let Some(spsr) = self.regs.spsr() {
                        let old_mode = self.regs.cpsr.mode();
                        self.regs.cpsr = spsr;
                        let new_mode = spsr.mode();
                        if old_mode != new_mode {
                            self.regs.switch_mode(new_mode);
                        }
                    }
                }
                self.regs.set_pc(result & !3);
                return 3; // branch penalty
            } else {
                self.regs.gpr[rd] = result;
            }
        }

        // Update flags
        if set_flags && rd != 15 {
            self.regs.cpsr.set(Psr::N, (result >> 31) != 0);
            self.regs.cpsr.set(Psr::Z, result == 0);
            if opcode.is_logical() {
                self.regs.cpsr.set(Psr::C, new_carry);
            } else {
                self.regs.cpsr.set(Psr::C, new_carry);
                self.regs.cpsr.set(Psr::V, new_overflow);
            }
        }

        // Cycle counting
        let extra = match operand2 {
            ShifterOperand::RegisterReg { .. } => 1, // +1I for register shift
            _ => 0,
        };
        1 + extra
    }

    // =========================================================================
    // Multiply
    // =========================================================================

    fn exec_multiply(
        &mut self,
        accumulate: bool,
        set_flags: bool,
        rd: usize,
        rn: usize,
        rs: usize,
        rm: usize,
    ) -> u32 {
        let result = if accumulate {
            self.regs.gpr[rm]
                .wrapping_mul(self.regs.gpr[rs])
                .wrapping_add(self.regs.gpr[rn])
        } else {
            self.regs.gpr[rm].wrapping_mul(self.regs.gpr[rs])
        };

        self.regs.gpr[rd] = result;

        if set_flags {
            self.regs.cpsr.set(Psr::N, (result >> 31) != 0);
            self.regs.cpsr.set(Psr::Z, result == 0);
            // C is destroyed (unpredictable), V is unaffected
        }

        // Multiply cycles: 1S + mI (m = 1-4 based on Rs magnitude)
        let m = multiply_cycles(self.regs.gpr[rs]);
        1 + m + if accumulate { 1 } else { 0 }
    }

    fn exec_multiply_long(
        &mut self,
        signed: bool,
        accumulate: bool,
        set_flags: bool,
        rd_hi: usize,
        rd_lo: usize,
        rs: usize,
        rm: usize,
    ) -> u32 {
        let result: u64 = if signed {
            let a = self.regs.gpr[rm] as i32 as i64;
            let b = self.regs.gpr[rs] as i32 as i64;
            if accumulate {
                let acc = ((self.regs.gpr[rd_hi] as u64) << 32) | self.regs.gpr[rd_lo] as u64;
                (a.wrapping_mul(b) as u64).wrapping_add(acc)
            } else {
                a.wrapping_mul(b) as u64
            }
        } else {
            let a = self.regs.gpr[rm] as u64;
            let b = self.regs.gpr[rs] as u64;
            if accumulate {
                let acc = ((self.regs.gpr[rd_hi] as u64) << 32) | self.regs.gpr[rd_lo] as u64;
                a.wrapping_mul(b).wrapping_add(acc)
            } else {
                a.wrapping_mul(b)
            }
        };

        self.regs.gpr[rd_lo] = result as u32;
        self.regs.gpr[rd_hi] = (result >> 32) as u32;

        if set_flags {
            self.regs.cpsr.set(Psr::N, (result >> 63) != 0);
            self.regs.cpsr.set(Psr::Z, result == 0);
        }

        let m = multiply_cycles(self.regs.gpr[rs]);
        1 + m + 1 + if accumulate { 1 } else { 0 }
    }

    // =========================================================================
    // Single Data Transfer (LDR/STR)
    // =========================================================================

    fn exec_single_transfer(
        &mut self,
        load: bool,
        byte: bool,
        write_back: bool,
        up: bool,
        pre: bool,
        rn: usize,
        rd: usize,
        offset: TransferOffset,
        bus: &mut impl BusAccess,
    ) -> u32 {
        let base = self.reg_value(rn);
        let offset_val = self.resolve_transfer_offset(offset);
        let offset_addr = if up {
            base.wrapping_add(offset_val)
        } else {
            base.wrapping_sub(offset_val)
        };

        let addr = if pre { offset_addr } else { base };

        if load {
            let value = if byte {
                bus.read_byte(addr) as u32
            } else {
                // Word loads are rotated for misaligned addresses
                let aligned = addr & !3;
                let rotation = (addr & 3) * 8;
                let val = bus.read_word(aligned);
                val.rotate_right(rotation)
            };

            if rd == 15 {
                self.regs.set_pc(value & !3);
            } else {
                self.regs.gpr[rd] = value;
            }
        } else {
            let value = if rd == 15 {
                self.regs.pc().wrapping_add(4)
            } else {
                self.regs.gpr[rd]
            };

            if byte {
                bus.write_byte(addr, value as u8);
            } else {
                bus.write_word(addr & !3, value);
            }
        }

        // Write-back (or post-indexed)
        if !pre || write_back {
            let wb = if pre { offset_addr } else { offset_addr };
            if rn != rd || !load {
                self.regs.gpr[rn] = wb;
            }
        }

        if load {
            if rd == 15 { 5 } else { 3 } // 1S + 1N + 1I (+ branch penalty)
        } else {
            2 // 2N
        }
    }

    fn resolve_transfer_offset(&self, offset: TransferOffset) -> u32 {
        match offset {
            TransferOffset::Immediate(imm) => imm as u32,
            TransferOffset::Register {
                rm,
                shift_type,
                amount,
            } => {
                let carry = self.regs.cpsr.contains(Psr::C);
                let (result, _) =
                    Self::barrel_shift(self.regs.gpr[rm], shift_type, amount, carry);
                result
            }
        }
    }

    // =========================================================================
    // Halfword / Signed Data Transfer
    // =========================================================================

    fn exec_halfword_transfer(
        &mut self,
        load: bool,
        write_back: bool,
        up: bool,
        pre: bool,
        signed: bool,
        half: bool,
        rn: usize,
        rd: usize,
        offset: HalfwordOffset,
        bus: &mut impl BusAccess,
    ) -> u32 {
        let base = self.reg_value(rn);
        let offset_val = match offset {
            HalfwordOffset::Immediate(imm) => imm as u32,
            HalfwordOffset::Register(rm) => self.regs.gpr[rm],
        };
        let offset_addr = if up {
            base.wrapping_add(offset_val)
        } else {
            base.wrapping_sub(offset_val)
        };

        let addr = if pre { offset_addr } else { base };

        if load {
            let value = if signed && !half {
                // LDRSB: signed byte
                bus.read_byte(addr) as i8 as i32 as u32
            } else if signed && half {
                // LDRSH: signed halfword
                if addr & 1 != 0 {
                    // Misaligned LDRSH loads as signed byte
                    bus.read_byte(addr) as i8 as i32 as u32
                } else {
                    bus.read_halfword(addr) as i16 as i32 as u32
                }
            } else {
                // LDRH: unsigned halfword
                if addr & 1 != 0 {
                    let val = bus.read_halfword(addr & !1);
                    val.rotate_right(8) as u32
                } else {
                    bus.read_halfword(addr) as u32
                }
            };
            self.regs.gpr[rd] = value;
        } else {
            // STRH
            let value = if rd == 15 {
                self.regs.pc().wrapping_add(4)
            } else {
                self.regs.gpr[rd]
            };
            bus.write_halfword(addr & !1, value as u16);
        }

        if !pre || write_back {
            if rn != rd || !load {
                self.regs.gpr[rn] = offset_addr;
            }
        }

        if load { 3 } else { 2 }
    }

    // =========================================================================
    // Block Data Transfer (LDM/STM)
    // =========================================================================

    fn exec_block_transfer(
        &mut self,
        load: bool,
        write_back: bool,
        up: bool,
        pre: bool,
        s_bit: bool,
        rn: usize,
        register_list: u16,
        bus: &mut impl BusAccess,
    ) -> u32 {
        let base = self.regs.gpr[rn];
        let count = register_list.count_ones();

        if count == 0 {
            // Empty register list: special behavior (transfers PC, base += 0x40)
            if load {
                let val = bus.read_word(base);
                self.regs.set_pc(val & !3);
            } else {
                bus.write_word(base, self.regs.pc().wrapping_add(4));
            }
            if write_back {
                self.regs.gpr[rn] = if up {
                    base.wrapping_add(0x40)
                } else {
                    base.wrapping_sub(0x40)
                };
            }
            return if load { 5 } else { 2 };
        }

        // Calculate start address
        let mut addr = if up {
            if pre { base.wrapping_add(4) } else { base }
        } else {
            let total = count * 4;
            if pre {
                base.wrapping_sub(total)
            } else {
                base.wrapping_sub(total).wrapping_add(4)
            }
        };

        let user_mode = s_bit && (!load || register_list & (1 << 15) == 0);
        let old_mode = self.regs.cpsr.mode();

        if user_mode {
            self.regs.switch_mode(CpuMode::User);
        }

        let mut cycles = 0u32;

        for i in 0..16 {
            if register_list & (1 << i) != 0 {
                if load {
                    let val = bus.read_word(addr & !3);
                    if i == 15 {
                        self.regs.set_pc(val & !3);
                        if s_bit {
                            // Restore CPSR from SPSR
                            if user_mode {
                                self.regs.switch_mode(old_mode);
                            }
                            if let Some(spsr) = self.regs.spsr() {
                                self.regs.cpsr = spsr;
                                let new_mode = spsr.mode();
                                if old_mode != new_mode {
                                    self.regs.switch_mode(new_mode);
                                }
                            }
                        }
                        cycles += 2; // branch penalty
                    } else {
                        self.regs.gpr[i] = val;
                    }
                } else {
                    let val = if i == 15 {
                        self.regs.pc().wrapping_add(4)
                    } else {
                        self.regs.gpr[i]
                    };
                    bus.write_word(addr & !3, val);
                }
                addr = addr.wrapping_add(4);
                cycles += 1;
            }
        }

        if user_mode && !(s_bit && load && register_list & (1 << 15) != 0) {
            self.regs.switch_mode(old_mode);
        }

        if write_back {
            self.regs.gpr[rn] = if up {
                base.wrapping_add(count * 4)
            } else {
                base.wrapping_sub(count * 4)
            };
        }

        cycles + 1 // +1 for final internal cycle
    }

    // =========================================================================
    // Swap (SWP/SWPB)
    // =========================================================================

    fn exec_swap(
        &mut self,
        byte: bool,
        rn: usize,
        rd: usize,
        rm: usize,
        bus: &mut impl BusAccess,
    ) -> u32 {
        let addr = self.regs.gpr[rn];

        if byte {
            let old = bus.read_byte(addr) as u32;
            bus.write_byte(addr, self.regs.gpr[rm] as u8);
            self.regs.gpr[rd] = old;
        } else {
            let aligned = addr & !3;
            let rotation = (addr & 3) * 8;
            let old = bus.read_word(aligned).rotate_right(rotation);
            bus.write_word(aligned, self.regs.gpr[rm]);
            self.regs.gpr[rd] = old;
        }

        4 // 1S + 2N + 1I
    }

    // =========================================================================
    // Status Register Access (MRS/MSR)
    // =========================================================================

    fn exec_mrs(&mut self, spsr: bool, rd: usize) -> u32 {
        let value = if spsr {
            self.regs.spsr().unwrap_or(self.regs.cpsr).bits()
        } else {
            self.regs.cpsr.bits()
        };
        self.regs.gpr[rd] = value;
        1
    }

    fn exec_msr(&mut self, spsr: bool, field_mask: u8, operand: MsrOperand) -> u32 {
        let value = match operand {
            MsrOperand::Immediate(imm) => imm,
            MsrOperand::Register(rm) => self.regs.gpr[rm],
        };

        // Build mask from field bits
        let mut mask = 0u32;
        if field_mask & 1 != 0 {
            mask |= 0x0000_00FF;
        } // control
        if field_mask & 2 != 0 {
            mask |= 0x0000_FF00;
        } // extension
        if field_mask & 4 != 0 {
            mask |= 0x00FF_0000;
        } // status
        if field_mask & 8 != 0 {
            mask |= 0xFF00_0000;
        } // flags

        // User mode can only modify flags
        if self.regs.cpsr.mode() == CpuMode::User {
            mask &= 0xFF00_0000;
        }

        if spsr {
            if let Some(old_spsr) = self.regs.spsr() {
                let new_val = (old_spsr.bits() & !mask) | (value & mask);
                self.regs.set_spsr(Psr::from_bits_retain(new_val));
            }
        } else {
            let old = self.regs.cpsr.bits();
            let new_val = (old & !mask) | (value & mask);
            let new_psr = Psr::from_bits_retain(new_val);

            // Mode change?
            let old_mode = self.regs.cpsr.mode();
            let new_mode = new_psr.mode();
            self.regs.cpsr = new_psr;
            if old_mode != new_mode {
                self.regs.switch_mode(new_mode);
            }
        }

        1
    }

    // =========================================================================
    // Software Interrupt
    // =========================================================================

    fn exec_swi(&mut self, _comment: u32) -> u32 {
        let cpsr = self.regs.cpsr;
        let return_addr = self.regs.pc().wrapping_sub(4);

        self.regs.switch_mode(CpuMode::Supervisor);
        self.regs.set_spsr(cpsr);
        self.regs.gpr[RegisterFile::LR] = return_addr;

        self.regs.cpsr.insert(Psr::I);
        self.regs.cpsr.remove(Psr::T);
        self.regs.set_pc(0x0000_0008);

        3
    }

    fn exec_undefined(&mut self) -> u32 {
        let cpsr = self.regs.cpsr;
        let return_addr = self.regs.pc().wrapping_sub(4);

        self.regs.switch_mode(CpuMode::Undefined);
        self.regs.set_spsr(cpsr);
        self.regs.gpr[RegisterFile::LR] = return_addr;

        self.regs.cpsr.insert(Psr::I);
        self.regs.cpsr.remove(Psr::T);
        self.regs.set_pc(0x0000_0004);

        3
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    /// Get register value (PC reads as PC+8 in ARM, PC+4 in Thumb)
    fn reg_value(&self, reg: usize) -> u32 {
        if reg == 15 {
            if self.regs.cpsr.thumb() {
                self.regs.pc().wrapping_add(2)
            } else {
                self.regs.pc().wrapping_add(4)
            }
        } else {
            self.regs.gpr[reg]
        }
    }
}

/// Calculate multiply instruction cycle count based on Rs value
pub fn multiply_cycles(rs: u32) -> u32 {
    if rs & 0xFFFF_FF00 == 0 || rs & 0xFFFF_FF00 == 0xFFFF_FF00 {
        1
    } else if rs & 0xFFFF_0000 == 0 || rs & 0xFFFF_0000 == 0xFFFF_0000 {
        2
    } else if rs & 0xFF00_0000 == 0 || rs & 0xFF00_0000 == 0xFF00_0000 {
        3
    } else {
        4
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rgba_core::bus::Bus;

    fn make_cpu() -> Arm7Tdmi {
        let mut cpu = Arm7Tdmi::new();
        cpu.reset_skip_bios();
        cpu
    }

    #[test]
    fn test_exec_mov_imm() {
        let mut cpu = make_cpu();
        let mut bus = Bus::new();
        // MOV R0, #42
        let instr = 0xE3A0_002A;
        cpu.execute_arm_instruction(instr, &mut bus);
        assert_eq!(cpu.regs.gpr[0], 42);
    }

    #[test]
    fn test_exec_add() {
        let mut cpu = make_cpu();
        let mut bus = Bus::new();
        cpu.regs.gpr[1] = 10;
        cpu.regs.gpr[2] = 20;
        // ADD R0, R1, R2
        let instr = 0xE081_0002;
        cpu.execute_arm_instruction(instr, &mut bus);
        assert_eq!(cpu.regs.gpr[0], 30);
    }

    #[test]
    fn test_exec_sub_flags() {
        let mut cpu = make_cpu();
        let mut bus = Bus::new();
        cpu.regs.gpr[0] = 5;
        cpu.regs.gpr[1] = 5;
        // SUBS R2, R0, R1
        let instr = 0xE050_2001;
        cpu.execute_arm_instruction(instr, &mut bus);
        assert_eq!(cpu.regs.gpr[2], 0);
        assert!(cpu.regs.cpsr.contains(Psr::Z));
        assert!(cpu.regs.cpsr.contains(Psr::C)); // no borrow
    }

    #[test]
    fn test_exec_cmp() {
        let mut cpu = make_cpu();
        let mut bus = Bus::new();
        cpu.regs.gpr[0] = 10;
        // CMP R0, #5
        let instr = 0xE350_0005;
        cpu.execute_arm_instruction(instr, &mut bus);
        assert!(!cpu.regs.cpsr.contains(Psr::Z));
        assert!(cpu.regs.cpsr.contains(Psr::C)); // 10 >= 5
    }

    #[test]
    fn test_exec_str_ldr() {
        let mut cpu = make_cpu();
        let mut bus = Bus::new();
        cpu.regs.gpr[0] = 0xDEAD_BEEF;
        cpu.regs.gpr[1] = 0x0200_0000; // EWRAM

        // STR R0, [R1]
        let str_instr = 0xE581_0000;
        cpu.execute_arm_instruction(str_instr, &mut bus);
        assert_eq!(bus.read_word(0x0200_0000), 0xDEAD_BEEF);

        // LDR R2, [R1]
        let ldr_instr = 0xE591_2000;
        cpu.execute_arm_instruction(ldr_instr, &mut bus);
        assert_eq!(cpu.regs.gpr[2], 0xDEAD_BEEF);
    }

    #[test]
    fn test_exec_str_ldr_offset() {
        let mut cpu = make_cpu();
        let mut bus = Bus::new();
        cpu.regs.gpr[0] = 0x1234;
        cpu.regs.gpr[1] = 0x0200_0000;

        // STR R0, [R1, #8]
        let instr = 0xE581_0008;
        cpu.execute_arm_instruction(instr, &mut bus);
        assert_eq!(bus.read_word(0x0200_0008), 0x1234);
    }

    #[test]
    fn test_exec_branch() {
        let mut cpu = make_cpu();
        let mut bus = Bus::new();
        let pc_before = cpu.regs.pc();
        // B +8: offset field = 2, shifted = 8, applied to PC which is already +4 from fetch
        let instr = 0xEA00_0002;
        cpu.execute_arm_instruction(instr, &mut bus);
        // PC was advanced by step_arm (+4), then branch adds offset to that
        // In execute_arm_instruction, PC = pc_before (no pre-advance here, that's in step)
        // exec_branch: pc = self.regs.pc() (= pc_before) + offset(8) = pc_before + 8
        assert_eq!(cpu.regs.pc(), pc_before.wrapping_add(8));
    }

    #[test]
    fn test_exec_branch_link() {
        let mut cpu = make_cpu();
        let mut bus = Bus::new();
        let pc_before = cpu.regs.pc();
        // BL +0
        let instr = 0xEB00_0000;
        cpu.execute_arm_instruction(instr, &mut bus);
        // LR should be old PC - 4
        assert_eq!(cpu.regs.gpr[RegisterFile::LR], pc_before.wrapping_sub(4));
    }

    #[test]
    fn test_exec_bx_thumb() {
        let mut cpu = make_cpu();
        cpu.regs.gpr[0] = 0x0800_0001; // bit 0 set = THUMB

        let instr = 0xE12F_FF10; // BX R0
        let mut bus = Bus::new();
        cpu.execute_arm_instruction(instr, &mut bus);

        assert!(cpu.regs.cpsr.thumb());
        assert_eq!(cpu.regs.pc(), 0x0800_0000);
    }

    #[test]
    fn test_exec_multiply() {
        let mut cpu = make_cpu();
        let mut bus = Bus::new();
        cpu.regs.gpr[1] = 7;
        cpu.regs.gpr[2] = 6;
        // MUL R0, R1, R2
        let instr = 0xE000_0291;
        cpu.execute_arm_instruction(instr, &mut bus);
        assert_eq!(cpu.regs.gpr[0], 42);
    }

    #[test]
    fn test_exec_swp() {
        let mut cpu = make_cpu();
        let mut bus = Bus::new();
        cpu.regs.gpr[0] = 0x0200_0000; // address
        cpu.regs.gpr[1] = 0xAAAA_BBBB; // value to swap in
        bus.write_word(0x0200_0000, 0xCCCC_DDDD); // existing value

        // SWP R2, R1, [R0]
        let instr = 0xE100_2091;
        cpu.execute_arm_instruction(instr, &mut bus);

        assert_eq!(cpu.regs.gpr[2], 0xCCCC_DDDD); // read old value
        assert_eq!(bus.read_word(0x0200_0000), 0xAAAA_BBBB); // wrote new value
    }

    #[test]
    fn test_exec_ldm_stm() {
        let mut cpu = make_cpu();
        let mut bus = Bus::new();
        cpu.regs.gpr[0] = 1;
        cpu.regs.gpr[1] = 2;
        cpu.regs.gpr[2] = 3;
        cpu.regs.gpr[13] = 0x0300_7F00;

        // STMDB SP!, {R0-R2} (push)
        let stm = 0xE92D_0007;
        cpu.execute_arm_instruction(stm, &mut bus);
        assert_eq!(cpu.regs.gpr[13], 0x0300_7F00 - 12);

        // Clear registers
        cpu.regs.gpr[0] = 0;
        cpu.regs.gpr[1] = 0;
        cpu.regs.gpr[2] = 0;

        // LDMIA SP!, {R0-R2} (pop)
        let ldm = 0xE8BD_0007;
        cpu.execute_arm_instruction(ldm, &mut bus);
        assert_eq!(cpu.regs.gpr[0], 1);
        assert_eq!(cpu.regs.gpr[1], 2);
        assert_eq!(cpu.regs.gpr[2], 3);
        assert_eq!(cpu.regs.gpr[13], 0x0300_7F00);
    }

    #[test]
    fn test_exec_mrs_msr() {
        let mut cpu = make_cpu();
        let mut bus = Bus::new();
        cpu.regs.cpsr.insert(Psr::N);
        cpu.regs.cpsr.insert(Psr::Z);

        // MRS R0, CPSR
        let mrs = 0xE10F_0000;
        cpu.execute_arm_instruction(mrs, &mut bus);
        assert_eq!(cpu.regs.gpr[0], cpu.regs.cpsr.bits());
    }

    #[test]
    fn test_exec_condition_fail() {
        let mut cpu = make_cpu();
        let mut bus = Bus::new();
        cpu.regs.gpr[0] = 99;
        // MOVEQ R0, #0 (condition EQ, but Z=0 so should not execute)
        let instr = 0x03A0_0000;
        cpu.execute_arm_instruction(instr, &mut bus);
        assert_eq!(cpu.regs.gpr[0], 99); // unchanged
    }

    #[test]
    fn test_exec_swi() {
        let mut cpu = make_cpu();
        let mut bus = Bus::new();
        let old_pc = cpu.regs.pc();
        // SWI #0
        let instr = 0xEF00_0000;
        cpu.execute_arm_instruction(instr, &mut bus);
        assert_eq!(cpu.regs.cpsr.mode(), CpuMode::Supervisor);
        assert_eq!(cpu.regs.pc(), 0x0000_0008);
        assert!(cpu.regs.cpsr.irq_disabled());
        // LR should point to instruction after SWI
        assert_eq!(cpu.regs.gpr[RegisterFile::LR], old_pc.wrapping_sub(4));
    }
}
