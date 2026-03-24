use rgba_core::constants::*;
use serde::{Deserialize, Serialize};

/// PPU (Picture Processing Unit) state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ppu {
    /// Current scanline (0-227)
    pub vcount: u16,
    /// Cycle within the current scanline
    pub dot: u32,
    /// Framebuffer: 240x160 pixels, RGB555 format
    pub framebuffer: Vec<u16>,
    /// Frame complete flag (set when VBlank starts)
    pub frame_ready: bool,
}

impl Ppu {
    pub fn new() -> Self {
        Self {
            vcount: 0,
            dot: 0,
            framebuffer: vec![0; FRAMEBUFFER_PIXELS],
            frame_ready: false,
        }
    }

    /// Render one scanline (called at the end of H-Draw)
    pub fn render_scanline(
        &mut self,
        _io: &[u8],
        _palette: &[u8],
        _vram: &[u8],
        _oam: &[u8],
    ) {
        if self.vcount >= VISIBLE_LINES as u16 {
            return;
        }

        // TODO: Implement per-mode rendering (US-10, US-11, US-12)
        // For now, clear the scanline
        let y = self.vcount as usize;
        let start = y * SCREEN_WIDTH as usize;
        let end = start + SCREEN_WIDTH as usize;
        for pixel in &mut self.framebuffer[start..end] {
            *pixel = 0; // black
        }
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
    /// Drawing a visible scanline
    Draw,
    /// VBlank just started (frame complete)
    VBlankStart,
    /// Still in VBlank
    VBlank,
    /// New frame starting
    FrameStart,
}
