// =============================================================================
// ARM Instruction Decoding & Execution
// =============================================================================
//
// ARM instructions are 32-bit, condition code in bits [31:28].
// Instruction format determined by bits [27:20] and [7:4].
//
// Major instruction groups:
// - Data Processing (AND, EOR, SUB, RSB, ADD, ADC, SBC, RSC, TST, TEQ, CMP, CMN, ORR, MOV, BIC, MVN)
// - Multiply (MUL, MLA, UMULL, UMLAL, SMULL, SMLAL)
// - Single Data Transfer (LDR, STR, LDRB, STRB)
// - Halfword/Signed Data Transfer (LDRH, STRH, LDRSB, LDRSH)
// - Block Data Transfer (LDM, STM)
// - Branch (B, BL)
// - Branch and Exchange (BX)
// - Software Interrupt (SWI)
// - MRS/MSR (status register access)
//
// TODO: Full implementation in US-03/US-04
