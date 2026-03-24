// =============================================================================
// GBA Hardware Constants
// =============================================================================

/// Master clock frequency: 16.777216 MHz
pub const CLOCK_FREQ: u32 = 16_777_216;

/// Frames per second: ~59.7275 Hz
pub const FRAME_RATE: f64 = 59.7275;

/// CPU cycles per frame: 280,896
pub const CYCLES_PER_FRAME: u32 = 280_896;

/// CPU cycles per scanline: 1,232
pub const CYCLES_PER_SCANLINE: u32 = 1_232;

/// Visible scanlines (drawing phase)
pub const VISIBLE_LINES: u32 = 160;

/// VBlank scanlines
pub const VBLANK_LINES: u32 = 68;

/// Total scanlines per frame
pub const TOTAL_LINES: u32 = VISIBLE_LINES + VBLANK_LINES;

/// Cycles spent drawing per scanline
pub const HDRAW_CYCLES: u32 = 1_006;

/// Cycles spent in HBlank per scanline
pub const HBLANK_CYCLES: u32 = 226;

/// Screen width in pixels
pub const SCREEN_WIDTH: u32 = 240;

/// Screen height in pixels
pub const SCREEN_HEIGHT: u32 = 160;

/// Framebuffer size in pixels
pub const FRAMEBUFFER_PIXELS: usize = (SCREEN_WIDTH * SCREEN_HEIGHT) as usize;

// =============================================================================
// Memory Region Base Addresses
// =============================================================================

pub const BIOS_START: u32 = 0x0000_0000;
pub const BIOS_END: u32 = 0x0000_3FFF;
pub const BIOS_SIZE: usize = 0x4000; // 16 KB

pub const EWRAM_START: u32 = 0x0200_0000;
pub const EWRAM_END: u32 = 0x0203_FFFF;
pub const EWRAM_SIZE: usize = 0x4_0000; // 256 KB

pub const IWRAM_START: u32 = 0x0300_0000;
pub const IWRAM_END: u32 = 0x0300_7FFF;
pub const IWRAM_SIZE: usize = 0x8000; // 32 KB

pub const IO_START: u32 = 0x0400_0000;
pub const IO_END: u32 = 0x0400_03FE;
pub const IO_SIZE: usize = 0x400; // 1 KB

pub const PALETTE_START: u32 = 0x0500_0000;
pub const PALETTE_END: u32 = 0x0500_03FF;
pub const PALETTE_SIZE: usize = 0x400; // 1 KB

pub const VRAM_START: u32 = 0x0600_0000;
pub const VRAM_END: u32 = 0x0601_7FFF;
pub const VRAM_SIZE: usize = 0x1_8000; // 96 KB

pub const OAM_START: u32 = 0x0700_0000;
pub const OAM_END: u32 = 0x0700_03FF;
pub const OAM_SIZE: usize = 0x400; // 1 KB

pub const ROM_START: u32 = 0x0800_0000;
pub const ROM_END: u32 = 0x09FF_FFFF;
pub const ROM_MAX_SIZE: usize = 0x200_0000; // 32 MB

/// ROM mirror regions (wait state 1 & 2)
pub const ROM_WS1_START: u32 = 0x0A00_0000;
pub const ROM_WS2_START: u32 = 0x0C00_0000;

pub const SRAM_START: u32 = 0x0E00_0000;
pub const SRAM_END: u32 = 0x0E00_FFFF;
pub const SRAM_SIZE: usize = 0x1_0000; // 64 KB (can be up to 128 KB with banking)

// =============================================================================
// I/O Register Offsets (relative to 0x0400_0000)
// =============================================================================

// Display
pub const REG_DISPCNT: u32 = 0x000;
pub const REG_GREENSWAP: u32 = 0x002;
pub const REG_DISPSTAT: u32 = 0x004;
pub const REG_VCOUNT: u32 = 0x006;

// Backgrounds
pub const REG_BG0CNT: u32 = 0x008;
pub const REG_BG1CNT: u32 = 0x00A;
pub const REG_BG2CNT: u32 = 0x00C;
pub const REG_BG3CNT: u32 = 0x00E;
pub const REG_BG0HOFS: u32 = 0x010;
pub const REG_BG0VOFS: u32 = 0x012;
pub const REG_BG1HOFS: u32 = 0x014;
pub const REG_BG1VOFS: u32 = 0x016;
pub const REG_BG2HOFS: u32 = 0x018;
pub const REG_BG2VOFS: u32 = 0x01A;
pub const REG_BG3HOFS: u32 = 0x01C;
pub const REG_BG3VOFS: u32 = 0x01E;

// BG2/BG3 Rotation/Scaling
pub const REG_BG2PA: u32 = 0x020;
pub const REG_BG2PB: u32 = 0x022;
pub const REG_BG2PC: u32 = 0x024;
pub const REG_BG2PD: u32 = 0x026;
pub const REG_BG2X: u32 = 0x028;
pub const REG_BG2Y: u32 = 0x02C;
pub const REG_BG3PA: u32 = 0x030;
pub const REG_BG3PB: u32 = 0x032;
pub const REG_BG3PC: u32 = 0x034;
pub const REG_BG3PD: u32 = 0x036;
pub const REG_BG3X: u32 = 0x038;
pub const REG_BG3Y: u32 = 0x03C;

// Window
pub const REG_WIN0H: u32 = 0x040;
pub const REG_WIN1H: u32 = 0x042;
pub const REG_WIN0V: u32 = 0x044;
pub const REG_WIN1V: u32 = 0x046;
pub const REG_WININ: u32 = 0x048;
pub const REG_WINOUT: u32 = 0x04A;

// Special Effects
pub const REG_MOSAIC: u32 = 0x04C;
pub const REG_BLDCNT: u32 = 0x050;
pub const REG_BLDALPHA: u32 = 0x052;
pub const REG_BLDY: u32 = 0x054;

// Sound
pub const REG_SOUND1CNT_L: u32 = 0x060;
pub const REG_SOUND1CNT_H: u32 = 0x062;
pub const REG_SOUND1CNT_X: u32 = 0x064;
pub const REG_SOUND2CNT_L: u32 = 0x068;
pub const REG_SOUND2CNT_H: u32 = 0x06C;
pub const REG_SOUND3CNT_L: u32 = 0x070;
pub const REG_SOUND3CNT_H: u32 = 0x072;
pub const REG_SOUND3CNT_X: u32 = 0x074;
pub const REG_SOUND4CNT_L: u32 = 0x078;
pub const REG_SOUND4CNT_H: u32 = 0x07C;
pub const REG_SOUNDCNT_L: u32 = 0x080;
pub const REG_SOUNDCNT_H: u32 = 0x082;
pub const REG_SOUNDCNT_X: u32 = 0x084;
pub const REG_SOUNDBIAS: u32 = 0x088;
pub const REG_WAVE_RAM: u32 = 0x090;
pub const REG_FIFO_A: u32 = 0x0A0;
pub const REG_FIFO_B: u32 = 0x0A4;

// DMA
pub const REG_DMA0SAD: u32 = 0x0B0;
pub const REG_DMA0DAD: u32 = 0x0B4;
pub const REG_DMA0CNT_L: u32 = 0x0B8;
pub const REG_DMA0CNT_H: u32 = 0x0BA;
pub const REG_DMA1SAD: u32 = 0x0BC;
pub const REG_DMA1DAD: u32 = 0x0C0;
pub const REG_DMA1CNT_L: u32 = 0x0C4;
pub const REG_DMA1CNT_H: u32 = 0x0C6;
pub const REG_DMA2SAD: u32 = 0x0C8;
pub const REG_DMA2DAD: u32 = 0x0CC;
pub const REG_DMA2CNT_L: u32 = 0x0D0;
pub const REG_DMA2CNT_H: u32 = 0x0D2;
pub const REG_DMA3SAD: u32 = 0x0D4;
pub const REG_DMA3DAD: u32 = 0x0D8;
pub const REG_DMA3CNT_L: u32 = 0x0DC;
pub const REG_DMA3CNT_H: u32 = 0x0DE;

// Timers
pub const REG_TM0CNT_L: u32 = 0x100;
pub const REG_TM0CNT_H: u32 = 0x102;
pub const REG_TM1CNT_L: u32 = 0x104;
pub const REG_TM1CNT_H: u32 = 0x106;
pub const REG_TM2CNT_L: u32 = 0x108;
pub const REG_TM2CNT_H: u32 = 0x10A;
pub const REG_TM3CNT_L: u32 = 0x10C;
pub const REG_TM3CNT_H: u32 = 0x10E;

// Keypad
pub const REG_KEYINPUT: u32 = 0x130;
pub const REG_KEYCNT: u32 = 0x132;

// Interrupt
pub const REG_IE: u32 = 0x200;
pub const REG_IF: u32 = 0x202;
pub const REG_WAITCNT: u32 = 0x204;
pub const REG_IME: u32 = 0x208;

// System
pub const REG_HALTCNT: u32 = 0x301;

// =============================================================================
// Interrupt Bit Flags
// =============================================================================

pub const IRQ_VBLANK: u16 = 1 << 0;
pub const IRQ_HBLANK: u16 = 1 << 1;
pub const IRQ_VCOUNT: u16 = 1 << 2;
pub const IRQ_TIMER0: u16 = 1 << 3;
pub const IRQ_TIMER1: u16 = 1 << 4;
pub const IRQ_TIMER2: u16 = 1 << 5;
pub const IRQ_TIMER3: u16 = 1 << 6;
pub const IRQ_SERIAL: u16 = 1 << 7;
pub const IRQ_DMA0: u16 = 1 << 8;
pub const IRQ_DMA1: u16 = 1 << 9;
pub const IRQ_DMA2: u16 = 1 << 10;
pub const IRQ_DMA3: u16 = 1 << 11;
pub const IRQ_KEYPAD: u16 = 1 << 12;
pub const IRQ_GAMEPAK: u16 = 1 << 13;
