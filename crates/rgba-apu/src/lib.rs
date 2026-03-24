use serde::{Deserialize, Serialize};

/// Sample rate for audio output
pub const SAMPLE_RATE: u32 = 32_768;

/// APU (Audio Processing Unit) state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Apu {
    /// PSG Channel 1: Pulse with sweep
    pub ch1: PulseChannel,
    /// PSG Channel 2: Pulse
    pub ch2: PulseChannel,
    /// PSG Channel 3: Wave
    pub ch3: WaveChannel,
    /// PSG Channel 4: Noise
    pub ch4: NoiseChannel,
    /// Direct Sound channel A
    pub fifo_a: FifoChannel,
    /// Direct Sound channel B
    pub fifo_b: FifoChannel,
    /// Output sample buffer
    pub sample_buffer: Vec<(f32, f32)>,
    /// Master enable
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PulseChannel {
    pub enabled: bool,
    pub length_counter: u16,
    pub duty: u8,
    pub volume: u8,
    pub frequency: u16,
    pub timer: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaveChannel {
    pub enabled: bool,
    pub length_counter: u16,
    pub volume_shift: u8,
    pub frequency: u16,
    pub wave_ram: [u8; 16],
    pub position: u8,
}

impl Default for WaveChannel {
    fn default() -> Self {
        Self {
            enabled: false,
            length_counter: 0,
            volume_shift: 0,
            frequency: 0,
            wave_ram: [0; 16],
            position: 0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NoiseChannel {
    pub enabled: bool,
    pub length_counter: u16,
    pub volume: u8,
    pub divisor: u8,
    pub width: bool,
    pub shift: u8,
    pub lfsr: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FifoChannel {
    pub buffer: [i8; 32],
    pub read_pos: usize,
    pub write_pos: usize,
    pub count: usize,
    pub current_sample: i8,
    pub enabled: bool,
    pub timer_select: u8,
    pub volume_full: bool,
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
        }
    }
}

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
        }
    }

    /// Generate one audio sample (called at SAMPLE_RATE frequency)
    pub fn generate_sample(&mut self) -> (f32, f32) {
        if !self.enabled {
            return (0.0, 0.0);
        }

        // TODO: Mix PSG + Direct Sound channels (US-13, US-14)
        (0.0, 0.0)
    }

    /// Push a sample to FIFO A
    pub fn push_fifo_a(&mut self, sample: i8) {
        if self.fifo_a.count < 32 {
            self.fifo_a.buffer[self.fifo_a.write_pos] = sample;
            self.fifo_a.write_pos = (self.fifo_a.write_pos + 1) % 32;
            self.fifo_a.count += 1;
        }
    }

    /// Push a sample to FIFO B
    pub fn push_fifo_b(&mut self, sample: i8) {
        if self.fifo_b.count < 32 {
            self.fifo_b.buffer[self.fifo_b.write_pos] = sample;
            self.fifo_b.write_pos = (self.fifo_b.write_pos + 1) % 32;
            self.fifo_b.count += 1;
        }
    }
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}
