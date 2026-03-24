// Mixer module — audio resampling and output buffering
// Used by the frontend for final audio output

/// Simple linear resampler from emulator sample rate to output device rate
pub struct AudioResampler {
    source_rate: f64,
    target_rate: f64,
    accumulator: f64,
    last_left: f32,
    last_right: f32,
}

impl AudioResampler {
    pub fn new(source_rate: u32, target_rate: u32) -> Self {
        Self {
            source_rate: source_rate as f64,
            target_rate: target_rate as f64,
            accumulator: 0.0,
            last_left: 0.0,
            last_right: 0.0,
        }
    }

    /// Feed a source sample, returns resampled output samples
    pub fn push(&mut self, left: f32, right: f32) -> Vec<(f32, f32)> {
        let mut output = Vec::new();
        self.last_left = left;
        self.last_right = right;

        self.accumulator += self.target_rate;
        while self.accumulator >= self.source_rate {
            self.accumulator -= self.source_rate;
            output.push((self.last_left, self.last_right));
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resampler_downsample() {
        // 32768 -> 44100 (upsample)
        let mut resampler = AudioResampler::new(32768, 44100);
        let mut total_output = 0;
        for _ in 0..32768 {
            total_output += resampler.push(0.5, -0.5).len();
        }
        // Should produce approximately 44100 samples
        assert!((total_output as i32 - 44100).abs() < 10);
    }
}
