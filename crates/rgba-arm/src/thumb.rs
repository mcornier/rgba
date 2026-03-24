// =============================================================================
// THUMB Instruction Decoding & Execution
// =============================================================================
//
// THUMB instructions are 16-bit, providing higher code density.
// No condition codes (except conditional branches).
//
// Major instruction groups:
// - Move shifted register
// - Add/subtract
// - Move/compare/add/subtract immediate
// - ALU operations
// - Hi register operations / BX
// - PC-relative load
// - Load/store with register offset
// - Load/store sign-extended byte/halfword
// - Load/store with immediate offset
// - Load/store halfword
// - SP-relative load/store
// - Load address (PC/SP relative)
// - Add offset to SP
// - Push/pop registers
// - Multiple load/store
// - Conditional branch
// - Software interrupt
// - Unconditional branch
// - Long branch with link (BL, two-instruction sequence)
//
// TODO: Full implementation in US-05
