use rgba_core::bus::BusAccess;
use rgba_core::types::{CpuMode, Psr, RegisterFile};

use crate::arm::ShiftType;
use crate::cpu::Arm7Tdmi;

// =============================================================================
// THUMB Instruction Execution
// =============================================================================

impl Arm7Tdmi {
    /// Execute a 16-bit THUMB instruction, returns cycles consumed
    pub fn execute_thumb_instruction(
        &mut self,
        instruction: u32,
        bus: &mut impl BusAccess,
    ) -> u32 {
        let op = instruction >> 8;

        match instruction >> 13 {
            0b000 => {
                if (instruction >> 11) & 3 == 3 {
                    // Format 2: Add/subtract
                    self.thumb_add_sub(instruction)
                } else {
                    // Format 1: Move shifted register
                    self.thumb_shift(instruction)
                }
            }
            0b001 => {
                // Format 3: Move/compare/add/subtract immediate
                self.thumb_imm_op(instruction)
            }
            0b010 => {
                if (instruction >> 10) & 7 == 0b000 {
                    // Format 4: ALU operations
                    self.thumb_alu(instruction)
                } else if (instruction >> 10) & 3 == 0b01 {
                    // Format 5: Hi register operations / BX
                    self.thumb_hi_reg_bx(instruction)
                } else if (instruction >> 11) & 1 == 1 {
                    // Format 6: PC-relative load
                    self.thumb_pc_load(instruction, bus)
                } else {
                    // Format 7/8: Load/store with register offset
                    self.thumb_load_store_reg(instruction, bus)
                }
            }
            0b011 => {
                // Format 9: Load/store with immediate offset
                self.thumb_load_store_imm(instruction, bus)
            }
            0b100 => {
                if (instruction >> 12) & 1 == 0 {
                    // Format 10: Load/store halfword
                    self.thumb_load_store_half(instruction, bus)
                } else {
                    // Format 11: SP-relative load/store
                    self.thumb_sp_load_store(instruction, bus)
                }
            }
            0b101 => {
                if (instruction >> 12) & 1 == 0 {
                    // Format 12: Load address (PC/SP + offset)
                    self.thumb_load_address(instruction)
                } else if (op & 0xFF) == 0b10110000 >> 0
                    || (instruction >> 8) & 0xFF == 0xB0
                {
                    // Format 13: Add offset to SP
                    self.thumb_sp_offset(instruction)
                } else {
                    // Format 14: Push/pop registers
                    self.thumb_push_pop(instruction, bus)
                }
            }
            0b110 => {
                if (instruction >> 12) & 1 == 0 {
                    // Format 15: Multiple load/store
                    self.thumb_multi_load_store(instruction, bus)
                } else if ((instruction >> 8) & 0xF) == 0xF {
                    // Format 17: Software interrupt
                    self.thumb_swi(instruction)
                } else if ((instruction >> 8) & 0xF) == 0xE {
                    // Undefined
                    1
                } else {
                    // Format 16: Conditional branch
                    self.thumb_cond_branch(instruction)
                }
            }
            0b111 => {
                if (instruction >> 11) & 3 == 0b00 {
                    // Format 18: Unconditional branch
                    self.thumb_branch(instruction)
                } else {
                    // Format 19: Long branch with link
                    self.thumb_long_branch(instruction)
                }
            }
            _ => 1,
        }
    }

    // =========================================================================
    // Format 1: Move shifted register (LSL/LSR/ASR Rd, Rs, #Offset)
    // =========================================================================

    fn thumb_shift(&mut self, instruction: u32) -> u32 {
        let op = (instruction >> 11) & 3;
        let offset = ((instruction >> 6) & 0x1F) as u8;
        let rs = ((instruction >> 3) & 7) as usize;
        let rd = (instruction & 7) as usize;

        let value = self.regs.gpr[rs];
        let carry = self.regs.cpsr.contains(Psr::C);

        let shift_type = match op {
            0 => ShiftType::Lsl,
            1 => ShiftType::Lsr,
            2 => ShiftType::Asr,
            _ => unreachable!(),
        };

        let (result, new_carry) = Self::barrel_shift(value, shift_type, offset, carry);

        self.regs.gpr[rd] = result;
        self.regs.cpsr.set(Psr::N, (result >> 31) != 0);
        self.regs.cpsr.set(Psr::Z, result == 0);
        self.regs.cpsr.set(Psr::C, new_carry);

        1
    }

    // =========================================================================
    // Format 2: Add/Subtract
    // =========================================================================

    fn thumb_add_sub(&mut self, instruction: u32) -> u32 {
        let is_imm = (instruction >> 10) & 1 != 0;
        let is_sub = (instruction >> 9) & 1 != 0;
        let rn_or_imm = ((instruction >> 6) & 7) as u32;
        let rs = ((instruction >> 3) & 7) as usize;
        let rd = (instruction & 7) as usize;

        let op1 = self.regs.gpr[rs];
        let op2 = if is_imm { rn_or_imm } else { self.regs.gpr[rn_or_imm as usize] };

        let (result, carry, overflow) = if is_sub {
            let (r, borrow) = op1.overflowing_sub(op2);
            let v = ((op1 ^ op2) & (op1 ^ r)) >> 31 != 0;
            (r, !borrow, v)
        } else {
            let (r, carry) = op1.overflowing_add(op2);
            let v = (!(op1 ^ op2) & (op1 ^ r)) >> 31 != 0;
            (r, carry, v)
        };

        self.regs.gpr[rd] = result;
        self.regs.cpsr.set(Psr::N, (result >> 31) != 0);
        self.regs.cpsr.set(Psr::Z, result == 0);
        self.regs.cpsr.set(Psr::C, carry);
        self.regs.cpsr.set(Psr::V, overflow);

        1
    }

    // =========================================================================
    // Format 3: Move/Compare/Add/Subtract immediate
    // =========================================================================

    fn thumb_imm_op(&mut self, instruction: u32) -> u32 {
        let op = (instruction >> 11) & 3;
        let rd = ((instruction >> 8) & 7) as usize;
        let imm = (instruction & 0xFF) as u32;

        match op {
            0 => {
                // MOV
                self.regs.gpr[rd] = imm;
                self.regs.cpsr.set(Psr::N, false);
                self.regs.cpsr.set(Psr::Z, imm == 0);
            }
            1 => {
                // CMP
                let op1 = self.regs.gpr[rd];
                let (result, borrow) = op1.overflowing_sub(imm);
                let v = ((op1 ^ imm) & (op1 ^ result)) >> 31 != 0;
                self.regs.cpsr.set(Psr::N, (result >> 31) != 0);
                self.regs.cpsr.set(Psr::Z, result == 0);
                self.regs.cpsr.set(Psr::C, !borrow);
                self.regs.cpsr.set(Psr::V, v);
            }
            2 => {
                // ADD
                let op1 = self.regs.gpr[rd];
                let (result, carry) = op1.overflowing_add(imm);
                let v = (!(op1 ^ imm) & (op1 ^ result)) >> 31 != 0;
                self.regs.gpr[rd] = result;
                self.regs.cpsr.set(Psr::N, (result >> 31) != 0);
                self.regs.cpsr.set(Psr::Z, result == 0);
                self.regs.cpsr.set(Psr::C, carry);
                self.regs.cpsr.set(Psr::V, v);
            }
            3 => {
                // SUB
                let op1 = self.regs.gpr[rd];
                let (result, borrow) = op1.overflowing_sub(imm);
                let v = ((op1 ^ imm) & (op1 ^ result)) >> 31 != 0;
                self.regs.gpr[rd] = result;
                self.regs.cpsr.set(Psr::N, (result >> 31) != 0);
                self.regs.cpsr.set(Psr::Z, result == 0);
                self.regs.cpsr.set(Psr::C, !borrow);
                self.regs.cpsr.set(Psr::V, v);
            }
            _ => unreachable!(),
        }

        1
    }

    // =========================================================================
    // Format 4: ALU operations
    // =========================================================================

    fn thumb_alu(&mut self, instruction: u32) -> u32 {
        let op = (instruction >> 6) & 0xF;
        let rs = ((instruction >> 3) & 7) as usize;
        let rd = (instruction & 7) as usize;

        let a = self.regs.gpr[rd];
        let b = self.regs.gpr[rs];
        let carry = self.regs.cpsr.contains(Psr::C);

        let mut cycles = 1u32;

        let (result, new_carry, new_overflow) = match op {
            0x0 => (a & b, carry, false),                          // AND
            0x1 => (a ^ b, carry, false),                          // EOR
            0x2 => {                                                // LSL
                let shift = b & 0xFF;
                let (r, c) = if shift == 0 {
                    (a, carry)
                } else {
                    Self::barrel_shift(a, ShiftType::Lsl, shift as u8, carry)
                };
                cycles = 2;
                (r, c, false)
            }
            0x3 => {                                                // LSR
                let shift = b & 0xFF;
                let (r, c) = if shift == 0 {
                    (a, carry)
                } else {
                    Self::barrel_shift(a, ShiftType::Lsr, shift as u8, carry)
                };
                cycles = 2;
                (r, c, false)
            }
            0x4 => {                                                // ASR
                let shift = b & 0xFF;
                let (r, c) = if shift == 0 {
                    (a, carry)
                } else {
                    Self::barrel_shift(a, ShiftType::Asr, shift as u8, carry)
                };
                cycles = 2;
                (r, c, false)
            }
            0x5 => {                                                // ADC
                let c_in = carry as u32;
                let (r1, c1) = a.overflowing_add(b);
                let (r2, c2) = r1.overflowing_add(c_in);
                let v = (!(a ^ b) & (a ^ r2)) >> 31 != 0;
                (r2, c1 || c2, v)
            }
            0x6 => {                                                // SBC
                let c_in = carry as u32;
                let (r1, b1) = a.overflowing_sub(b);
                let (r2, b2) = r1.overflowing_sub(1 - c_in);
                let v = ((a ^ b) & (a ^ r2)) >> 31 != 0;
                (r2, !(b1 || b2), v)
            }
            0x7 => {                                                // ROR
                let shift = b & 0xFF;
                let (r, c) = if shift == 0 {
                    (a, carry)
                } else {
                    Self::barrel_shift(a, ShiftType::Ror, shift as u8, carry)
                };
                cycles = 2;
                (r, c, false)
            }
            0x8 => {                                                // TST
                let result = a & b;
                self.regs.cpsr.set(Psr::N, (result >> 31) != 0);
                self.regs.cpsr.set(Psr::Z, result == 0);
                return 1;
            }
            0x9 => {                                                // NEG
                let (result, borrow) = 0u32.overflowing_sub(b);
                let v = (b & result) >> 31 != 0;
                (result, !borrow, v)
            }
            0xA => {                                                // CMP
                let (result, borrow) = a.overflowing_sub(b);
                let v = ((a ^ b) & (a ^ result)) >> 31 != 0;
                self.regs.cpsr.set(Psr::N, (result >> 31) != 0);
                self.regs.cpsr.set(Psr::Z, result == 0);
                self.regs.cpsr.set(Psr::C, !borrow);
                self.regs.cpsr.set(Psr::V, v);
                return 1;
            }
            0xB => {                                                // CMN
                let (result, c) = a.overflowing_add(b);
                let v = (!(a ^ b) & (a ^ result)) >> 31 != 0;
                self.regs.cpsr.set(Psr::N, (result >> 31) != 0);
                self.regs.cpsr.set(Psr::Z, result == 0);
                self.regs.cpsr.set(Psr::C, c);
                self.regs.cpsr.set(Psr::V, v);
                return 1;
            }
            0xC => (a | b, carry, false),                          // ORR
            0xD => {                                                // MUL
                let result = a.wrapping_mul(b);
                cycles = 1 + crate::exec_arm::multiply_cycles(a);
                (result, carry, false)
            }
            0xE => (a & !b, carry, false),                         // BIC
            0xF => (!b, carry, false),                              // MVN
            _ => unreachable!(),
        };

        self.regs.gpr[rd] = result;
        self.regs.cpsr.set(Psr::N, (result >> 31) != 0);
        self.regs.cpsr.set(Psr::Z, result == 0);
        if matches!(op, 0x5 | 0x6 | 0x9) {
            // Arithmetic: update C and V
            self.regs.cpsr.set(Psr::C, new_carry);
            self.regs.cpsr.set(Psr::V, new_overflow);
        } else if !matches!(op, 0xD) {
            // Logical shift/bitwise: update C only
            self.regs.cpsr.set(Psr::C, new_carry);
        }

        cycles
    }

    // =========================================================================
    // Format 5: Hi register operations / BX
    // =========================================================================

    fn thumb_hi_reg_bx(&mut self, instruction: u32) -> u32 {
        let op = (instruction >> 8) & 3;
        let h1 = ((instruction >> 7) & 1) as usize;
        let h2 = ((instruction >> 6) & 1) as usize;
        let rs = (((instruction >> 3) & 7) as usize) | (h2 << 3);
        let rd = ((instruction & 7) as usize) | (h1 << 3);

        match op {
            0 => {
                // ADD
                self.regs.gpr[rd] = self.regs.gpr[rd].wrapping_add(self.reg_hi(rs));
                if rd == 15 {
                    self.regs.set_pc(self.regs.pc() & !1);
                    return 3;
                }
            }
            1 => {
                // CMP
                let a = self.regs.gpr[rd];
                let b = self.reg_hi(rs);
                let (result, borrow) = a.overflowing_sub(b);
                let v = ((a ^ b) & (a ^ result)) >> 31 != 0;
                self.regs.cpsr.set(Psr::N, (result >> 31) != 0);
                self.regs.cpsr.set(Psr::Z, result == 0);
                self.regs.cpsr.set(Psr::C, !borrow);
                self.regs.cpsr.set(Psr::V, v);
            }
            2 => {
                // MOV
                self.regs.gpr[rd] = self.reg_hi(rs);
                if rd == 15 {
                    self.regs.set_pc(self.regs.pc() & !1);
                    return 3;
                }
            }
            3 => {
                // BX
                let addr = self.reg_hi(rs);
                if addr & 1 != 0 {
                    self.regs.cpsr.insert(Psr::T);
                    self.regs.set_pc(addr & !1);
                } else {
                    self.regs.cpsr.remove(Psr::T);
                    self.regs.set_pc(addr & !3);
                }
                return 3;
            }
            _ => unreachable!(),
        }

        1
    }

    fn reg_hi(&self, reg: usize) -> u32 {
        if reg == 15 {
            self.regs.pc().wrapping_add(2)
        } else {
            self.regs.gpr[reg]
        }
    }

    // =========================================================================
    // Format 6: PC-relative load
    // =========================================================================

    fn thumb_pc_load(&mut self, instruction: u32, bus: &impl BusAccess) -> u32 {
        let rd = ((instruction >> 8) & 7) as usize;
        let offset = (instruction & 0xFF) as u32 * 4;
        let addr = (self.regs.pc().wrapping_add(2) & !3).wrapping_add(offset);
        self.regs.gpr[rd] = bus.read_word(addr);
        3
    }

    // =========================================================================
    // Format 7/8: Load/store with register offset
    // =========================================================================

    fn thumb_load_store_reg(&mut self, instruction: u32, bus: &mut impl BusAccess) -> u32 {
        let is_format8 = (instruction >> 9) & 1 != 0;
        let ro = ((instruction >> 6) & 7) as usize;
        let rb = ((instruction >> 3) & 7) as usize;
        let rd = (instruction & 7) as usize;
        let addr = self.regs.gpr[rb].wrapping_add(self.regs.gpr[ro]);

        if is_format8 {
            // Format 8: signed/halfword
            let op = (instruction >> 10) & 3;
            match op {
                0 => {
                    // STRH
                    bus.write_halfword(addr & !1, self.regs.gpr[rd] as u16);
                    return 2;
                }
                1 => {
                    // LDSB
                    self.regs.gpr[rd] = bus.read_byte(addr) as i8 as i32 as u32;
                }
                2 => {
                    // LDRH
                    let val = bus.read_halfword(addr & !1);
                    self.regs.gpr[rd] = if addr & 1 != 0 {
                        (val as u32).rotate_right(8)
                    } else {
                        val as u32
                    };
                }
                3 => {
                    // LDSH
                    if addr & 1 != 0 {
                        self.regs.gpr[rd] = bus.read_byte(addr) as i8 as i32 as u32;
                    } else {
                        self.regs.gpr[rd] = bus.read_halfword(addr) as i16 as i32 as u32;
                    }
                }
                _ => unreachable!(),
            }
            3
        } else {
            // Format 7: byte/word
            let load = (instruction >> 11) & 1 != 0;
            let byte = (instruction >> 10) & 1 != 0;

            if load {
                if byte {
                    self.regs.gpr[rd] = bus.read_byte(addr) as u32;
                } else {
                    let aligned = addr & !3;
                    let rotation = (addr & 3) * 8;
                    self.regs.gpr[rd] = bus.read_word(aligned).rotate_right(rotation);
                }
                3
            } else {
                if byte {
                    bus.write_byte(addr, self.regs.gpr[rd] as u8);
                } else {
                    bus.write_word(addr & !3, self.regs.gpr[rd]);
                }
                2
            }
        }
    }

    // =========================================================================
    // Format 9: Load/store with immediate offset
    // =========================================================================

    fn thumb_load_store_imm(&mut self, instruction: u32, bus: &mut impl BusAccess) -> u32 {
        let byte = (instruction >> 12) & 1 != 0;
        let load = (instruction >> 11) & 1 != 0;
        let offset = ((instruction >> 6) & 0x1F) as u32;
        let rb = ((instruction >> 3) & 7) as usize;
        let rd = (instruction & 7) as usize;

        let offset = if byte { offset } else { offset * 4 };
        let addr = self.regs.gpr[rb].wrapping_add(offset);

        if load {
            if byte {
                self.regs.gpr[rd] = bus.read_byte(addr) as u32;
            } else {
                let aligned = addr & !3;
                let rotation = (addr & 3) * 8;
                self.regs.gpr[rd] = bus.read_word(aligned).rotate_right(rotation);
            }
            3
        } else {
            if byte {
                bus.write_byte(addr, self.regs.gpr[rd] as u8);
            } else {
                bus.write_word(addr & !3, self.regs.gpr[rd]);
            }
            2
        }
    }

    // =========================================================================
    // Format 10: Load/store halfword
    // =========================================================================

    fn thumb_load_store_half(&mut self, instruction: u32, bus: &mut impl BusAccess) -> u32 {
        let load = (instruction >> 11) & 1 != 0;
        let offset = ((instruction >> 6) & 0x1F) as u32 * 2;
        let rb = ((instruction >> 3) & 7) as usize;
        let rd = (instruction & 7) as usize;

        let addr = self.regs.gpr[rb].wrapping_add(offset);

        if load {
            self.regs.gpr[rd] = bus.read_halfword(addr & !1) as u32;
            3
        } else {
            bus.write_halfword(addr & !1, self.regs.gpr[rd] as u16);
            2
        }
    }

    // =========================================================================
    // Format 11: SP-relative load/store
    // =========================================================================

    fn thumb_sp_load_store(&mut self, instruction: u32, bus: &mut impl BusAccess) -> u32 {
        let load = (instruction >> 11) & 1 != 0;
        let rd = ((instruction >> 8) & 7) as usize;
        let offset = (instruction & 0xFF) as u32 * 4;
        let addr = self.regs.gpr[RegisterFile::SP].wrapping_add(offset);

        if load {
            self.regs.gpr[rd] = bus.read_word(addr & !3);
            3
        } else {
            bus.write_word(addr & !3, self.regs.gpr[rd]);
            2
        }
    }

    // =========================================================================
    // Format 12: Load address (ADD Rd, PC/SP, #imm)
    // =========================================================================

    fn thumb_load_address(&mut self, instruction: u32) -> u32 {
        let sp = (instruction >> 11) & 1 != 0;
        let rd = ((instruction >> 8) & 7) as usize;
        let offset = (instruction & 0xFF) as u32 * 4;

        self.regs.gpr[rd] = if sp {
            self.regs.gpr[RegisterFile::SP].wrapping_add(offset)
        } else {
            (self.regs.pc().wrapping_add(2) & !3).wrapping_add(offset)
        };

        1
    }

    // =========================================================================
    // Format 13: Add offset to SP
    // =========================================================================

    fn thumb_sp_offset(&mut self, instruction: u32) -> u32 {
        let negative = (instruction >> 7) & 1 != 0;
        let offset = (instruction & 0x7F) as u32 * 4;

        if negative {
            self.regs.gpr[RegisterFile::SP] =
                self.regs.gpr[RegisterFile::SP].wrapping_sub(offset);
        } else {
            self.regs.gpr[RegisterFile::SP] =
                self.regs.gpr[RegisterFile::SP].wrapping_add(offset);
        }

        1
    }

    // =========================================================================
    // Format 14: Push/Pop registers
    // =========================================================================

    fn thumb_push_pop(&mut self, instruction: u32, bus: &mut impl BusAccess) -> u32 {
        let load = (instruction >> 11) & 1 != 0;
        let pc_lr = (instruction >> 8) & 1 != 0;
        let reg_list = (instruction & 0xFF) as u8;

        let count = reg_list.count_ones() + pc_lr as u32;
        let mut cycles = 0u32;

        if load {
            // POP
            let mut addr = self.regs.gpr[RegisterFile::SP];
            for i in 0..8 {
                if reg_list & (1 << i) != 0 {
                    self.regs.gpr[i] = bus.read_word(addr & !3);
                    addr = addr.wrapping_add(4);
                    cycles += 1;
                }
            }
            if pc_lr {
                let val = bus.read_word(addr & !3);
                self.regs.set_pc(val & !1);
                addr = addr.wrapping_add(4);
                cycles += 2; // branch penalty
            }
            self.regs.gpr[RegisterFile::SP] = addr;
        } else {
            // PUSH
            let mut addr = self.regs.gpr[RegisterFile::SP].wrapping_sub(count * 4);
            self.regs.gpr[RegisterFile::SP] = addr;
            for i in 0..8 {
                if reg_list & (1 << i) != 0 {
                    bus.write_word(addr & !3, self.regs.gpr[i]);
                    addr = addr.wrapping_add(4);
                    cycles += 1;
                }
            }
            if pc_lr {
                bus.write_word(addr & !3, self.regs.gpr[RegisterFile::LR]);
                cycles += 1;
            }
        }

        cycles + 1
    }

    // =========================================================================
    // Format 15: Multiple load/store (LDMIA/STMIA)
    // =========================================================================

    fn thumb_multi_load_store(&mut self, instruction: u32, bus: &mut impl BusAccess) -> u32 {
        let load = (instruction >> 11) & 1 != 0;
        let rb = ((instruction >> 8) & 7) as usize;
        let reg_list = (instruction & 0xFF) as u8;

        let mut addr = self.regs.gpr[rb];
        let mut cycles = 0u32;

        if reg_list == 0 {
            // Empty list: special behavior
            if load {
                let val = bus.read_word(addr & !3);
                self.regs.set_pc(val & !1);
            } else {
                bus.write_word(addr & !3, self.regs.pc().wrapping_add(2));
            }
            self.regs.gpr[rb] = addr.wrapping_add(0x40);
            return 3;
        }

        let base_in_list = reg_list & (1 << rb) != 0;

        for i in 0..8usize {
            if reg_list & (1 << i) != 0 {
                if load {
                    self.regs.gpr[i] = bus.read_word(addr & !3);
                } else {
                    bus.write_word(addr & !3, self.regs.gpr[i]);
                }
                addr = addr.wrapping_add(4);
                cycles += 1;
            }
        }

        // Write-back (not if Rb is in the list and it's a load)
        if !load || !base_in_list {
            self.regs.gpr[rb] = addr;
        }

        cycles + 1
    }

    // =========================================================================
    // Format 16: Conditional branch
    // =========================================================================

    fn thumb_cond_branch(&mut self, instruction: u32) -> u32 {
        let cond = (instruction >> 8) & 0xF;
        if !self.check_condition(cond) {
            return 1;
        }

        let offset = (instruction & 0xFF) as i8 as i32;
        let offset = offset << 1;
        let pc = self.regs.pc().wrapping_add(2);
        self.regs.set_pc((pc as i32).wrapping_add(offset) as u32);
        3
    }

    // =========================================================================
    // Format 17: Software interrupt
    // =========================================================================

    fn thumb_swi(&mut self, _instruction: u32) -> u32 {
        let cpsr = self.regs.cpsr;
        let return_addr = self.regs.pc().wrapping_sub(2);

        self.regs.switch_mode(CpuMode::Supervisor);
        self.regs.set_spsr(cpsr);
        self.regs.gpr[RegisterFile::LR] = return_addr;

        self.regs.cpsr.insert(Psr::I);
        self.regs.cpsr.remove(Psr::T);
        self.regs.set_pc(0x0000_0008);

        3
    }

    // =========================================================================
    // Format 18: Unconditional branch
    // =========================================================================

    fn thumb_branch(&mut self, instruction: u32) -> u32 {
        let offset = instruction & 0x7FF;
        let offset = if offset & 0x400 != 0 {
            (offset | 0xFFFFF800) as i32
        } else {
            offset as i32
        };
        let offset = offset << 1;
        let pc = self.regs.pc().wrapping_add(2);
        self.regs.set_pc((pc as i32).wrapping_add(offset) as u32);
        3
    }

    // =========================================================================
    // Format 19: Long branch with link (BL — two-instruction sequence)
    // =========================================================================

    fn thumb_long_branch(&mut self, instruction: u32) -> u32 {
        let h = (instruction >> 11) & 1;

        if h == 0 {
            // First instruction: LR = PC + (offset << 12)
            let offset = instruction & 0x7FF;
            let offset = if offset & 0x400 != 0 {
                (offset | 0xFFFFF800) as i32
            } else {
                offset as i32
            };
            self.regs.gpr[RegisterFile::LR] =
                (self.regs.pc().wrapping_add(2) as i32).wrapping_add(offset << 12) as u32;
            1
        } else {
            // Second instruction: PC = LR + (offset << 1), LR = old PC | 1
            let offset = (instruction & 0x7FF) << 1;
            let old_pc = self.regs.pc().wrapping_sub(2);
            let target = self.regs.gpr[RegisterFile::LR].wrapping_add(offset);
            self.regs.gpr[RegisterFile::LR] = old_pc | 1;
            self.regs.set_pc(target & !1);
            3
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rgba_core::bus::Bus;

    fn make_thumb_cpu() -> Arm7Tdmi {
        let mut cpu = Arm7Tdmi::new();
        cpu.reset_skip_bios();
        cpu.regs.cpsr.insert(Psr::T); // Thumb mode
        cpu
    }

    #[test]
    fn test_thumb_mov_imm() {
        let mut cpu = make_thumb_cpu();
        let mut bus = Bus::new();
        // MOV R0, #42
        let instr = 0x202A;
        cpu.execute_thumb_instruction(instr, &mut bus);
        assert_eq!(cpu.regs.gpr[0], 42);
    }

    #[test]
    fn test_thumb_add_imm() {
        let mut cpu = make_thumb_cpu();
        let mut bus = Bus::new();
        cpu.regs.gpr[0] = 10;
        // ADD R0, #5
        let instr = 0x3005;
        cpu.execute_thumb_instruction(instr, &mut bus);
        assert_eq!(cpu.regs.gpr[0], 15);
    }

    #[test]
    fn test_thumb_sub_imm() {
        let mut cpu = make_thumb_cpu();
        let mut bus = Bus::new();
        cpu.regs.gpr[1] = 10;
        // SUB R1, #3
        let instr = 0x3903;
        cpu.execute_thumb_instruction(instr, &mut bus);
        assert_eq!(cpu.regs.gpr[1], 7);
    }

    #[test]
    fn test_thumb_lsl() {
        let mut cpu = make_thumb_cpu();
        let mut bus = Bus::new();
        cpu.regs.gpr[0] = 1;
        // LSL R1, R0, #4
        let instr = 0x0101; // LSL Rd=R1, Rs=R0, #4
        cpu.execute_thumb_instruction(instr, &mut bus);
        assert_eq!(cpu.regs.gpr[1], 16);
    }

    #[test]
    fn test_thumb_add_reg() {
        let mut cpu = make_thumb_cpu();
        let mut bus = Bus::new();
        cpu.regs.gpr[0] = 5;
        cpu.regs.gpr[1] = 3;
        // ADD R2, R0, R1 — Format 2: 000 11 0 0 Rm=001 Rs=000 Rd=010
        let instr = 0x1842;
        cpu.execute_thumb_instruction(instr, &mut bus);
        assert_eq!(cpu.regs.gpr[2], 8);
    }

    #[test]
    fn test_thumb_str_ldr() {
        let mut cpu = make_thumb_cpu();
        let mut bus = Bus::new();
        cpu.regs.gpr[0] = 0xABCD;
        cpu.regs.gpr[1] = 0x0200_0000;
        // STR R0, [R1, #0] (format 9, word, imm offset 0)
        let str_instr = 0x6008;
        cpu.execute_thumb_instruction(str_instr, &mut bus);
        assert_eq!(bus.read_word(0x0200_0000), 0xABCD);

        // LDR R2, [R1, #0]
        let ldr_instr = 0x680A;
        cpu.execute_thumb_instruction(ldr_instr, &mut bus);
        assert_eq!(cpu.regs.gpr[2], 0xABCD);
    }

    #[test]
    fn test_thumb_push_pop() {
        let mut cpu = make_thumb_cpu();
        let mut bus = Bus::new();
        cpu.regs.gpr[0] = 0xAA;
        cpu.regs.gpr[1] = 0xBB;
        cpu.regs.gpr[RegisterFile::SP] = 0x0300_7F00;

        // PUSH {R0, R1}
        let push = 0xB403;
        cpu.execute_thumb_instruction(push, &mut bus);
        assert_eq!(cpu.regs.gpr[RegisterFile::SP], 0x0300_7F00 - 8);

        cpu.regs.gpr[0] = 0;
        cpu.regs.gpr[1] = 0;

        // POP {R0, R1}
        let pop = 0xBC03;
        cpu.execute_thumb_instruction(pop, &mut bus);
        assert_eq!(cpu.regs.gpr[0], 0xAA);
        assert_eq!(cpu.regs.gpr[1], 0xBB);
    }

    #[test]
    fn test_thumb_cond_branch() {
        let mut cpu = make_thumb_cpu();
        let mut bus = Bus::new();
        cpu.regs.cpsr.insert(Psr::Z);
        let pc_before = cpu.regs.pc();

        // BEQ +4 (offset = 2, shifted = 4)
        let instr = 0xD002;
        cpu.execute_thumb_instruction(instr, &mut bus);
        assert_eq!(cpu.regs.pc(), pc_before.wrapping_add(2 + 4));
    }

    #[test]
    fn test_thumb_branch() {
        let mut cpu = make_thumb_cpu();
        let mut bus = Bus::new();
        let pc_before = cpu.regs.pc();

        // B +10 (offset = 5, shifted = 10)
        let instr = 0xE005;
        cpu.execute_thumb_instruction(instr, &mut bus);
        assert_eq!(cpu.regs.pc(), pc_before.wrapping_add(2 + 10));
    }

    #[test]
    fn test_thumb_sp_offset() {
        let mut cpu = make_thumb_cpu();
        let mut bus = Bus::new();
        cpu.regs.gpr[RegisterFile::SP] = 0x1000;

        // ADD SP, #-16 (offset 4, negative)
        let instr = 0xB084;
        cpu.execute_thumb_instruction(instr, &mut bus);
        assert_eq!(cpu.regs.gpr[RegisterFile::SP], 0x1000 - 16);
    }

    #[test]
    fn test_thumb_bx_arm() {
        let mut cpu = make_thumb_cpu();
        let mut bus = Bus::new();
        cpu.regs.gpr[0] = 0x0800_0000; // bit 0 = 0 => ARM mode

        // BX R0
        let instr = 0x4700;
        cpu.execute_thumb_instruction(instr, &mut bus);
        assert!(!cpu.regs.cpsr.thumb());
        assert_eq!(cpu.regs.pc(), 0x0800_0000);
    }
}
