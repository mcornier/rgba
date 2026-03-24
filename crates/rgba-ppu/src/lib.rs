mod render;

use rgba_core::constants::*;
use serde::{Deserialize, Serialize};

pub use render::*;

/// PPU (Picture Processing Unit) state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ppu {
    /// Current scanline (0-227)
    pub vcount: u16,
    /// Cycle within the current scanline
    pub dot: u32,
    /// Framebuffer: 240x160 pixels, RGB555 format
    pub framebuffer: Vec<u16>,
    /// Per-scanline priority buffer (for sorting layers)
    pub priority_buffer: Vec<u8>,
    /// Frame complete flag (set when VBlank starts)
    pub frame_ready: bool,
    /// Internal BG2/BG3 reference point X (28.4 fixed-point)
    pub bg2_ref_x: i32,
    pub bg2_ref_y: i32,
    pub bg3_ref_x: i32,
    pub bg3_ref_y: i32,
}

impl Ppu {
    pub fn new() -> Self {
        Self {
            vcount: 0,
            dot: 0,
            framebuffer: vec![0; FRAMEBUFFER_PIXELS],
            priority_buffer: vec![4; SCREEN_WIDTH as usize],
            frame_ready: false,
            bg2_ref_x: 0,
            bg2_ref_y: 0,
            bg3_ref_x: 0,
            bg3_ref_y: 0,
        }
    }

    /// Render one scanline (called at the end of H-Draw)
    pub fn render_scanline(
        &mut self,
        io: &[u8],
        palette: &[u8],
        vram: &[u8],
        oam: &[u8],
    ) {
        if self.vcount >= VISIBLE_LINES as u16 {
            return;
        }

        let dispcnt = read16(io, REG_DISPCNT as usize);
        let mode = dispcnt & 7;
        let y = self.vcount;

        // Clear scanline
        let start = y as usize * SCREEN_WIDTH as usize;
        let line = &mut self.framebuffer[start..start + SCREEN_WIDTH as usize];
        let backdrop = read16(palette, 0);
        for pixel in line.iter_mut() {
            *pixel = backdrop;
        }
        for p in self.priority_buffer.iter_mut() {
            *p = 4;
        }

        match mode {
            0 => self.render_mode0(y, io, palette, vram, oam),
            1 => self.render_mode1(y, io, palette, vram, oam),
            2 => self.render_mode2(y, io, palette, vram, oam),
            3 => self.render_mode3(y, io, palette, vram),
            4 => self.render_mode4(y, io, palette, vram),
            5 => self.render_mode5(y, io, palette, vram),
            _ => {} // modes 6-7 are invalid
        }

        // Render sprites on top
        let obj_enable = dispcnt & (1 << 12) != 0;
        if obj_enable {
            self.render_sprites(y, io, palette, vram, oam);
        }

        // Apply windowing and blending effects
        self.apply_effects(y, io, palette);
    }

    /// Advance PPU by one scanline
    pub fn advance_scanline(&mut self) -> PpuEvent {
        self.vcount += 1;

        if self.vcount == VISIBLE_LINES as u16 {
            self.frame_ready = true;
            PpuEvent::VBlankStart
        } else if self.vcount >= TOTAL_LINES as u16 {
            self.vcount = 0;
            self.frame_ready = false;
            PpuEvent::FrameStart
        } else if self.vcount < VISIBLE_LINES as u16 {
            PpuEvent::Draw
        } else {
            PpuEvent::VBlank
        }
    }

    /// Latch internal reference points from I/O (called on write to BG2X/Y, BG3X/Y)
    pub fn latch_ref_points(&mut self, io: &[u8]) {
        self.bg2_ref_x = sign_extend_28(read32(io, REG_BG2X as usize));
        self.bg2_ref_y = sign_extend_28(read32(io, REG_BG2Y as usize));
        self.bg3_ref_x = sign_extend_28(read32(io, REG_BG3X as usize));
        self.bg3_ref_y = sign_extend_28(read32(io, REG_BG3Y as usize));
    }

    /// Get the framebuffer as RGBA8888 for display
    pub fn framebuffer_rgba(&self) -> Vec<u8> {
        let mut rgba = Vec::with_capacity(FRAMEBUFFER_PIXELS * 4);
        for &pixel in &self.framebuffer {
            let r = ((pixel & 0x1F) << 3) as u8;
            let g = (((pixel >> 5) & 0x1F) << 3) as u8;
            let b = (((pixel >> 10) & 0x1F) << 3) as u8;
            rgba.push(r);
            rgba.push(g);
            rgba.push(b);
            rgba.push(255);
        }
        rgba
    }
}

impl Default for Ppu {
    fn default() -> Self {
        Self::new()
    }
}

/// Events emitted by the PPU
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PpuEvent {
    Draw,
    VBlankStart,
    VBlank,
    FrameStart,
}

// =============================================================================
// Helpers
// =============================================================================

pub(crate) fn read16(mem: &[u8], offset: usize) -> u16 {
    if offset + 1 < mem.len() {
        mem[offset] as u16 | ((mem[offset + 1] as u16) << 8)
    } else {
        0
    }
}

pub(crate) fn read32(mem: &[u8], offset: usize) -> u32 {
    if offset + 3 < mem.len() {
        mem[offset] as u32
            | ((mem[offset + 1] as u32) << 8)
            | ((mem[offset + 2] as u32) << 16)
            | ((mem[offset + 3] as u32) << 24)
    } else {
        0
    }
}

fn sign_extend_28(val: u32) -> i32 {
    if val & (1 << 27) != 0 {
        (val | 0xF000_0000) as i32
    } else {
        (val & 0x0FFF_FFFF) as i32
    }
}
