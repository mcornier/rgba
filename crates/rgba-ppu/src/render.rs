use rgba_core::constants::*;

use crate::{read16, Ppu};

// =============================================================================
// Background Rendering
// =============================================================================

impl Ppu {
    /// Mode 0: 4 text backgrounds
    pub(crate) fn render_mode0(
        &mut self,
        y: u16,
        io: &[u8],
        palette: &[u8],
        vram: &[u8],
        _oam: &[u8],
    ) {
        let dispcnt = read16(io, REG_DISPCNT as usize);
        // Render BG0-BG3 from lowest priority to highest
        for bg_idx in (0..4).rev() {
            if dispcnt & (1 << (8 + bg_idx)) == 0 {
                continue;
            }
            self.render_text_bg(bg_idx, y, io, palette, vram);
        }
    }

    /// Mode 1: BG0, BG1 text + BG2 affine
    pub(crate) fn render_mode1(
        &mut self,
        y: u16,
        io: &[u8],
        palette: &[u8],
        vram: &[u8],
        _oam: &[u8],
    ) {
        let dispcnt = read16(io, REG_DISPCNT as usize);

        // BG2 affine (lowest priority among enabled)
        if dispcnt & (1 << 10) != 0 {
            self.render_affine_bg(2, y, io, palette, vram);
        }
        // BG1
        if dispcnt & (1 << 9) != 0 {
            self.render_text_bg(1, y, io, palette, vram);
        }
        // BG0
        if dispcnt & (1 << 8) != 0 {
            self.render_text_bg(0, y, io, palette, vram);
        }
    }

    /// Mode 2: BG2, BG3 affine
    pub(crate) fn render_mode2(
        &mut self,
        y: u16,
        io: &[u8],
        palette: &[u8],
        vram: &[u8],
        _oam: &[u8],
    ) {
        let dispcnt = read16(io, REG_DISPCNT as usize);
        if dispcnt & (1 << 11) != 0 {
            self.render_affine_bg(3, y, io, palette, vram);
        }
        if dispcnt & (1 << 10) != 0 {
            self.render_affine_bg(2, y, io, palette, vram);
        }
    }

    /// Mode 3: Single 240x160 16-bit bitmap (full color)
    pub(crate) fn render_mode3(
        &mut self,
        y: u16,
        _io: &[u8],
        _palette: &[u8],
        vram: &[u8],
    ) {
        let line_start = y as usize * SCREEN_WIDTH as usize;
        for x in 0..SCREEN_WIDTH as usize {
            let vram_offset = (y as usize * SCREEN_WIDTH as usize + x) * 2;
            if vram_offset + 1 < vram.len() {
                let color = read16(vram, vram_offset);
                self.framebuffer[line_start + x] = color;
            }
        }
    }

    /// Mode 4: Single 240x160 8-bit indexed bitmap (page-flipped)
    pub(crate) fn render_mode4(
        &mut self,
        y: u16,
        io: &[u8],
        palette: &[u8],
        vram: &[u8],
    ) {
        let dispcnt = read16(io, REG_DISPCNT as usize);
        let page_offset = if dispcnt & (1 << 4) != 0 { 0xA000 } else { 0 };
        let line_start = y as usize * SCREEN_WIDTH as usize;

        for x in 0..SCREEN_WIDTH as usize {
            let idx = page_offset + y as usize * SCREEN_WIDTH as usize + x;
            if idx < vram.len() {
                let color_idx = vram[idx] as usize;
                if color_idx != 0 {
                    let color = read16(palette, color_idx * 2);
                    self.framebuffer[line_start + x] = color;
                }
            }
        }
    }

    /// Mode 5: 160x128 16-bit bitmap (page-flipped)
    pub(crate) fn render_mode5(
        &mut self,
        y: u16,
        io: &[u8],
        _palette: &[u8],
        vram: &[u8],
    ) {
        if y >= 128 {
            return;
        }
        let dispcnt = read16(io, REG_DISPCNT as usize);
        let page_offset = if dispcnt & (1 << 4) != 0 { 0xA000 } else { 0 };
        let line_start = y as usize * SCREEN_WIDTH as usize;

        for x in 0..160usize {
            let vram_offset = page_offset + (y as usize * 160 + x) * 2;
            if vram_offset + 1 < vram.len() {
                let color = read16(vram, vram_offset);
                self.framebuffer[line_start + x] = color;
            }
        }
    }

    // =========================================================================
    // Text Background Rendering
    // =========================================================================

    fn render_text_bg(
        &mut self,
        bg: usize,
        y: u16,
        io: &[u8],
        palette: &[u8],
        vram: &[u8],
    ) {
        let bgcnt = read16(io, (REG_BG0CNT + bg as u32 * 2) as usize);
        let priority = bgcnt & 3;
        let tile_base = ((bgcnt >> 2) & 3) as usize * 0x4000;
        let mosaic = bgcnt & (1 << 6) != 0;
        let palette_mode = bgcnt & (1 << 7) != 0; // 0=16/16, 1=256/1
        let map_base = ((bgcnt >> 8) & 0x1F) as usize * 0x800;
        let screen_size = (bgcnt >> 14) & 3;

        let hofs = read16(io, (REG_BG0HOFS + bg as u32 * 4) as usize) & 0x1FF;
        let vofs = read16(io, (REG_BG0VOFS + bg as u32 * 4) as usize) & 0x1FF;

        let (map_w, map_h) = match screen_size {
            0 => (256u32, 256u32),
            1 => (512, 256),
            2 => (256, 512),
            3 => (512, 512),
            _ => (256, 256),
        };

        let line_start = y as usize * SCREEN_WIDTH as usize;
        let py = (y as u32 + vofs as u32) % map_h;

        for screen_x in 0..SCREEN_WIDTH {
            let px = (screen_x + hofs as u32) % map_w;

            let tile_x = px / 8;
            let tile_y = py / 8;
            let pixel_x = (px % 8) as usize;
            let pixel_y = (py % 8) as usize;

            // Determine which screen block
            let screen_block = match screen_size {
                0 => 0,
                1 => tile_x / 32,
                2 => tile_y / 32,
                3 => (tile_x / 32) + (tile_y / 32) * 2,
                _ => 0,
            };
            let local_tile_x = tile_x % 32;
            let local_tile_y = tile_y % 32;

            let map_offset =
                map_base + screen_block as usize * 0x800 + (local_tile_y * 32 + local_tile_x) as usize * 2;

            if map_offset + 1 >= vram.len() {
                continue;
            }
            let tile_entry = read16(vram, map_offset);
            let tile_id = (tile_entry & 0x3FF) as usize;
            let h_flip = tile_entry & (1 << 10) != 0;
            let v_flip = tile_entry & (1 << 11) != 0;
            let pal_bank = ((tile_entry >> 12) & 0xF) as usize;

            let actual_px = if h_flip { 7 - pixel_x } else { pixel_x };
            let actual_py = if v_flip { 7 - pixel_y } else { pixel_y };

            let color_idx = if palette_mode {
                // 256-color mode
                let offset = tile_base + tile_id * 64 + actual_py * 8 + actual_px;
                if offset < vram.len() {
                    vram[offset] as usize
                } else {
                    0
                }
            } else {
                // 16-color mode
                let offset = tile_base + tile_id * 32 + actual_py * 4 + actual_px / 2;
                if offset < vram.len() {
                    let byte = vram[offset];
                    let nybble = if actual_px & 1 != 0 {
                        (byte >> 4) as usize
                    } else {
                        (byte & 0xF) as usize
                    };
                    if nybble == 0 {
                        0
                    } else {
                        pal_bank * 16 + nybble
                    }
                } else {
                    0
                }
            };

            if color_idx != 0 {
                let x = screen_x as usize;
                if priority as u8 <= self.priority_buffer[x] {
                    let color = read16(palette, color_idx * 2);
                    self.framebuffer[line_start + x] = color;
                    self.priority_buffer[x] = priority as u8;
                }
            }
        }

        let _ = mosaic; // TODO: mosaic effect
    }

    // =========================================================================
    // Affine Background Rendering
    // =========================================================================

    fn render_affine_bg(
        &mut self,
        bg: usize,
        y: u16,
        io: &[u8],
        palette: &[u8],
        vram: &[u8],
    ) {
        let bgcnt = read16(io, (REG_BG0CNT + bg as u32 * 2) as usize);
        let priority = bgcnt & 3;
        let tile_base = ((bgcnt >> 2) & 3) as usize * 0x4000;
        let map_base = ((bgcnt >> 8) & 0x1F) as usize * 0x800;
        let wraparound = bgcnt & (1 << 13) != 0;
        let screen_size = (bgcnt >> 14) & 3;

        let size = match screen_size {
            0 => 128,
            1 => 256,
            2 => 512,
            3 => 1024,
            _ => 128,
        };
        let tiles = size / 8;

        // Affine parameters
        let pa_offset = if bg == 2 { REG_BG2PA } else { REG_BG3PA };
        let pa = read16(io, pa_offset as usize) as i16 as i32;
        let pb = read16(io, (pa_offset + 2) as usize) as i16 as i32;
        let pc = read16(io, (pa_offset + 4) as usize) as i16 as i32;
        let pd = read16(io, (pa_offset + 6) as usize) as i16 as i32;

        let (ref_x, ref_y) = if bg == 2 {
            (self.bg2_ref_x, self.bg2_ref_y)
        } else {
            (self.bg3_ref_x, self.bg3_ref_y)
        };

        // Starting texture coordinates for this scanline
        let mut tex_x = ref_x + pb * y as i32;
        let mut tex_y = ref_y + pd * y as i32;

        let line_start = y as usize * SCREEN_WIDTH as usize;

        for screen_x in 0..SCREEN_WIDTH as usize {
            // Convert from 8.8 fixed-point to pixels
            let px = tex_x >> 8;
            let py = tex_y >> 8;

            tex_x += pa;
            tex_y += pc;

            if !wraparound && (px < 0 || py < 0 || px >= size as i32 || py >= size as i32) {
                continue;
            }

            let px = ((px as u32) % size) as usize;
            let py = ((py as u32) % size) as usize;

            let tile_x = px / 8;
            let tile_y = py / 8;
            let pixel_x = px % 8;
            let pixel_y = py % 8;

            let map_offset = map_base + tile_y * tiles as usize + tile_x;
            if map_offset >= vram.len() {
                continue;
            }
            let tile_id = vram[map_offset] as usize;

            let vram_offset = tile_base + tile_id * 64 + pixel_y * 8 + pixel_x;
            if vram_offset >= vram.len() {
                continue;
            }
            let color_idx = vram[vram_offset] as usize;

            if color_idx != 0 && (priority as u8) <= self.priority_buffer[screen_x] {
                let color = read16(palette, color_idx * 2);
                self.framebuffer[line_start + screen_x] = color;
                self.priority_buffer[screen_x] = priority as u8;
            }
        }
    }

    // =========================================================================
    // Sprite Rendering
    // =========================================================================

    pub(crate) fn render_sprites(
        &mut self,
        y: u16,
        io: &[u8],
        palette: &[u8],
        vram: &[u8],
        oam: &[u8],
    ) {
        let dispcnt = read16(io, REG_DISPCNT as usize);
        let obj_mapping = dispcnt & (1 << 6) != 0; // 0=2D, 1=1D

        let line_start = y as usize * SCREEN_WIDTH as usize;

        // Iterate sprites in reverse order (lower index = higher priority)
        for obj_idx in (0..128).rev() {
            let attr_offset = obj_idx * 8;
            if attr_offset + 5 >= oam.len() {
                continue;
            }

            let attr0 = read16(oam, attr_offset);
            let attr1 = read16(oam, attr_offset + 2);
            let attr2 = read16(oam, attr_offset + 4);

            // Check if sprite is disabled
            let obj_mode = (attr0 >> 8) & 3;
            if obj_mode == 2 {
                continue; // disabled
            }

            let is_affine = obj_mode == 1 || obj_mode == 3;
            let double_size = obj_mode == 3;

            let shape = (attr0 >> 14) & 3;
            let size_idx = (attr1 >> 14) & 3;

            let (width, height) = sprite_dimensions(shape, size_idx);

            let obj_y = (attr0 & 0xFF) as i32;
            let obj_x = ((attr1 & 0x1FF) as i16) as i32;
            let obj_x = if obj_x >= 240 { obj_x - 512 } else { obj_x };

            let render_h = if double_size { height * 2 } else { height } as i32;
            let render_w = if double_size { width * 2 } else { width } as i32;

            // Check if this sprite is on the current scanline
            let sprite_y = y as i32 - obj_y;
            let sprite_y = if obj_y > 160 {
                y as i32 + (256 - obj_y)
            } else {
                sprite_y
            };

            if sprite_y < 0 || sprite_y >= render_h {
                continue;
            }

            let tile_id = (attr2 & 0x3FF) as usize;
            let priority = ((attr2 >> 10) & 3) as u8;
            let palette_bank = ((attr2 >> 12) & 0xF) as usize;
            let palette_256 = attr0 & (1 << 13) != 0;

            let h_flip = !is_affine && attr1 & (1 << 12) != 0;
            let v_flip = !is_affine && attr1 & (1 << 13) != 0;

            for screen_off_x in 0..render_w {
                let screen_x = obj_x + screen_off_x;
                if screen_x < 0 || screen_x >= SCREEN_WIDTH as i32 {
                    continue;
                }

                let (tex_x, tex_y) = if is_affine {
                    let affine_idx = ((attr1 >> 9) & 0x1F) as usize;
                    let pa = read16(oam, affine_idx * 32 + 6) as i16 as i32;
                    let pb = read16(oam, affine_idx * 32 + 14) as i16 as i32;
                    let pc = read16(oam, affine_idx * 32 + 22) as i16 as i32;
                    let pd = read16(oam, affine_idx * 32 + 30) as i16 as i32;

                    let cx = render_w / 2;
                    let cy = render_h / 2;
                    let dx = screen_off_x - cx;
                    let dy = sprite_y - cy;

                    let tx = ((pa * dx + pb * dy) >> 8) + (width as i32 / 2);
                    let ty = ((pc * dx + pd * dy) >> 8) + (height as i32 / 2);

                    if tx < 0 || tx >= width as i32 || ty < 0 || ty >= height as i32 {
                        continue;
                    }
                    (tx as usize, ty as usize)
                } else {
                    let tx = if h_flip {
                        width as i32 - 1 - screen_off_x
                    } else {
                        screen_off_x
                    };
                    let ty = if v_flip {
                        height as i32 - 1 - sprite_y
                    } else {
                        sprite_y
                    };
                    (tx as usize, ty as usize)
                };

                let tile_offset_x = tex_x / 8;
                let tile_offset_y = tex_y / 8;
                let pixel_x = tex_x % 8;
                let pixel_y = tex_y % 8;

                let actual_tile = if obj_mapping {
                    // 1D mapping
                    if palette_256 {
                        tile_id + tile_offset_y * (width as usize / 8) * 2 + tile_offset_x * 2
                    } else {
                        tile_id + tile_offset_y * (width as usize / 8) + tile_offset_x
                    }
                } else {
                    // 2D mapping (32 tiles per row)
                    if palette_256 {
                        tile_id + tile_offset_y * 32 + tile_offset_x * 2
                    } else {
                        tile_id + tile_offset_y * 32 + tile_offset_x
                    }
                };

                let color_idx = if palette_256 {
                    let offset = 0x10000 + actual_tile * 32 + pixel_y * 8 + pixel_x;
                    if offset < vram.len() { vram[offset] as usize } else { 0 }
                } else {
                    let offset = 0x10000 + actual_tile * 32 + pixel_y * 4 + pixel_x / 2;
                    if offset < vram.len() {
                        let byte = vram[offset];
                        let nybble = if pixel_x & 1 != 0 {
                            (byte >> 4) as usize
                        } else {
                            (byte & 0xF) as usize
                        };
                        if nybble == 0 { 0 } else { palette_bank * 16 + nybble }
                    } else {
                        0
                    }
                };

                if color_idx != 0 {
                    let sx = screen_x as usize;
                    if priority <= self.priority_buffer[sx] {
                        // Sprite palette is at offset 0x200 in palette RAM
                        let pal_offset = if palette_256 {
                            0x200 + color_idx * 2
                        } else {
                            0x200 + color_idx * 2
                        };
                        let color = read16(palette, pal_offset);
                        self.framebuffer[line_start + sx] = color;
                        self.priority_buffer[sx] = priority;
                    }
                }
            }
        }
    }

    // =========================================================================
    // Effects (blending, windowing) — stub for now
    // =========================================================================

    pub(crate) fn apply_effects(&mut self, _y: u16, io: &[u8], _palette: &[u8]) {
        let bldcnt = read16(io, REG_BLDCNT as usize);
        let blend_mode = (bldcnt >> 6) & 3;

        if blend_mode == 0 {
            return; // No blending
        }

        // Brightness increase/decrease (mode 2 and 3) applied at framebuffer level
        if blend_mode == 2 || blend_mode == 3 {
            let bldy = (read16(io, REG_BLDY as usize) & 0x1F).min(16);
            if bldy == 0 {
                return;
            }
            let y = _y as usize;
            let start = y * SCREEN_WIDTH as usize;
            for x in 0..SCREEN_WIDTH as usize {
                let color = self.framebuffer[start + x];
                self.framebuffer[start + x] = if blend_mode == 2 {
                    brightness_increase(color, bldy)
                } else {
                    brightness_decrease(color, bldy)
                };
            }
        }

        // TODO: Alpha blending (mode 1) requires tracking layer sources per pixel
    }
}

// =============================================================================
// Sprite dimension lookup
// =============================================================================

fn sprite_dimensions(shape: u16, size: u16) -> (u32, u32) {
    match (shape, size) {
        // Square
        (0, 0) => (8, 8),
        (0, 1) => (16, 16),
        (0, 2) => (32, 32),
        (0, 3) => (64, 64),
        // Horizontal
        (1, 0) => (16, 8),
        (1, 1) => (32, 8),
        (1, 2) => (32, 16),
        (1, 3) => (64, 32),
        // Vertical
        (2, 0) => (8, 16),
        (2, 1) => (8, 32),
        (2, 2) => (16, 32),
        (2, 3) => (32, 64),
        _ => (8, 8),
    }
}

// =============================================================================
// Blending helpers
// =============================================================================

fn brightness_increase(color: u16, evy: u16) -> u16 {
    let r = color & 0x1F;
    let g = (color >> 5) & 0x1F;
    let b = (color >> 10) & 0x1F;

    let r = r + ((31 - r) * evy / 16);
    let g = g + ((31 - g) * evy / 16);
    let b = b + ((31 - b) * evy / 16);

    (r & 0x1F) | ((g & 0x1F) << 5) | ((b & 0x1F) << 10)
}

fn brightness_decrease(color: u16, evy: u16) -> u16 {
    let r = color & 0x1F;
    let g = (color >> 5) & 0x1F;
    let b = (color >> 10) & 0x1F;

    let r = r - (r * evy / 16);
    let g = g - (g * evy / 16);
    let b = b - (b * evy / 16);

    (r & 0x1F) | ((g & 0x1F) << 5) | ((b & 0x1F) << 10)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sprite_dimensions() {
        assert_eq!(sprite_dimensions(0, 0), (8, 8));
        assert_eq!(sprite_dimensions(0, 3), (64, 64));
        assert_eq!(sprite_dimensions(1, 2), (32, 16));
        assert_eq!(sprite_dimensions(2, 1), (8, 32));
    }

    #[test]
    fn test_brightness_increase() {
        // White stays white
        assert_eq!(brightness_increase(0x7FFF, 16), 0x7FFF);
        // Black becomes white at max
        assert_eq!(brightness_increase(0x0000, 16), 0x7FFF);
    }

    #[test]
    fn test_brightness_decrease() {
        // Black stays black
        assert_eq!(brightness_decrease(0x0000, 16), 0x0000);
        // White becomes black at max
        assert_eq!(brightness_decrease(0x7FFF, 16), 0x0000);
    }

    #[test]
    fn test_mode3_render() {
        let mut ppu = Ppu::new();
        ppu.vcount = 0;

        let mut io = vec![0u8; 0x400];
        let mut palette = vec![0u8; 0x400];
        let mut vram = vec![0u8; 0x18000];
        let oam = vec![0u8; 0x400];

        // Set mode 3
        io[0] = 3; // DISPCNT = mode 3
        io[1] = 0;

        // Write a red pixel at (0,0)
        let red: u16 = 0x001F; // R=31 in RGB555
        vram[0] = red as u8;
        vram[1] = (red >> 8) as u8;

        ppu.render_scanline(&io, &palette, &vram, &oam);

        assert_eq!(ppu.framebuffer[0], red);
    }

    #[test]
    fn test_mode4_render() {
        let mut ppu = Ppu::new();
        ppu.vcount = 0;

        let mut io = vec![0u8; 0x400];
        let mut palette = vec![0u8; 0x400];
        let mut vram = vec![0u8; 0x18000];
        let oam = vec![0u8; 0x400];

        // Set mode 4
        io[0] = 4;
        io[1] = 0;

        // Set palette entry 1 to green
        let green: u16 = 0x03E0;
        palette[2] = green as u8;
        palette[3] = (green >> 8) as u8;

        // Set pixel (0,0) to palette index 1
        vram[0] = 1;

        ppu.render_scanline(&io, &palette, &vram, &oam);

        assert_eq!(ppu.framebuffer[0], green);
    }
}
