use rgba_core::types::Psr;

use crate::cpu::Arm7Tdmi;

// =============================================================================
// ARM Instruction Decoding & Execution
// =============================================================================

/// Decoded ARM instruction
#[derive(Debug, Clone, Copy)]
pub enum ArmInstruction {
    /// Branch and Exchange (BX Rn)
    BranchExchange { rn: usize },
    /// Branch (B/BL offset)
    Branch { link: bool, offset: i32 },
    /// Data Processing (ALU operations)
    DataProcessing {
        opcode: AluOp,
        set_flags: bool,
        rn: usize,
        rd: usize,
        operand2: ShifterOperand,
    },
    /// Multiply (MUL/MLA)
    Multiply {
        accumulate: bool,
        set_flags: bool,
        rd: usize,
        rn: usize,
        rs: usize,
        rm: usize,
    },
    /// Multiply Long (UMULL/UMLAL/SMULL/SMLAL)
    MultiplyLong {
        signed: bool,
        accumulate: bool,
        set_flags: bool,
        rd_hi: usize,
        rd_lo: usize,
        rs: usize,
        rm: usize,
    },
    /// Single Data Transfer (LDR/STR)
    SingleTransfer {
        load: bool,
        byte: bool,
        write_back: bool,
        up: bool,
        pre: bool,
        rn: usize,
        rd: usize,
        offset: TransferOffset,
    },
    /// Halfword/Signed Data Transfer (LDRH/STRH/LDRSB/LDRSH)
    HalfwordTransfer {
        load: bool,
        write_back: bool,
        up: bool,
        pre: bool,
        signed: bool,
        half: bool,
        rn: usize,
        rd: usize,
        offset: HalfwordOffset,
    },
    /// Block Data Transfer (LDM/STM)
    BlockTransfer {
        load: bool,
        write_back: bool,
        up: bool,
        pre: bool,
        s_bit: bool,
        rn: usize,
        register_list: u16,
    },
    /// Single Data Swap (SWP/SWPB)
    Swap {
        byte: bool,
        rn: usize,
        rd: usize,
        rm: usize,
    },
    /// MRS (read status register)
    Mrs {
        spsr: bool,
        rd: usize,
    },
    /// MSR (write status register)
    Msr {
        spsr: bool,
        field_mask: u8,
        operand: MsrOperand,
    },
    /// Software Interrupt
    Swi {
        comment: u32,
    },
    /// Undefined instruction
    Undefined,
}

/// ALU operation codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AluOp {
    And = 0,
    Eor = 1,
    Sub = 2,
    Rsb = 3,
    Add = 4,
    Adc = 5,
    Sbc = 6,
    Rsc = 7,
    Tst = 8,
    Teq = 9,
    Cmp = 10,
    Cmn = 11,
    Orr = 12,
    Mov = 13,
    Bic = 14,
    Mvn = 15,
}

impl AluOp {
    pub fn from_u32(v: u32) -> Self {
        match v & 0xF {
            0 => AluOp::And,
            1 => AluOp::Eor,
            2 => AluOp::Sub,
            3 => AluOp::Rsb,
            4 => AluOp::Add,
            5 => AluOp::Adc,
            6 => AluOp::Sbc,
            7 => AluOp::Rsc,
            8 => AluOp::Tst,
            9 => AluOp::Teq,
            10 => AluOp::Cmp,
            11 => AluOp::Cmn,
            12 => AluOp::Orr,
            13 => AluOp::Mov,
            14 => AluOp::Bic,
            15 => AluOp::Mvn,
            _ => unreachable!(),
        }
    }

    /// Is this a test/compare operation (no destination write)?
    pub fn is_test(self) -> bool {
        matches!(self, AluOp::Tst | AluOp::Teq | AluOp::Cmp | AluOp::Cmn)
    }

    /// Is this a logical operation (for flag calculation)?
    pub fn is_logical(self) -> bool {
        matches!(
            self,
            AluOp::And
                | AluOp::Eor
                | AluOp::Tst
                | AluOp::Teq
                | AluOp::Orr
                | AluOp::Mov
                | AluOp::Bic
                | AluOp::Mvn
        )
    }
}

/// Shifter operand for data processing instructions
#[derive(Debug, Clone, Copy)]
pub enum ShifterOperand {
    /// Immediate value with rotation: value = imm8 ROR (rot * 2)
    Immediate { value: u32, carry: Option<bool> },
    /// Register shifted by immediate
    RegisterImm {
        rm: usize,
        shift_type: ShiftType,
        amount: u8,
    },
    /// Register shifted by register
    RegisterReg {
        rm: usize,
        shift_type: ShiftType,
        rs: usize,
    },
}

/// Shift types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftType {
    Lsl = 0,
    Lsr = 1,
    Asr = 2,
    Ror = 3,
}

impl ShiftType {
    pub fn from_u32(v: u32) -> Self {
        match v & 3 {
            0 => ShiftType::Lsl,
            1 => ShiftType::Lsr,
            2 => ShiftType::Asr,
            3 => ShiftType::Ror,
            _ => unreachable!(),
        }
    }
}

/// Offset for single data transfer
#[derive(Debug, Clone, Copy)]
pub enum TransferOffset {
    Immediate(u16),
    Register {
        rm: usize,
        shift_type: ShiftType,
        amount: u8,
    },
}

/// Offset for halfword transfer
#[derive(Debug, Clone, Copy)]
pub enum HalfwordOffset {
    Immediate(u8),
    Register(usize),
}

/// Operand for MSR instruction
#[derive(Debug, Clone, Copy)]
pub enum MsrOperand {
    Immediate(u32),
    Register(usize),
}

// =============================================================================
// Decode
// =============================================================================

impl Arm7Tdmi {
    /// Decode a 32-bit ARM instruction
    pub fn decode_arm(instruction: u32) -> ArmInstruction {
        let cond = instruction >> 28;
        let _ = cond; // condition is checked separately

        // Bits [27:20] and [7:4] determine instruction type
        match (instruction >> 20) & 0xFF {
            // Branch and Exchange: 0001_0010_xxxx_0001
            _ if (instruction & 0x0FFF_FFF0) == 0x012F_FF10 => {
                let rn = (instruction & 0xF) as usize;
                ArmInstruction::BranchExchange { rn }
            }

            // SWP/SWPB: 0001_0x00_xxxx_1001
            _ if (instruction & 0x0FB0_0FF0) == 0x0100_0090 => {
                let byte = (instruction >> 22) & 1 != 0;
                let rn = ((instruction >> 16) & 0xF) as usize;
                let rd = ((instruction >> 12) & 0xF) as usize;
                let rm = (instruction & 0xF) as usize;
                ArmInstruction::Swap { byte, rn, rd, rm }
            }

            // Multiply Long: 0000_1xxx_xxxx_1001
            _ if (instruction & 0x0F80_00F0) == 0x0080_0090 => {
                let signed = (instruction >> 22) & 1 != 0;
                let accumulate = (instruction >> 21) & 1 != 0;
                let set_flags = (instruction >> 20) & 1 != 0;
                let rd_hi = ((instruction >> 16) & 0xF) as usize;
                let rd_lo = ((instruction >> 12) & 0xF) as usize;
                let rs = ((instruction >> 8) & 0xF) as usize;
                let rm = (instruction & 0xF) as usize;
                ArmInstruction::MultiplyLong {
                    signed,
                    accumulate,
                    set_flags,
                    rd_hi,
                    rd_lo,
                    rs,
                    rm,
                }
            }

            // Multiply: 0000_00xx_xxxx_1001
            _ if (instruction & 0x0FC0_00F0) == 0x0000_0090 => {
                let accumulate = (instruction >> 21) & 1 != 0;
                let set_flags = (instruction >> 20) & 1 != 0;
                let rd = ((instruction >> 16) & 0xF) as usize;
                let rn = ((instruction >> 12) & 0xF) as usize;
                let rs = ((instruction >> 8) & 0xF) as usize;
                let rm = (instruction & 0xF) as usize;
                ArmInstruction::Multiply {
                    accumulate,
                    set_flags,
                    rd,
                    rn,
                    rs,
                    rm,
                }
            }

            // Halfword transfer (register offset): xxxx_000x_xxxx_1xx1 (not multiply)
            _ if (instruction & 0x0E40_0F90) == 0x0000_0090
                && (instruction & 0x0FB0_0FF0) != 0x0100_0090
                && (instruction & 0x0F80_00F0) != 0x0080_0090
                && (instruction & 0x0FC0_00F0) != 0x0000_0090
                && (instruction & 0x0E00_0090) == 0x0000_0090
                && (instruction & 0x60) != 0 =>
            {
                let pre = (instruction >> 24) & 1 != 0;
                let up = (instruction >> 23) & 1 != 0;
                let imm = (instruction >> 22) & 1 != 0;
                let write_back = (instruction >> 21) & 1 != 0;
                let load = (instruction >> 20) & 1 != 0;
                let rn = ((instruction >> 16) & 0xF) as usize;
                let rd = ((instruction >> 12) & 0xF) as usize;
                let sh = (instruction >> 5) & 3;
                let signed = sh & 2 != 0;
                let half = sh & 1 != 0;

                let offset = if imm {
                    let hi = ((instruction >> 8) & 0xF) as u8;
                    let lo = (instruction & 0xF) as u8;
                    HalfwordOffset::Immediate(hi << 4 | lo)
                } else {
                    HalfwordOffset::Register((instruction & 0xF) as usize)
                };

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
                }
            }

            // MRS: 0001_0x00_xxxx_0000_0000_0000
            _ if (instruction & 0x0FBF_0FFF) == 0x010F_0000 => {
                let spsr = (instruction >> 22) & 1 != 0;
                let rd = ((instruction >> 12) & 0xF) as usize;
                ArmInstruction::Mrs { spsr, rd }
            }

            // MSR (register): 0001_0x10_xxxx_1111_0000_xxxx
            _ if (instruction & 0x0FB0_FFF0) == 0x0120_F000 => {
                let spsr = (instruction >> 22) & 1 != 0;
                let field_mask = ((instruction >> 16) & 0xF) as u8;
                let rm = (instruction & 0xF) as usize;
                ArmInstruction::Msr {
                    spsr,
                    field_mask,
                    operand: MsrOperand::Register(rm),
                }
            }

            // MSR (immediate): 0011_0x10_xxxx_1111_xxxx_xxxx
            _ if (instruction & 0x0FB0_F000) == 0x0320_F000 => {
                let spsr = (instruction >> 22) & 1 != 0;
                let field_mask = ((instruction >> 16) & 0xF) as u8;
                let rotate = ((instruction >> 8) & 0xF) * 2;
                let imm = instruction & 0xFF;
                let value = imm.rotate_right(rotate);
                ArmInstruction::Msr {
                    spsr,
                    field_mask,
                    operand: MsrOperand::Immediate(value),
                }
            }

            // Branch: 101x
            _ if (instruction >> 25) & 7 == 5 => {
                let link = (instruction >> 24) & 1 != 0;
                // Sign-extend 24-bit offset, shift left 2
                let offset = instruction & 0x00FF_FFFF;
                let offset = if offset & 0x0080_0000 != 0 {
                    (offset | 0xFF00_0000) as i32
                } else {
                    offset as i32
                };
                let offset = offset << 2;
                ArmInstruction::Branch { link, offset }
            }

            // Block Data Transfer: 100x
            _ if (instruction >> 25) & 7 == 4 => {
                let pre = (instruction >> 24) & 1 != 0;
                let up = (instruction >> 23) & 1 != 0;
                let s_bit = (instruction >> 22) & 1 != 0;
                let write_back = (instruction >> 21) & 1 != 0;
                let load = (instruction >> 20) & 1 != 0;
                let rn = ((instruction >> 16) & 0xF) as usize;
                let register_list = (instruction & 0xFFFF) as u16;
                ArmInstruction::BlockTransfer {
                    load,
                    write_back,
                    up,
                    pre,
                    s_bit,
                    rn,
                    register_list,
                }
            }

            // Single Data Transfer: 01xx
            _ if (instruction >> 26) & 3 == 1 => {
                let pre = (instruction >> 24) & 1 != 0;
                let up = (instruction >> 23) & 1 != 0;
                let byte = (instruction >> 22) & 1 != 0;
                let write_back = (instruction >> 21) & 1 != 0;
                let load = (instruction >> 20) & 1 != 0;
                let rn = ((instruction >> 16) & 0xF) as usize;
                let rd = ((instruction >> 12) & 0xF) as usize;

                let offset = if (instruction >> 25) & 1 == 0 {
                    // Immediate offset
                    TransferOffset::Immediate((instruction & 0xFFF) as u16)
                } else {
                    // Register offset with shift
                    let rm = (instruction & 0xF) as usize;
                    let shift_type = ShiftType::from_u32((instruction >> 5) & 3);
                    let amount = ((instruction >> 7) & 0x1F) as u8;
                    TransferOffset::Register {
                        rm,
                        shift_type,
                        amount,
                    }
                };

                ArmInstruction::SingleTransfer {
                    load,
                    byte,
                    write_back,
                    up,
                    pre,
                    rn,
                    rd,
                    offset,
                }
            }

            // Software Interrupt: 1111
            _ if (instruction >> 24) & 0xF == 0xF => {
                let comment = instruction & 0x00FF_FFFF;
                ArmInstruction::Swi { comment }
            }

            // Data Processing: 00xx
            _ if (instruction >> 26) & 3 == 0 => {
                let opcode = AluOp::from_u32((instruction >> 21) & 0xF);
                let set_flags = (instruction >> 20) & 1 != 0;
                let rn = ((instruction >> 16) & 0xF) as usize;
                let rd = ((instruction >> 12) & 0xF) as usize;

                let operand2 = if (instruction >> 25) & 1 != 0 {
                    // Immediate operand
                    let rotate = ((instruction >> 8) & 0xF) * 2;
                    let imm = instruction & 0xFF;
                    let value = imm.rotate_right(rotate);
                    let carry = if rotate != 0 {
                        Some((value >> 31) != 0)
                    } else {
                        None
                    };
                    ShifterOperand::Immediate { value, carry }
                } else if (instruction >> 4) & 1 == 0 {
                    // Register shifted by immediate
                    let rm = (instruction & 0xF) as usize;
                    let shift_type = ShiftType::from_u32((instruction >> 5) & 3);
                    let amount = ((instruction >> 7) & 0x1F) as u8;
                    ShifterOperand::RegisterImm {
                        rm,
                        shift_type,
                        amount,
                    }
                } else {
                    // Register shifted by register
                    let rm = (instruction & 0xF) as usize;
                    let shift_type = ShiftType::from_u32((instruction >> 5) & 3);
                    let rs = ((instruction >> 8) & 0xF) as usize;
                    ShifterOperand::RegisterReg {
                        rm,
                        shift_type,
                        rs,
                    }
                };

                ArmInstruction::DataProcessing {
                    opcode,
                    set_flags,
                    rn,
                    rd,
                    operand2,
                }
            }

            _ => ArmInstruction::Undefined,
        }
    }
}

// =============================================================================
// Barrel Shifter
// =============================================================================

impl Arm7Tdmi {
    /// Apply barrel shifter operation, returns (result, carry_out)
    pub fn barrel_shift(
        value: u32,
        shift_type: ShiftType,
        amount: u8,
        old_carry: bool,
    ) -> (u32, bool) {
        if amount == 0 {
            // Special cases for shift by 0
            match shift_type {
                ShiftType::Lsl => (value, old_carry),
                ShiftType::Lsr => (0, (value >> 31) != 0), // LSR #32
                ShiftType::Asr => {
                    if (value >> 31) != 0 {
                        (0xFFFF_FFFF, true)
                    } else {
                        (0, false)
                    }
                } // ASR #32
                ShiftType::Ror => {
                    // RRX (rotate right extended by 1)
                    let carry = value & 1 != 0;
                    let result = (value >> 1) | ((old_carry as u32) << 31);
                    (result, carry)
                }
            }
        } else {
            match shift_type {
                ShiftType::Lsl => {
                    if amount < 32 {
                        let carry = (value >> (32 - amount)) & 1 != 0;
                        (value << amount, carry)
                    } else if amount == 32 {
                        (0, value & 1 != 0)
                    } else {
                        (0, false)
                    }
                }
                ShiftType::Lsr => {
                    if amount < 32 {
                        let carry = (value >> (amount - 1)) & 1 != 0;
                        (value >> amount, carry)
                    } else if amount == 32 {
                        (0, (value >> 31) != 0)
                    } else {
                        (0, false)
                    }
                }
                ShiftType::Asr => {
                    if amount < 32 {
                        let carry = ((value as i32) >> (amount - 1)) & 1 != 0;
                        ((value as i32 >> amount) as u32, carry)
                    } else {
                        let bit31 = (value >> 31) != 0;
                        if bit31 {
                            (0xFFFF_FFFF, true)
                        } else {
                            (0, false)
                        }
                    }
                }
                ShiftType::Ror => {
                    let amount = amount & 31;
                    if amount == 0 {
                        (value, (value >> 31) != 0)
                    } else {
                        let result = value.rotate_right(amount as u32);
                        (result, (result >> 31) != 0)
                    }
                }
            }
        }
    }

    /// Resolve a shifter operand to (value, carry_out)
    pub fn resolve_operand(&self, operand: ShifterOperand) -> (u32, bool) {
        let carry = self.regs.cpsr.contains(Psr::C);
        match operand {
            ShifterOperand::Immediate { value, carry: c } => (value, c.unwrap_or(carry)),
            ShifterOperand::RegisterImm {
                rm,
                shift_type,
                amount,
            } => {
                let value = self.reg_for_alu(rm);
                Self::barrel_shift(value, shift_type, amount, carry)
            }
            ShifterOperand::RegisterReg {
                rm,
                shift_type,
                rs,
            } => {
                let value = self.reg_for_alu(rm);
                let shift_amount = (self.regs.gpr[rs] & 0xFF) as u8;
                if shift_amount == 0 {
                    (value, carry)
                } else {
                    Self::barrel_shift(value, shift_type, shift_amount, carry)
                }
            }
        }
    }

    /// Get register value for ALU operations (PC reads as PC+8 in ARM mode)
    fn reg_for_alu(&self, reg: usize) -> u32 {
        if reg == 15 {
            // In ARM mode, PC reads as current instruction address + 8
            self.regs.pc().wrapping_add(4)
        } else {
            self.regs.gpr[reg]
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_branch() {
        // B #0x100 (always, forward)
        let instr = 0xEA00_0040; // B +0x108
        match Arm7Tdmi::decode_arm(instr) {
            ArmInstruction::Branch { link, offset } => {
                assert!(!link);
                assert_eq!(offset, 0x100);
            }
            _ => panic!("Expected Branch"),
        }
    }

    #[test]
    fn test_decode_branch_link() {
        // BL #-8 (backward)
        let instr = 0xEBFF_FFFE;
        match Arm7Tdmi::decode_arm(instr) {
            ArmInstruction::Branch { link, offset } => {
                assert!(link);
                assert_eq!(offset, -8);
            }
            _ => panic!("Expected Branch"),
        }
    }

    #[test]
    fn test_decode_bx() {
        // BX R0
        let instr = 0xE12F_FF10;
        match Arm7Tdmi::decode_arm(instr) {
            ArmInstruction::BranchExchange { rn } => {
                assert_eq!(rn, 0);
            }
            _ => panic!("Expected BranchExchange"),
        }
    }

    #[test]
    fn test_decode_mov_imm() {
        // MOV R0, #0x42
        let instr = 0xE3A0_0042;
        match Arm7Tdmi::decode_arm(instr) {
            ArmInstruction::DataProcessing {
                opcode,
                set_flags,
                rd,
                operand2,
                ..
            } => {
                assert_eq!(opcode, AluOp::Mov);
                assert!(!set_flags);
                assert_eq!(rd, 0);
                match operand2 {
                    ShifterOperand::Immediate { value, .. } => assert_eq!(value, 0x42),
                    _ => panic!("Expected immediate"),
                }
            }
            _ => panic!("Expected DataProcessing"),
        }
    }

    #[test]
    fn test_decode_add_reg() {
        // ADD R0, R1, R2
        let instr = 0xE081_0002;
        match Arm7Tdmi::decode_arm(instr) {
            ArmInstruction::DataProcessing {
                opcode,
                rn,
                rd,
                operand2,
                ..
            } => {
                assert_eq!(opcode, AluOp::Add);
                assert_eq!(rn, 1);
                assert_eq!(rd, 0);
                match operand2 {
                    ShifterOperand::RegisterImm { rm, amount, .. } => {
                        assert_eq!(rm, 2);
                        assert_eq!(amount, 0);
                    }
                    _ => panic!("Expected register"),
                }
            }
            _ => panic!("Expected DataProcessing"),
        }
    }

    #[test]
    fn test_decode_ldr() {
        // LDR R0, [R1, #4]
        let instr = 0xE591_0004;
        match Arm7Tdmi::decode_arm(instr) {
            ArmInstruction::SingleTransfer {
                load,
                byte,
                pre,
                up,
                rn,
                rd,
                offset,
                ..
            } => {
                assert!(load);
                assert!(!byte);
                assert!(pre);
                assert!(up);
                assert_eq!(rn, 1);
                assert_eq!(rd, 0);
                match offset {
                    TransferOffset::Immediate(v) => assert_eq!(v, 4),
                    _ => panic!("Expected immediate offset"),
                }
            }
            _ => panic!("Expected SingleTransfer"),
        }
    }

    #[test]
    fn test_decode_stm() {
        // STMDB SP!, {R4-R11, LR}
        let instr = 0xE92D_4FF0;
        match Arm7Tdmi::decode_arm(instr) {
            ArmInstruction::BlockTransfer {
                load,
                write_back,
                up,
                pre,
                rn,
                register_list,
                ..
            } => {
                assert!(!load); // STM
                assert!(write_back);
                assert!(!up); // DB = decrement before
                assert!(pre);
                assert_eq!(rn, 13); // SP
                assert_eq!(register_list, 0x4FF0);
            }
            _ => panic!("Expected BlockTransfer"),
        }
    }

    #[test]
    fn test_decode_swi() {
        // SWI #0
        let instr = 0xEF00_0000;
        match Arm7Tdmi::decode_arm(instr) {
            ArmInstruction::Swi { comment } => {
                assert_eq!(comment, 0);
            }
            _ => panic!("Expected SWI"),
        }
    }

    #[test]
    fn test_decode_mul() {
        // MUL R0, R1, R2
        let instr = 0xE000_0291;
        match Arm7Tdmi::decode_arm(instr) {
            ArmInstruction::Multiply {
                accumulate,
                rd,
                rs,
                rm,
                ..
            } => {
                assert!(!accumulate);
                assert_eq!(rd, 0);
                assert_eq!(rs, 2);
                assert_eq!(rm, 1);
            }
            _ => panic!("Expected Multiply"),
        }
    }

    #[test]
    fn test_barrel_shift_lsl() {
        let (result, carry) = Arm7Tdmi::barrel_shift(0x80000001, ShiftType::Lsl, 1, false);
        assert_eq!(result, 0x00000002);
        assert!(carry); // bit 31 shifted out
    }

    #[test]
    fn test_barrel_shift_lsr() {
        let (result, carry) = Arm7Tdmi::barrel_shift(0x80000001, ShiftType::Lsr, 1, false);
        assert_eq!(result, 0x40000000);
        assert!(carry); // bit 0 shifted out
    }

    #[test]
    fn test_barrel_shift_asr() {
        let (result, carry) = Arm7Tdmi::barrel_shift(0x80000000, ShiftType::Asr, 1, false);
        assert_eq!(result, 0xC0000000); // sign-extended
        assert!(!carry);
    }

    #[test]
    fn test_barrel_shift_ror() {
        let (result, _carry) = Arm7Tdmi::barrel_shift(0x00000001, ShiftType::Ror, 1, false);
        assert_eq!(result, 0x80000000);
    }

    #[test]
    fn test_barrel_shift_rrx() {
        // ROR #0 = RRX
        let (result, carry) = Arm7Tdmi::barrel_shift(0x00000003, ShiftType::Ror, 0, true);
        assert_eq!(result, 0x80000001); // carry in at bit 31, bit 0 dropped
        assert!(carry); // old bit 0
    }
}
