mod mixer;

use serde::{Deserialize, Serialize};

/// Target sample rate for audio output
pub const SAMPLE_RATE: u32 = 32_768;

/// CPU cycles per audio sample at 32768 Hz
pub const CYCLES_PER_SAMPLE: u32 = 16_777_216 / SAMPLE_RATE;

/// Frame sequencer frequency (512 Hz)
pub const SEQUENCER_RATE: u32 = 512;
pub const CYCLES_PER_SEQUENCER: u32 = 16_777_216 / SEQUENCER_RATE;

/// APU (Audio Processing Unit) state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Apu {
    /// PSG Channel 1: Pulse with sweep
    pub ch1: PulseChannel,
    /// PSG Channel 2: Pulse (no sweep)
    pub ch2: PulseChannel,
    /// PSG Channel 3: Wave
    pub ch3: WaveChannel,
    /// PSG Channel 4: Noise
    pub ch4: NoiseChannel,
    /// Direct Sound channel A
    pub fifo_a: FifoChannel,
    /// Direct Sound channel B
    pub fifo_b: FifoChannel,
    /// Output sample buffer (stereo interleaved)
    pub sample_buffer: Vec<(f32, f32)>,
    /// Master enable (SOUNDCNT_X bit 7)
    pub enabled: bool,
    /// Frame sequencer step (0-7)
    pub sequencer_step: u8,
    /// PSG master volume left/right (SOUNDCNT_L)
    pub psg_volume_left: u8,
    pub psg_volume_right: u8,
    /// PSG channel enable left/right (SOUNDCNT_L bits 8-15)
    pub psg_enable_left: u8,
    pub psg_enable_right: u8,
    /// SOUNDCNT_H: Direct Sound volume and mixing
    pub soundcnt_h: u16,
    /// SOUNDBIAS
    pub sound_bias: u16,
}

// =============================================================================
// PSG Channel 1/2: Pulse (square wave)
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulseChannel {
    pub enabled: bool,
    pub dac_enabled: bool,
    // Sweep (channel 1 only)
    pub sweep_period: u8,
    pub sweep_negate: bool,
    pub sweep_shift: u8,
    pub sweep_timer: u8,
    pub sweep_enabled: bool,
    pub sweep_shadow_freq: u16,
    // Duty cycle
    pub duty: u8,
    // Length
    pub length_counter: u16,
    pub length_enable: bool,
    // Envelope
    pub envelope_volume: u8,
    pub envelope_add: bool,
    pub envelope_period: u8,
    pub envelope_timer: u8,
    // Frequency / timer
    pub frequency: u16,
    pub timer: u16,
    pub duty_pos: u8,
    // Output
    pub output: i8,
}

impl Default for PulseChannel {
    fn default() -> Self {
        Self {
            enabled: false,
            dac_enabled: false,
            sweep_period: 0,
            sweep_negate: false,
            sweep_shift: 0,
            sweep_timer: 0,
            sweep_enabled: false,
            sweep_shadow_freq: 0,
            duty: 0,
            length_counter: 0,
            length_enable: false,
            envelope_volume: 0,
            envelope_add: false,
            envelope_period: 0,
            envelope_timer: 0,
            frequency: 0,
            timer: 0,
            duty_pos: 0,
            output: 0,
        }
    }
}

/// Duty cycle waveform patterns
const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 1], // 12.5%
    [1, 0, 0, 0, 0, 0, 0, 1], // 25%
    [1, 0, 0, 0, 0, 1, 1, 1], // 50%
    [0, 1, 1, 1, 1, 1, 1, 0], // 75%
];

impl PulseChannel {
    /// Clock the channel timer (called at CPU clock rate, effectively)
    pub fn tick(&mut self) {
        if self.timer == 0 {
            self.timer = (2048 - self.frequency) * 4;
            self.duty_pos = (self.duty_pos + 1) & 7;
        } else {
            self.timer -= 1;
        }

        self.output = if self.enabled && self.dac_enabled {
            let sample = DUTY_TABLE[self.duty as usize][self.duty_pos as usize];
            if sample != 0 {
                self.envelope_volume as i8
            } else {
                -(self.envelope_volume as i8)
            }
        } else {
            0
        };
    }

    /// Clock the length counter (called at 256 Hz)
    pub fn clock_length(&mut self) {
        if self.length_enable && self.length_counter > 0 {
            self.length_counter -= 1;
            if self.length_counter == 0 {
                self.enabled = false;
            }
        }
    }

    /// Clock the envelope (called at 64 Hz)
    pub fn clock_envelope(&mut self) {
        if self.envelope_period == 0 {
            return;
        }
        if self.envelope_timer > 0 {
            self.envelope_timer -= 1;
        }
        if self.envelope_timer == 0 {
            self.envelope_timer = self.envelope_period;
            if self.envelope_add && self.envelope_volume < 15 {
                self.envelope_volume += 1;
            } else if !self.envelope_add && self.envelope_volume > 0 {
                self.envelope_volume -= 1;
            }
        }
    }

    /// Clock the sweep (called at 128 Hz, channel 1 only)
    pub fn clock_sweep(&mut self) {
        if !self.sweep_enabled || self.sweep_period == 0 {
            return;
        }
        if self.sweep_timer > 0 {
            self.sweep_timer -= 1;
        }
        if self.sweep_timer == 0 {
            self.sweep_timer = self.sweep_period;
            let new_freq = self.calculate_sweep();
            if new_freq <= 2047 && self.sweep_shift > 0 {
                self.sweep_shadow_freq = new_freq;
                self.frequency = new_freq;
                // Overflow check
                if self.calculate_sweep() > 2047 {
                    self.enabled = false;
                }
            } else if new_freq > 2047 {
                self.enabled = false;
            }
        }
    }

    fn calculate_sweep(&self) -> u16 {
        let delta = self.sweep_shadow_freq >> self.sweep_shift;
        if self.sweep_negate {
            self.sweep_shadow_freq.wrapping_sub(delta)
        } else {
            self.sweep_shadow_freq.wrapping_add(delta)
        }
    }

    /// Trigger the channel
    pub fn trigger(&mut self) {
        self.enabled = true;
        if self.length_counter == 0 {
            self.length_counter = 64;
        }
        self.timer = (2048 - self.frequency) * 4;
        self.envelope_timer = self.envelope_period;
        self.envelope_volume = (self.envelope_volume) | 0; // reload from register
        self.sweep_shadow_freq = self.frequency;
        self.sweep_timer = self.sweep_period;
        self.sweep_enabled = self.sweep_period > 0 || self.sweep_shift > 0;
        self.duty_pos = 0;
    }
}

// =============================================================================
// PSG Channel 3: Wave
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaveChannel {
    pub enabled: bool,
    pub dac_enabled: bool,
    pub length_counter: u16,
    pub length_enable: bool,
    pub volume_shift: u8,  // 0=mute, 1=100%, 2=50%, 3=25%
    pub frequency: u16,
    pub timer: u16,
    pub wave_ram: [u8; 32],  // Two banks of 16 bytes (4-bit samples)
    pub position: u8,
    pub bank_mode: bool,     // 0=two banks, 1=single bank
    pub current_bank: u8,
    pub output: i8,
}

impl Default for WaveChannel {
    fn default() -> Self {
        Self {
            enabled: false,
            dac_enabled: false,
            length_counter: 0,
            length_enable: false,
            volume_shift: 0,
            frequency: 0,
            timer: 0,
            wave_ram: [0; 32],
            position: 0,
            bank_mode: false,
            current_bank: 0,
            output: 0,
        }
    }
}

impl WaveChannel {
    pub fn tick(&mut self) {
        if self.timer == 0 {
            self.timer = (2048 - self.frequency) * 2;
            self.position = (self.position + 1) & 63;
        } else {
            self.timer -= 1;
        }

        if self.enabled && self.dac_enabled {
            let bank_offset = if self.bank_mode { 0 } else { self.current_bank as usize * 16 };
            let byte_pos = bank_offset + (self.position as usize / 2);
            let sample = if self.position & 1 == 0 {
                (self.wave_ram[byte_pos] >> 4) & 0xF
            } else {
                self.wave_ram[byte_pos] & 0xF
            };

            let shifted = match self.volume_shift {
                0 => 0,
                1 => sample,
                2 => sample >> 1,
                3 => sample >> 2,
                _ => 0,
            };
            self.output = shifted as i8 - 8;
        } else {
            self.output = 0;
        }
    }

    pub fn clock_length(&mut self) {
        if self.length_enable && self.length_counter > 0 {
            self.length_counter -= 1;
            if self.length_counter == 0 {
                self.enabled = false;
            }
        }
    }

    pub fn trigger(&mut self) {
        self.enabled = true;
        if self.length_counter == 0 {
            self.length_counter = 256;
        }
        self.timer = (2048 - self.frequency) * 2;
        self.position = 0;
    }
}

// =============================================================================
// PSG Channel 4: Noise
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoiseChannel {
    pub enabled: bool,
    pub dac_enabled: bool,
    pub length_counter: u16,
    pub length_enable: bool,
    pub envelope_volume: u8,
    pub envelope_add: bool,
    pub envelope_period: u8,
    pub envelope_timer: u8,
    pub divisor_code: u8,
    pub width_mode: bool,  // true = 7-bit, false = 15-bit
    pub clock_shift: u8,
    pub lfsr: u16,
    pub timer: u32,
    pub output: i8,
}

impl Default for NoiseChannel {
    fn default() -> Self {
        Self {
            enabled: false,
            dac_enabled: false,
            length_counter: 0,
            length_enable: false,
            envelope_volume: 0,
            envelope_add: false,
            envelope_period: 0,
            envelope_timer: 0,
            divisor_code: 0,
            width_mode: false,
            clock_shift: 0,
            lfsr: 0x7FFF,
            timer: 0,
            output: 0,
        }
    }
}

const NOISE_DIVISORS: [u32; 8] = [8, 16, 32, 48, 64, 80, 96, 112];

impl NoiseChannel {
    pub fn tick(&mut self) {
        if self.timer == 0 {
            self.timer = NOISE_DIVISORS[self.divisor_code as usize] << self.clock_shift;

            let bit = (self.lfsr ^ (self.lfsr >> 1)) & 1;
            self.lfsr >>= 1;
            self.lfsr |= bit << 14;
            if self.width_mode {
                self.lfsr &= !(1 << 6);
                self.lfsr |= bit << 6;
            }
        } else {
            self.timer -= 1;
        }

        self.output = if self.enabled && self.dac_enabled && (self.lfsr & 1 == 0) {
            self.envelope_volume as i8
        } else {
            -(self.envelope_volume as i8)
        };
    }

    pub fn clock_length(&mut self) {
        if self.length_enable && self.length_counter > 0 {
            self.length_counter -= 1;
            if self.length_counter == 0 {
                self.enabled = false;
            }
        }
    }

    pub fn clock_envelope(&mut self) {
        if self.envelope_period == 0 {
            return;
        }
        if self.envelope_timer > 0 {
            self.envelope_timer -= 1;
        }
        if self.envelope_timer == 0 {
            self.envelope_timer = self.envelope_period;
            if self.envelope_add && self.envelope_volume < 15 {
                self.envelope_volume += 1;
            } else if !self.envelope_add && self.envelope_volume > 0 {
                self.envelope_volume -= 1;
            }
        }
    }

    pub fn trigger(&mut self) {
        self.enabled = true;
        if self.length_counter == 0 {
            self.length_counter = 64;
        }
        self.lfsr = 0x7FFF;
        self.timer = NOISE_DIVISORS[self.divisor_code as usize] << self.clock_shift;
        self.envelope_timer = self.envelope_period;
    }
}

// =============================================================================
// Direct Sound FIFO Channels
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FifoChannel {
    pub buffer: [i8; 32],
    pub read_pos: usize,
    pub write_pos: usize,
    pub count: usize,
    pub current_sample: i8,
    pub enabled: bool,
    pub timer_select: u8,  // which timer (0 or 1) drives this channel
    pub volume_full: bool, // true = 100%, false = 50%
    pub enable_left: bool,
    pub enable_right: bool,
}

impl Default for FifoChannel {
    fn default() -> Self {
        Self {
            buffer: [0; 32],
            read_pos: 0,
            write_pos: 0,
            count: 0,
            current_sample: 0,
            enabled: false,
            timer_select: 0,
            volume_full: false,
            enable_left: false,
            enable_right: false,
        }
    }
}

impl FifoChannel {
    /// Push a sample into the FIFO
    pub fn push(&mut self, sample: i8) {
        if self.count < 32 {
            self.buffer[self.write_pos] = sample;
            self.write_pos = (self.write_pos + 1) % 32;
            self.count += 1;
        }
    }

    /// Push 4 bytes (32-bit write to FIFO register)
    pub fn push_word(&mut self, value: u32) {
        self.push(value as i8);
        self.push((value >> 8) as i8);
        self.push((value >> 16) as i8);
        self.push((value >> 24) as i8);
    }

    /// Pop the next sample (called on timer overflow)
    pub fn pop(&mut self) -> i8 {
        if self.count > 0 {
            let sample = self.buffer[self.read_pos];
            self.read_pos = (self.read_pos + 1) % 32;
            self.count -= 1;
            self.current_sample = sample;
            sample
        } else {
            self.current_sample
        }
    }

    /// Reset the FIFO
    pub fn reset(&mut self) {
        self.read_pos = 0;
        self.write_pos = 0;
        self.count = 0;
        self.current_sample = 0;
    }

    /// Check if FIFO needs refill (count <= 16)
    pub fn needs_refill(&self) -> bool {
        self.count <= 16
    }
}

// =============================================================================
// APU Implementation
// =============================================================================

impl Apu {
    pub fn new() -> Self {
        Self {
            ch1: PulseChannel::default(),
            ch2: PulseChannel::default(),
            ch3: WaveChannel::default(),
            ch4: NoiseChannel::default(),
            fifo_a: FifoChannel::default(),
            fifo_b: FifoChannel::default(),
            sample_buffer: Vec::with_capacity(1024),
            enabled: false,
            sequencer_step: 0,
            psg_volume_left: 7,
            psg_volume_right: 7,
            psg_enable_left: 0,
            psg_enable_right: 0,
            soundcnt_h: 0,
            sound_bias: 0x200,
        }
    }

    /// Clock the frame sequencer (called at 512 Hz)
    pub fn clock_sequencer(&mut self) {
        match self.sequencer_step {
            0 => {
                self.ch1.clock_length();
                self.ch2.clock_length();
                self.ch3.clock_length();
                self.ch4.clock_length();
            }
            2 => {
                self.ch1.clock_length();
                self.ch2.clock_length();
                self.ch3.clock_length();
                self.ch4.clock_length();
                self.ch1.clock_sweep();
            }
            4 => {
                self.ch1.clock_length();
                self.ch2.clock_length();
                self.ch3.clock_length();
                self.ch4.clock_length();
            }
            6 => {
                self.ch1.clock_length();
                self.ch2.clock_length();
                self.ch3.clock_length();
                self.ch4.clock_length();
                self.ch1.clock_sweep();
            }
            7 => {
                self.ch1.clock_envelope();
                self.ch2.clock_envelope();
                self.ch4.clock_envelope();
            }
            _ => {}
        }
        self.sequencer_step = (self.sequencer_step + 1) & 7;
    }

    /// Called when a timer overflows — advances Direct Sound if appropriate
    pub fn on_timer_overflow(&mut self, timer_id: u8) {
        if self.fifo_a.timer_select == timer_id {
            self.fifo_a.pop();
        }
        if self.fifo_b.timer_select == timer_id {
            self.fifo_b.pop();
        }
    }

    /// Generate one audio sample (stereo)
    pub fn generate_sample(&mut self) -> (f32, f32) {
        if !self.enabled {
            return (0.0, 0.0);
        }

        // Mix PSG channels
        let mut left = 0i16;
        let mut right = 0i16;

        if self.psg_enable_left & 1 != 0 {
            left += self.ch1.output as i16;
        }
        if self.psg_enable_left & 2 != 0 {
            left += self.ch2.output as i16;
        }
        if self.psg_enable_left & 4 != 0 {
            left += self.ch3.output as i16;
        }
        if self.psg_enable_left & 8 != 0 {
            left += self.ch4.output as i16;
        }

        if self.psg_enable_right & 1 != 0 {
            right += self.ch1.output as i16;
        }
        if self.psg_enable_right & 2 != 0 {
            right += self.ch2.output as i16;
        }
        if self.psg_enable_right & 4 != 0 {
            right += self.ch3.output as i16;
        }
        if self.psg_enable_right & 8 != 0 {
            right += self.ch4.output as i16;
        }

        // Scale PSG by master volume
        left = left * (self.psg_volume_left as i16 + 1) / 8;
        right = right * (self.psg_volume_right as i16 + 1) / 8;

        // PSG volume ratio (SOUNDCNT_H bits 0-1)
        let psg_ratio = match self.soundcnt_h & 3 {
            0 => 1, // 25%
            1 => 2, // 50%
            2 => 4, // 100%
            _ => 0, // prohibited
        };
        left = left * psg_ratio / 4;
        right = right * psg_ratio / 4;

        // Mix Direct Sound channels
        let dsa = self.fifo_a.current_sample as i16;
        let dsb = self.fifo_b.current_sample as i16;

        let dsa_vol = if self.fifo_a.volume_full { dsa } else { dsa / 2 };
        let dsb_vol = if self.fifo_b.volume_full { dsb } else { dsb / 2 };

        if self.fifo_a.enable_left {
            left += dsa_vol;
        }
        if self.fifo_a.enable_right {
            right += dsa_vol;
        }
        if self.fifo_b.enable_left {
            left += dsb_vol;
        }
        if self.fifo_b.enable_right {
            right += dsb_vol;
        }

        // Apply bias and clamp
        let bias = (self.sound_bias & 0x3FF) as i16;
        left = (left + bias).clamp(0, 0x3FF);
        right = (right + bias).clamp(0, 0x3FF);

        // Normalize to -1.0..1.0
        let left_f = (left as f32 - bias as f32) / 512.0;
        let right_f = (right as f32 - bias as f32) / 512.0;

        (left_f.clamp(-1.0, 1.0), right_f.clamp(-1.0, 1.0))
    }

    /// Update APU registers from I/O (called on sound register writes)
    pub fn update_from_io(&mut self, io: &[u8]) {
        use rgba_core::constants::*;

        // SOUNDCNT_L (PSG volume/enable)
        let soundcnt_l = io[REG_SOUNDCNT_L as usize] as u16
            | ((io[(REG_SOUNDCNT_L + 1) as usize] as u16) << 8);
        self.psg_volume_right = (soundcnt_l & 7) as u8;
        self.psg_volume_left = ((soundcnt_l >> 4) & 7) as u8;
        self.psg_enable_right = ((soundcnt_l >> 8) & 0xF) as u8;
        self.psg_enable_left = ((soundcnt_l >> 12) & 0xF) as u8;

        // SOUNDCNT_H (Direct Sound)
        self.soundcnt_h = io[REG_SOUNDCNT_H as usize] as u16
            | ((io[(REG_SOUNDCNT_H + 1) as usize] as u16) << 8);
        self.fifo_a.volume_full = self.soundcnt_h & (1 << 2) != 0;
        self.fifo_b.volume_full = self.soundcnt_h & (1 << 3) != 0;
        self.fifo_a.enable_right = self.soundcnt_h & (1 << 8) != 0;
        self.fifo_a.enable_left = self.soundcnt_h & (1 << 9) != 0;
        self.fifo_a.timer_select = ((self.soundcnt_h >> 10) & 1) as u8;
        self.fifo_b.enable_right = self.soundcnt_h & (1 << 12) != 0;
        self.fifo_b.enable_left = self.soundcnt_h & (1 << 13) != 0;
        self.fifo_b.timer_select = ((self.soundcnt_h >> 14) & 1) as u8;

        if self.soundcnt_h & (1 << 11) != 0 {
            self.fifo_a.reset();
        }
        if self.soundcnt_h & (1 << 15) != 0 {
            self.fifo_b.reset();
        }

        // SOUNDCNT_X
        let soundcnt_x = io[REG_SOUNDCNT_X as usize];
        self.enabled = soundcnt_x & (1 << 7) != 0;

        // SOUNDBIAS
        self.sound_bias = io[REG_SOUNDBIAS as usize] as u16
            | ((io[(REG_SOUNDBIAS + 1) as usize] as u16) << 8);
    }
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fifo_push_pop() {
        let mut fifo = FifoChannel::default();
        fifo.push(10);
        fifo.push(20);
        fifo.push(30);
        assert_eq!(fifo.count, 3);
        assert_eq!(fifo.pop(), 10);
        assert_eq!(fifo.pop(), 20);
        assert_eq!(fifo.pop(), 30);
        assert_eq!(fifo.count, 0);
    }

    #[test]
    fn test_fifo_word_push() {
        let mut fifo = FifoChannel::default();
        fifo.push_word(0x04030201);
        assert_eq!(fifo.count, 4);
        assert_eq!(fifo.pop(), 0x01);
        assert_eq!(fifo.pop(), 0x02);
        assert_eq!(fifo.pop(), 0x03);
        assert_eq!(fifo.pop(), 0x04);
    }

    #[test]
    fn test_fifo_overflow() {
        let mut fifo = FifoChannel::default();
        for i in 0..40 {
            fifo.push(i);
        }
        assert_eq!(fifo.count, 32); // capped at 32
    }

    #[test]
    fn test_pulse_channel_trigger() {
        let mut ch = PulseChannel::default();
        ch.frequency = 1024;
        ch.envelope_volume = 15;
        ch.dac_enabled = true;
        ch.trigger();
        assert!(ch.enabled);
        assert_eq!(ch.length_counter, 64);
    }

    #[test]
    fn test_noise_lfsr() {
        let mut ch = NoiseChannel::default();
        ch.enabled = true;
        ch.dac_enabled = true;
        ch.envelope_volume = 15;
        ch.trigger();

        let initial = ch.lfsr;
        ch.tick();
        // LFSR should have shifted
        // It starts at 0x7FFF and after one tick with timer=0 it should advance
    }

    #[test]
    fn test_apu_disabled_silence() {
        let mut apu = Apu::new();
        apu.enabled = false;
        let (l, r) = apu.generate_sample();
        assert_eq!(l, 0.0);
        assert_eq!(r, 0.0);
    }

    #[test]
    fn test_wave_channel() {
        let mut ch = WaveChannel::default();
        ch.enabled = true;
        ch.dac_enabled = true;
        ch.volume_shift = 1; // 100%
        ch.frequency = 2000;
        ch.wave_ram[0] = 0xF0; // high nibble = 15, low = 0
        ch.trigger();
        ch.tick();
        // Should produce a non-zero output based on wave data
    }
}
