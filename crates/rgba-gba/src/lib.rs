pub mod cartridge;

use serde::{Deserialize, Serialize};

use rgba_core::bus::{Bus, BusAccess};
use rgba_core::constants::*;
use rgba_core::dma::{DmaController, DmaTiming};
use rgba_core::scheduler::{EventKind, Scheduler};
use rgba_core::timer::TimerController;
use rgba_core::types::*;

use rgba_arm::Arm7Tdmi;
use rgba_apu::Apu;
use rgba_ppu::{Ppu, PpuEvent};

use crate::cartridge::Cartridge;

/// The complete GBA emulator state
#[derive(Clone, Serialize, Deserialize)]
pub struct Gba {
    pub cpu: Arm7Tdmi,
    pub bus: Bus,
    pub ppu: Ppu,
    pub apu: Apu,
    pub dma: DmaController,
    pub timers: TimerController,
    pub scheduler: Scheduler,
    pub cartridge: Option<Cartridge>,

    /// Keypad state (active-low: 0 = pressed)
    pub keyinput: u16,

    /// Speed mode
    pub speed_mode: SpeedMode,

    /// Memory watches for RAM inspection
    pub watches: Vec<MemoryWatch>,

    /// Frame counter
    pub frame_count: u64,

    /// Audio sample accumulator
    audio_cycle_accum: u32,
    /// Sequencer cycle accumulator
    sequencer_cycle_accum: u32,
}

impl Gba {
    pub fn new() -> Self {
        Self {
            cpu: Arm7Tdmi::new(),
            bus: Bus::new(),
            ppu: Ppu::new(),
            apu: Apu::new(),
            dma: DmaController::new(),
            timers: TimerController::new(),
            scheduler: Scheduler::new(),
            cartridge: None,
            keyinput: 0x03FF,
            speed_mode: SpeedMode::Normal,
            watches: Vec::new(),
            frame_count: 0,
            audio_cycle_accum: 0,
            sequencer_cycle_accum: 0,
        }
    }

    /// Load a ROM and initialize
    pub fn load_rom(&mut self, rom_data: &[u8]) {
        self.bus.load_rom(rom_data);
        self.cartridge = Some(Cartridge::from_rom(rom_data));

        // If we have a cartridge with SRAM, copy it to the bus
        if let Some(cart) = &self.cartridge {
            if !cart.sram.is_empty() {
                let len = cart.sram.len().min(self.bus.sram.len());
                self.bus.sram[..len].copy_from_slice(&cart.sram[..len]);
            }
        }
    }

    /// Load BIOS data
    pub fn load_bios(&mut self, bios_data: &[u8]) {
        self.bus.load_bios(bios_data);
    }

    /// Reset and boot (skip BIOS)
    pub fn reset(&mut self) {
        self.cpu.reset_skip_bios();
        self.ppu = Ppu::new();
        self.scheduler = Scheduler::new();
        self.dma = DmaController::new();
        self.timers = TimerController::new();
        self.frame_count = 0;

        // Schedule first scanline events
        self.scheduler.schedule(EventKind::HBlank, HDRAW_CYCLES as u64);
    }

    // =========================================================================
    // Main Emulation Loop
    // =========================================================================

    /// Run emulation for one complete frame (280,896 CPU cycles)
    pub fn run_frame(&mut self) {
        self.ppu.frame_ready = false;

        while !self.ppu.frame_ready {
            self.step();
        }

        self.frame_count += 1;
    }

    /// Step one CPU instruction + handle events
    pub fn step(&mut self) {
        // Update key input
        self.bus.keyinput = self.keyinput;

        // Check for IRQ
        if self.bus.has_pending_irq() && !self.cpu.regs.cpsr.irq_disabled() {
            self.cpu.trigger_irq();
        }

        // Execute one CPU instruction
        let cycles = self.cpu.step(&mut self.bus) as u32;

        // Advance scheduler
        self.scheduler.advance(cycles as u64);

        // Tick timers
        let timer_overflow = self.timers.tick(cycles, &mut self.bus);
        if timer_overflow != 0 {
            for i in 0..4 {
                if timer_overflow & (1 << i) != 0 {
                    self.apu.on_timer_overflow(i);
                }
            }
        }

        // Audio sample generation
        self.audio_cycle_accum += cycles;
        while self.audio_cycle_accum >= rgba_apu::CYCLES_PER_SAMPLE {
            self.audio_cycle_accum -= rgba_apu::CYCLES_PER_SAMPLE;
            let sample = self.apu.generate_sample();
            self.apu.sample_buffer.push(sample);
        }

        // Frame sequencer
        self.sequencer_cycle_accum += cycles;
        while self.sequencer_cycle_accum >= rgba_apu::CYCLES_PER_SEQUENCER {
            self.sequencer_cycle_accum -= rgba_apu::CYCLES_PER_SEQUENCER;
            self.apu.clock_sequencer();
        }

        // Process scheduled events
        while let Some(event) = self.scheduler.pop_pending() {
            match event.kind {
                EventKind::HBlank => {
                    // Render the current scanline
                    if self.ppu.vcount < VISIBLE_LINES as u16 {
                        self.ppu.render_scanline(
                            &self.bus.io,
                            &self.bus.palette,
                            &self.bus.vram,
                            &self.bus.oam,
                        );
                    }

                    // Update DISPSTAT
                    self.bus.update_display_status(self.ppu.vcount, true);

                    // HBlank IRQ
                    let dispstat = self.bus.io_read16(REG_DISPSTAT);
                    if dispstat & (1 << 4) != 0 {
                        self.bus.request_interrupt(IRQ_HBLANK);
                    }

                    // HBlank DMA
                    self.dma.check_timing(DmaTiming::HBlank, &mut self.bus);

                    // Schedule end of HBlank
                    self.scheduler.schedule(
                        EventKind::HBlankEnd,
                        event.timestamp + HBLANK_CYCLES as u64,
                    );
                }
                EventKind::HBlankEnd => {
                    // Advance to next scanline
                    let ppu_event = self.ppu.advance_scanline();
                    self.bus.update_display_status(self.ppu.vcount, false);

                    match ppu_event {
                        PpuEvent::VBlankStart => {
                            // VBlank IRQ
                            let dispstat = self.bus.io_read16(REG_DISPSTAT);
                            if dispstat & (1 << 3) != 0 {
                                self.bus.request_interrupt(IRQ_VBLANK);
                            }
                            // VBlank DMA
                            self.dma.check_timing(DmaTiming::VBlank, &mut self.bus);
                        }
                        PpuEvent::FrameStart => {
                            // Latch BG reference points
                            self.ppu.latch_ref_points(&self.bus.io);
                        }
                        _ => {}
                    }

                    // VCount match IRQ
                    let dispstat = self.bus.io_read16(REG_DISPSTAT);
                    if dispstat & 4 != 0 && dispstat & (1 << 5) != 0 {
                        self.bus.request_interrupt(IRQ_VCOUNT);
                    }

                    // Schedule next HBlank
                    self.scheduler.schedule(
                        EventKind::HBlank,
                        event.timestamp + HDRAW_CYCLES as u64,
                    );
                }
                _ => {}
            }
        }
    }

    // =========================================================================
    // Input (US-16)
    // =========================================================================

    /// Press a button
    pub fn press_button(&mut self, button: GbaButton) {
        self.keyinput &= !button.bit();

        // Check keypad interrupt
        self.check_keypad_irq();
    }

    /// Release a button
    pub fn release_button(&mut self, button: GbaButton) {
        self.keyinput |= button.bit();
    }

    /// Set all buttons at once (bitmask, active-low)
    pub fn set_keyinput(&mut self, value: u16) {
        self.keyinput = value & 0x03FF;
    }

    fn check_keypad_irq(&mut self) {
        let keycnt = self.bus.io_read16(REG_KEYCNT);
        let irq_enable = keycnt & (1 << 14) != 0;
        if !irq_enable {
            return;
        }

        let mask = keycnt & 0x03FF;
        let pressed = !self.keyinput & 0x03FF;
        let irq_condition = keycnt & (1 << 15) != 0; // AND vs OR

        let trigger = if irq_condition {
            (pressed & mask) == mask // all specified keys pressed
        } else {
            (pressed & mask) != 0 // any specified key pressed
        };

        if trigger {
            self.bus.request_interrupt(IRQ_KEYPAD);
            self.cpu.halted = false;
        }
    }

    // =========================================================================
    // Snapshots / Savestates (US-18)
    // =========================================================================

    /// Create a full savestate
    pub fn save_state(&self) -> Vec<u8> {
        // Use a simple format: length-prefixed JSON (for now)
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Load a savestate
    pub fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        let state: Gba = serde_json::from_slice(data)
            .map_err(|e| format!("Failed to deserialize savestate: {}", e))?;
        *self = state;
        Ok(())
    }

    // =========================================================================
    // Speed Control (US-19)
    // =========================================================================

    /// Set emulation speed mode
    pub fn set_speed(&mut self, mode: SpeedMode) {
        self.speed_mode = mode;
    }

    /// Get the target frame time in seconds based on speed mode
    pub fn target_frame_time(&self) -> Option<f64> {
        match self.speed_mode {
            SpeedMode::Normal => Some(1.0 / FRAME_RATE),
            SpeedMode::FastForward(mult) => Some(1.0 / (FRAME_RATE * mult)),
            SpeedMode::Unlimited => None, // no frame limiter
            SpeedMode::Paused => None,    // don't run frames
        }
    }

    // =========================================================================
    // Memory Inspection & Manipulation (US-20)
    // =========================================================================

    /// Read a byte from any address (no side effects)
    pub fn peek(&self, address: u32) -> u8 {
        self.bus.peek_byte(address)
    }

    /// Read a 16-bit value
    pub fn peek16(&self, address: u32) -> u16 {
        self.bus.read_halfword(address)
    }

    /// Read a 32-bit value
    pub fn peek32(&self, address: u32) -> u32 {
        self.bus.read_word(address)
    }

    /// Write a byte to any writable address (bypasses I/O side effects)
    pub fn poke(&mut self, address: u32, value: u8) {
        self.bus.poke_byte(address, value);
    }

    /// Write a 16-bit value
    pub fn poke16(&mut self, address: u32, value: u16) {
        self.bus.poke_byte(address, value as u8);
        self.bus.poke_byte(address + 1, (value >> 8) as u8);
    }

    /// Write a 32-bit value
    pub fn poke32(&mut self, address: u32, value: u32) {
        self.poke16(address, value as u16);
        self.poke16(address + 2, (value >> 16) as u16);
    }

    /// Search memory region for a value
    pub fn search_memory(&self, start: u32, end: u32, value: u8) -> Vec<u32> {
        let mut results = Vec::new();
        let mut addr = start;
        while addr <= end {
            if self.peek(addr) == value {
                results.push(addr);
            }
            addr = addr.wrapping_add(1);
            if addr == 0 {
                break;
            }
        }
        results
    }

    /// Search memory for a 16-bit value
    pub fn search_memory16(&self, start: u32, end: u32, value: u16) -> Vec<u32> {
        let mut results = Vec::new();
        let mut addr = start;
        while addr + 1 <= end {
            if self.peek16(addr) == value {
                results.push(addr);
            }
            addr = addr.wrapping_add(2);
            if addr == 0 {
                break;
            }
        }
        results
    }

    /// Search memory for a 32-bit value
    pub fn search_memory32(&self, start: u32, end: u32, value: u32) -> Vec<u32> {
        let mut results = Vec::new();
        let mut addr = start;
        while addr + 3 <= end {
            if self.peek32(addr) == value {
                results.push(addr);
            }
            addr = addr.wrapping_add(4);
            if addr == 0 {
                break;
            }
        }
        results
    }

    /// Add a memory watch
    pub fn add_watch(&mut self, address: u32, label: String, width: AccessWidth) -> usize {
        let value = match width {
            AccessWidth::Byte => self.peek(address) as u32,
            AccessWidth::Halfword => self.peek16(address) as u32,
            AccessWidth::Word => self.peek32(address),
        };
        let watch = MemoryWatch {
            address,
            label,
            width,
            last_value: value,
        };
        self.watches.push(watch);
        self.watches.len() - 1
    }

    /// Update all watches and return changed ones
    pub fn update_watches(&mut self) -> Vec<(usize, u32, u32)> {
        let mut changes = Vec::new();
        for (i, watch) in self.watches.iter_mut().enumerate() {
            let new_value = match watch.width {
                AccessWidth::Byte => self.bus.peek_byte(watch.address) as u32,
                AccessWidth::Halfword => self.bus.read_halfword(watch.address) as u32,
                AccessWidth::Word => self.bus.read_word(watch.address),
            };
            if new_value != watch.last_value {
                changes.push((i, watch.last_value, new_value));
                watch.last_value = new_value;
            }
        }
        changes
    }

    /// Remove a memory watch
    pub fn remove_watch(&mut self, index: usize) {
        if index < self.watches.len() {
            self.watches.remove(index);
        }
    }

    /// Dump a memory region as bytes
    pub fn dump_memory(&self, start: u32, length: u32) -> Vec<u8> {
        let mut data = Vec::with_capacity(length as usize);
        for i in 0..length {
            data.push(self.peek(start.wrapping_add(i)));
        }
        data
    }

    /// Get save data from cartridge
    pub fn get_save_data(&self) -> Vec<u8> {
        self.cartridge.as_ref().map(|c| c.get_save_data()).unwrap_or_default()
    }

    /// Load save data to cartridge
    pub fn load_save_data(&mut self, data: &[u8]) {
        if let Some(cart) = &mut self.cartridge {
            cart.load_save_data(data);
        }
    }
}

impl Default for Gba {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gba_new() {
        let gba = Gba::new();
        assert_eq!(gba.keyinput, 0x03FF);
        assert_eq!(gba.frame_count, 0);
    }

    #[test]
    fn test_button_press_release() {
        let mut gba = Gba::new();
        gba.press_button(GbaButton::A);
        assert_eq!(gba.keyinput & GbaButton::A.bit(), 0); // pressed = bit clear
        gba.release_button(GbaButton::A);
        assert_ne!(gba.keyinput & GbaButton::A.bit(), 0); // released = bit set
    }

    #[test]
    fn test_peek_poke() {
        let mut gba = Gba::new();
        gba.poke(0x0200_0000, 0x42);
        assert_eq!(gba.peek(0x0200_0000), 0x42);
    }

    #[test]
    fn test_peek_poke_16() {
        let mut gba = Gba::new();
        gba.poke16(0x0200_0000, 0xABCD);
        assert_eq!(gba.peek16(0x0200_0000), 0xABCD);
    }

    #[test]
    fn test_memory_search() {
        let mut gba = Gba::new();
        gba.poke(0x0200_0010, 0x42);
        gba.poke(0x0200_0020, 0x42);
        let results = gba.search_memory(0x0200_0000, 0x0200_0030, 0x42);
        assert_eq!(results.len(), 2);
        assert!(results.contains(&0x0200_0010));
        assert!(results.contains(&0x0200_0020));
    }

    #[test]
    fn test_memory_watch() {
        let mut gba = Gba::new();
        gba.poke(0x0200_0000, 0x10);
        let idx = gba.add_watch(0x0200_0000, "test".into(), AccessWidth::Byte);

        // No change yet
        let changes = gba.update_watches();
        assert!(changes.is_empty());

        // Change value
        gba.poke(0x0200_0000, 0x20);
        let changes = gba.update_watches();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0], (idx, 0x10, 0x20));
    }

    #[test]
    fn test_savestate() {
        let mut gba = Gba::new();
        gba.poke(0x0200_0000, 0x42);

        let state = gba.save_state();
        assert!(!state.is_empty());

        gba.poke(0x0200_0000, 0x00);
        assert_eq!(gba.peek(0x0200_0000), 0x00);

        gba.load_state(&state).unwrap();
        assert_eq!(gba.peek(0x0200_0000), 0x42);
    }

    #[test]
    fn test_speed_mode() {
        let mut gba = Gba::new();
        assert!(gba.target_frame_time().is_some());

        gba.set_speed(SpeedMode::FastForward(2.0));
        let ft = gba.target_frame_time().unwrap();
        assert!(ft < 1.0 / 59.0);

        gba.set_speed(SpeedMode::Unlimited);
        assert!(gba.target_frame_time().is_none());
    }

    #[test]
    fn test_dump_memory() {
        let mut gba = Gba::new();
        gba.poke(0x0200_0000, 0xAA);
        gba.poke(0x0200_0001, 0xBB);
        let data = gba.dump_memory(0x0200_0000, 2);
        assert_eq!(data, vec![0xAA, 0xBB]);
    }
}
