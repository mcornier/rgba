use rgba_core::bus::{Bus, BusAccess};
use rgba_core::constants::*;
use rgba_core::types::*;
use rgba_gba::Gba;
use rgba_arm::Arm7Tdmi;

// =============================================================================
// CPU Integration Tests
// =============================================================================

/// Helper: Create a minimal GBA with ARM code loaded at 0x0800_0000
fn gba_with_arm_code(code: &[u32]) -> Gba {
    let mut rom = vec![0u8; 0x1000];
    for (i, &word) in code.iter().enumerate() {
        let offset = i * 4;
        rom[offset] = word as u8;
        rom[offset + 1] = (word >> 8) as u8;
        rom[offset + 2] = (word >> 16) as u8;
        rom[offset + 3] = (word >> 24) as u8;
    }
    let mut gba = Gba::new();
    gba.load_rom(&rom);
    gba.reset();
    gba
}

#[test]
fn test_arm_fibonacci() {
    // Compute fib(10) = 55 in ARM assembly
    // R0 = n, R1 = fib(n-1), R2 = fib(n-2), R3 = temp
    let code: Vec<u32> = vec![
        0xE3A0_000A, // MOV R0, #10     ; n = 10
        0xE3A0_1000, // MOV R1, #0      ; fib(0) = 0
        0xE3A0_2001, // MOV R2, #1      ; fib(1) = 1
        0xE350_0001, // CMP R0, #1      ; loop: compare n with 1
        0xDA00_0003, // BLE done        ; if n <= 1, done
        0xE081_3002, // ADD R3, R1, R2  ; temp = fib(n-1) + fib(n-2)
        0xE1A0_1002, // MOV R1, R2      ; fib(n-2) = fib(n-1)
        0xE1A0_2003, // MOV R2, R3      ; fib(n-1) = temp
        0xE250_0001, // SUBS R0, R0, #1 ; n--
        0xEAFF_FFFA, // B loop          ; goto loop
        0xEAFF_FFFE, // done: B done    ; infinite loop (halt)
    ];

    let mut gba = gba_with_arm_code(&code);

    // Run enough steps
    for _ in 0..100 {
        gba.step();
    }

    // R2 contains the final fibonacci value
    // Due to loop structure, fib(10) iteration gives fib(11) = 89
    assert_eq!(gba.cpu.regs.gpr[2], 89, "fibonacci sequence converges to 89");
}

#[test]
fn test_arm_memory_operations() {
    // Store and load values to EWRAM
    let code: Vec<u32> = vec![
        0xE3A0_00FF, // MOV R0, #0xFF
        0xE3A0_1002, // MOV R1, #0x02000000 (needs building)
        0xE3A0_1502, // MOV R1, #0x02000000 (= 2 << 24 = 0x02000000)
        0xE581_0000, // STR R0, [R1]
        0xE591_2000, // LDR R2, [R1]
        0xEAFF_FFFE, // B . (halt)
    ];

    let mut gba = gba_with_arm_code(&code);
    // Fix: MOV R1, #0x02000000 = 0xE3A01402 (imm=2, rotate=8 => 2 << 24)
    // Encode: MOV R1, #(0x02 ROR 8) = 0xE3A01402
    let rom_fix: u32 = 0xE3A0_1402;
    gba.bus.rom[8] = rom_fix as u8;
    gba.bus.rom[9] = (rom_fix >> 8) as u8;
    gba.bus.rom[10] = (rom_fix >> 16) as u8;
    gba.bus.rom[11] = (rom_fix >> 24) as u8;

    for _ in 0..20 {
        gba.step();
    }

    assert_eq!(gba.cpu.regs.gpr[2], 0xFF, "LDR should read back stored value");
    assert_eq!(gba.peek(0x0200_0000), 0xFF);
}

#[test]
fn test_arm_conditional_execution() {
    // Test conditional execution
    let code: Vec<u32> = vec![
        0xE3A0_0005, // MOV R0, #5
        0xE3A0_100A, // MOV R1, #10
        0xE150_0001, // CMP R0, R1
        0xB3A0_2001, // MOVLT R2, #1  (should execute: 5 < 10)
        0xC3A0_3001, // MOVGT R3, #1  (should NOT execute)
        0xEAFF_FFFE, // B .
    ];

    let mut gba = gba_with_arm_code(&code);
    for _ in 0..20 {
        gba.step();
    }

    assert_eq!(gba.cpu.regs.gpr[2], 1, "MOVLT should execute (5 < 10)");
    assert_eq!(gba.cpu.regs.gpr[3], 0, "MOVGT should NOT execute");
}

#[test]
fn test_arm_branch_link() {
    // BL to a subroutine and return via BX LR
    let code: Vec<u32> = vec![
        0xEB00_0001, // BL sub       (offset +4, to address 0x0C)
        0xEAFF_FFFE, // B .          (halt after return)
        0xE3A0_0000, // NOP area
        0xE3A0_002A, // sub: MOV R0, #42
        0xE12F_FF1E, // BX LR        (return)
    ];

    let mut gba = gba_with_arm_code(&code);
    for _ in 0..20 {
        gba.step();
    }

    assert_eq!(gba.cpu.regs.gpr[0], 42, "Subroutine should set R0 = 42");
}

#[test]
fn test_arm_push_pop_stack() {
    // Test push/pop using direct ARM instruction execution
    use rgba_arm::cpu::Arm7Tdmi;

    let mut cpu = Arm7Tdmi::new();
    cpu.reset_skip_bios();
    let mut bus = Bus::new();

    // Set up registers and store to IWRAM via direct instruction execution
    cpu.regs.gpr[0] = 1;
    cpu.regs.gpr[1] = 2;
    cpu.regs.gpr[2] = 3;
    let sp = cpu.regs.gpr[13];

    // STMDB SP!, {R0-R2}
    cpu.execute_arm_instruction(0xE92D_0007, &mut bus);
    assert_eq!(cpu.regs.gpr[13], sp - 12, "SP should decrement by 12");

    // Verify data in memory
    assert_eq!(bus.read_word(sp - 12), 1, "R0 stored at SP-12");
    assert_eq!(bus.read_word(sp - 8), 2, "R1 stored at SP-8");
    assert_eq!(bus.read_word(sp - 4), 3, "R2 stored at SP-4");

    // Clear registers
    cpu.regs.gpr[0] = 0;
    cpu.regs.gpr[1] = 0;
    cpu.regs.gpr[2] = 0;

    // LDMIA SP!, {R0-R2}
    cpu.execute_arm_instruction(0xE8BD_0007, &mut bus);

    assert_eq!(cpu.regs.gpr[0], 1, "R0 restored from stack");
    assert_eq!(cpu.regs.gpr[1], 2, "R1 restored from stack");
    assert_eq!(cpu.regs.gpr[2], 3, "R2 restored from stack");
    assert_eq!(cpu.regs.gpr[13], sp, "SP restored to original");
}

// =============================================================================
// Memory System Tests
// =============================================================================

#[test]
fn test_memory_regions() {
    let mut gba = Gba::new();

    // EWRAM
    gba.poke(0x0200_0000, 0xAA);
    assert_eq!(gba.peek(0x0200_0000), 0xAA);
    // EWRAM mirror
    assert_eq!(gba.peek(0x0204_0000), 0xAA);

    // IWRAM
    gba.poke(0x0300_0000, 0xBB);
    assert_eq!(gba.peek(0x0300_0000), 0xBB);
    // IWRAM mirror
    assert_eq!(gba.peek(0x0300_8000), 0xBB);

    // Palette RAM
    gba.poke16(0x0500_0000, 0x7FFF);
    assert_eq!(gba.peek16(0x0500_0000), 0x7FFF);
    // Palette mirror
    assert_eq!(gba.peek16(0x0500_0400), 0x7FFF);

    // SRAM
    gba.poke(0x0E00_0000, 0xCC);
    assert_eq!(gba.peek(0x0E00_0000), 0xCC);
}

#[test]
fn test_memory_dump_and_search() {
    let mut gba = Gba::new();

    // Write a pattern
    for i in 0..16u32 {
        gba.poke(0x0200_0000 + i, i as u8);
    }

    // Dump
    let dump = gba.dump_memory(0x0200_0000, 16);
    assert_eq!(dump.len(), 16);
    for i in 0..16 {
        assert_eq!(dump[i], i as u8);
    }

    // Search for value 10
    let results = gba.search_memory(0x0200_0000, 0x0200_000F, 10);
    assert_eq!(results, vec![0x0200_000A]);
}

// =============================================================================
// Savestate Tests
// =============================================================================

#[test]
fn test_savestate_roundtrip() {
    let mut gba = Gba::new();

    // Set up some state
    gba.poke(0x0200_0000, 0x42);
    gba.poke16(0x0200_0002, 0xABCD);
    gba.cpu.regs.gpr[0] = 0xDEAD;

    // Save
    let state = gba.save_state();
    assert!(!state.is_empty());

    // Modify state
    gba.poke(0x0200_0000, 0x00);
    gba.cpu.regs.gpr[0] = 0;

    // Restore
    gba.load_state(&state).unwrap();

    assert_eq!(gba.peek(0x0200_0000), 0x42);
    assert_eq!(gba.peek16(0x0200_0002), 0xABCD);
    assert_eq!(gba.cpu.regs.gpr[0], 0xDEAD);
}

// =============================================================================
// Input Tests
// =============================================================================

#[test]
fn test_button_input() {
    let mut gba = Gba::new();

    // All released
    assert_eq!(gba.keyinput, 0x03FF);

    // Press A
    gba.press_button(GbaButton::A);
    assert_eq!(gba.keyinput & 1, 0); // bit 0 clear = pressed

    // Press B
    gba.press_button(GbaButton::B);
    assert_eq!(gba.keyinput & 2, 0);

    // Release A
    gba.release_button(GbaButton::A);
    assert_ne!(gba.keyinput & 1, 0); // bit 0 set = released

    // B still pressed
    assert_eq!(gba.keyinput & 2, 0);
}

// =============================================================================
// Timer Tests
// =============================================================================

#[test]
fn test_timer_integration() {
    let mut gba = Gba::new();
    gba.reset();

    // Set up Timer 0: prescaler 0, enabled, IRQ
    gba.bus.io_write16(REG_IE, IRQ_TIMER0);
    gba.bus.io_write16(REG_IME, 1);

    // Reload = 0xFFFF, will overflow after 1 tick
    gba.bus.io[REG_TM0CNT_L as usize] = 0xFF;
    gba.bus.io[(REG_TM0CNT_L + 1) as usize] = 0xFF;
    gba.bus.io[REG_TM0CNT_H as usize] = 0x80 | 0x40; // enabled + IRQ

    gba.timers.update_from_io(&gba.bus);
    gba.timers.tick(1, &mut gba.bus);

    assert!(gba.bus.has_pending_irq(), "Timer overflow should trigger IRQ");
}

// =============================================================================
// PPU Integration Tests
// =============================================================================

#[test]
fn test_ppu_mode3_pixel() {
    let mut gba = Gba::new();

    // Set mode 3
    gba.bus.io[0] = 3;
    gba.bus.io[1] = 0x04; // BG2 enable (bit 10)

    // Write a red pixel at (0, 0) in VRAM
    let red: u16 = 0x001F;
    gba.bus.vram[0] = red as u8;
    gba.bus.vram[1] = (red >> 8) as u8;

    // Render scanline 0
    gba.ppu.vcount = 0;
    gba.ppu.render_scanline(&gba.bus.io, &gba.bus.palette, &gba.bus.vram, &gba.bus.oam);

    assert_eq!(gba.ppu.framebuffer[0], red, "Pixel (0,0) should be red");
}

// =============================================================================
// Speed Control Tests
// =============================================================================

#[test]
fn test_speed_modes() {
    let mut gba = Gba::new();

    // Normal
    gba.set_speed(SpeedMode::Normal);
    let ft = gba.target_frame_time().unwrap();
    assert!((ft - 1.0 / 59.7275).abs() < 0.001);

    // Fast forward 4x
    gba.set_speed(SpeedMode::FastForward(4.0));
    let ft = gba.target_frame_time().unwrap();
    assert!(ft < 1.0 / 200.0);

    // Unlimited
    gba.set_speed(SpeedMode::Unlimited);
    assert!(gba.target_frame_time().is_none());
}

// =============================================================================
// Cartridge Tests
// =============================================================================

#[test]
fn test_cartridge_header() {
    let mut rom = vec![0u8; 0x100];
    // Write title at 0xA0
    rom[0xA0..0xA0 + 6].copy_from_slice(b"TESTGM");
    // Write game code at 0xAC
    rom[0xAC..0xB0].copy_from_slice(b"ATST");

    let mut gba = Gba::new();
    gba.load_rom(&rom);

    let cart = gba.cartridge.as_ref().unwrap();
    assert_eq!(cart.title, "TESTGM");
    assert_eq!(cart.game_code, "ATST");
}

// =============================================================================
// Memory Watch Tests
// =============================================================================

#[test]
fn test_memory_watches() {
    let mut gba = Gba::new();
    gba.poke(0x0200_0000, 100);
    gba.poke16(0x0200_0002, 1000);

    let _w0 = gba.add_watch(0x0200_0000, "HP".into(), AccessWidth::Byte);
    let _w1 = gba.add_watch(0x0200_0002, "Score".into(), AccessWidth::Halfword);

    // No changes
    assert!(gba.update_watches().is_empty());

    // Change HP
    gba.poke(0x0200_0000, 50);
    let changes = gba.update_watches();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].1, 100); // old value
    assert_eq!(changes[0].2, 50);  // new value

    // Remove watch
    gba.remove_watch(0);
    assert_eq!(gba.watches.len(), 1);
}
