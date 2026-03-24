use serde::{Deserialize, Serialize};

use crate::bus::Bus;
use crate::constants::*;

/// Prescaler divisors for timer clock
const PRESCALER: [u32; 4] = [1, 64, 256, 1024];

/// Timer channel state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimerChannel {
    /// Current counter value
    pub counter: u16,
    /// Reload value (written to TM_CNT_L)
    pub reload: u16,
    /// Prescaler selection (0-3)
    pub prescaler: u8,
    /// Count-up timing (cascade from previous timer)
    pub cascade: bool,
    /// IRQ on overflow
    pub irq_enable: bool,
    /// Timer enabled
    pub enabled: bool,
    /// Internal cycle accumulator
    pub cycles: u32,
}

impl Default for TimerChannel {
    fn default() -> Self {
        Self {
            counter: 0,
            reload: 0,
            prescaler: 0,
            cascade: false,
            irq_enable: false,
            enabled: false,
            cycles: 0,
        }
    }
}

/// Timer controller with 4 channels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimerController {
    pub timers: [TimerChannel; 4],
}

impl TimerController {
    pub fn new() -> Self {
        Self {
            timers: Default::default(),
        }
    }

    /// Synchronize timer state from I/O registers
    pub fn update_from_io(&mut self, bus: &Bus) {
        for i in 0..4 {
            let cnt_l_offset = REG_TM0CNT_L + (i as u32 * 4);
            let cnt_h_offset = REG_TM0CNT_H + (i as u32 * 4);

            let reload = bus.io[cnt_l_offset as usize] as u16
                | ((bus.io[(cnt_l_offset + 1) as usize] as u16) << 8);
            let control = bus.io[cnt_h_offset as usize] as u16
                | ((bus.io[(cnt_h_offset + 1) as usize] as u16) << 8);

            let was_enabled = self.timers[i].enabled;
            self.timers[i].reload = reload;
            self.timers[i].prescaler = (control & 3) as u8;
            self.timers[i].cascade = i > 0 && (control & (1 << 2)) != 0;
            self.timers[i].irq_enable = (control & (1 << 6)) != 0;
            self.timers[i].enabled = (control & (1 << 7)) != 0;

            // Reset counter on first enable
            if self.timers[i].enabled && !was_enabled {
                self.timers[i].counter = self.timers[i].reload;
                self.timers[i].cycles = 0;
            }
        }
    }

    /// Advance timers by the given number of CPU cycles.
    /// Returns which timers overflowed (bitmask).
    pub fn tick(&mut self, cpu_cycles: u32, bus: &mut Bus) -> u8 {
        let mut overflow_mask = 0u8;

        for i in 0..4 {
            if !self.timers[i].enabled || self.timers[i].cascade {
                continue;
            }

            let divisor = PRESCALER[self.timers[i].prescaler as usize];
            self.timers[i].cycles += cpu_cycles;

            while self.timers[i].cycles >= divisor {
                self.timers[i].cycles -= divisor;
                let (new_val, overflow) = self.timers[i].counter.overflowing_add(1);

                if overflow {
                    self.timers[i].counter = self.timers[i].reload;
                    overflow_mask |= 1 << i;

                    if self.timers[i].irq_enable {
                        let irq = match i {
                            0 => IRQ_TIMER0,
                            1 => IRQ_TIMER1,
                            2 => IRQ_TIMER2,
                            3 => IRQ_TIMER3,
                            _ => 0,
                        };
                        bus.request_interrupt(irq);
                    }

                    // Cascade: increment next timer
                    if i < 3 && self.timers[i + 1].cascade && self.timers[i + 1].enabled {
                        let (next_val, next_overflow) =
                            self.timers[i + 1].counter.overflowing_add(1);
                        if next_overflow {
                            self.timers[i + 1].counter = self.timers[i + 1].reload;
                            overflow_mask |= 1 << (i + 1);
                            if self.timers[i + 1].irq_enable {
                                let irq = match i + 1 {
                                    1 => IRQ_TIMER1,
                                    2 => IRQ_TIMER2,
                                    3 => IRQ_TIMER3,
                                    _ => 0,
                                };
                                bus.request_interrupt(irq);
                            }
                        } else {
                            self.timers[i + 1].counter = next_val;
                        }
                    }
                } else {
                    self.timers[i].counter = new_val;
                }
            }

            // Write back counter to I/O for reads
            let cnt_l_offset = REG_TM0CNT_L + (i as u32 * 4);
            bus.io[cnt_l_offset as usize] = self.timers[i].counter as u8;
            bus.io[(cnt_l_offset + 1) as usize] = (self.timers[i].counter >> 8) as u8;
        }

        overflow_mask
    }
}

impl Default for TimerController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timer_basic() {
        let mut bus = Bus::new();
        let mut timers = TimerController::new();

        // Set up Timer 0: prescaler 0 (1:1), enabled, no IRQ
        let cnt_h = REG_TM0CNT_H as usize;
        bus.io[cnt_h] = 0x80; // enabled
        bus.io[cnt_h + 1] = 0;

        // Reload = 0xFFF0 (overflow after 16 ticks)
        let cnt_l = REG_TM0CNT_L as usize;
        bus.io[cnt_l] = 0xF0;
        bus.io[cnt_l + 1] = 0xFF;

        timers.update_from_io(&bus);
        assert!(timers.timers[0].enabled);
        assert_eq!(timers.timers[0].counter, 0xFFF0);

        // Tick 15 cycles — should not overflow
        let overflow = timers.tick(15, &mut bus);
        assert_eq!(overflow, 0);
        assert_eq!(timers.timers[0].counter, 0xFFFF);

        // Tick 1 more — should overflow
        let overflow = timers.tick(1, &mut bus);
        assert_ne!(overflow & 1, 0);
        assert_eq!(timers.timers[0].counter, 0xFFF0); // reloaded
    }

    #[test]
    fn test_timer_cascade() {
        let mut bus = Bus::new();
        let mut timers = TimerController::new();

        // Timer 0: prescaler 0, enabled, reload = 0xFFFF
        bus.io[REG_TM0CNT_L as usize] = 0xFF;
        bus.io[(REG_TM0CNT_L + 1) as usize] = 0xFF;
        bus.io[REG_TM0CNT_H as usize] = 0x80;

        // Timer 1: cascade mode, enabled, reload = 0
        bus.io[REG_TM1CNT_L as usize] = 0;
        bus.io[(REG_TM1CNT_L + 1) as usize] = 0;
        bus.io[REG_TM1CNT_H as usize] = 0x80 | 0x04; // enabled + cascade

        timers.update_from_io(&bus);
        assert!(timers.timers[0].enabled);
        assert!(timers.timers[1].enabled);
        assert!(timers.timers[1].cascade);

        // Timer 0 starts at 0xFFFF, tick 1 = overflow
        let overflow = timers.tick(1, &mut bus);
        assert_ne!(overflow & 1, 0); // Timer 0 overflowed
        // Timer 1 should have incremented by 1 (cascade)
        assert_eq!(timers.timers[1].counter, 1);
    }

    #[test]
    fn test_timer_irq() {
        let mut bus = Bus::new();
        let mut timers = TimerController::new();

        // Enable Timer0 IRQ in IE
        bus.io_write16(REG_IE, IRQ_TIMER0);
        bus.io_write16(REG_IME, 1);

        // Timer 0: prescaler 0, enabled, IRQ enabled, reload = 0xFFFF
        bus.io[REG_TM0CNT_L as usize] = 0xFF;
        bus.io[(REG_TM0CNT_L + 1) as usize] = 0xFF;
        bus.io[REG_TM0CNT_H as usize] = 0x80 | 0x40; // enabled + IRQ

        timers.update_from_io(&bus);
        timers.tick(1, &mut bus); // overflow

        assert!(bus.has_pending_irq());
    }

    #[test]
    fn test_timer_prescaler() {
        let mut bus = Bus::new();
        let mut timers = TimerController::new();

        // Timer 0: prescaler 1 (64:1), enabled, reload = 0
        bus.io[REG_TM0CNT_L as usize] = 0;
        bus.io[(REG_TM0CNT_L + 1) as usize] = 0;
        bus.io[REG_TM0CNT_H as usize] = 0x80 | 0x01; // enabled, prescaler = 1

        timers.update_from_io(&bus);

        // 63 cycles — not enough for one tick
        timers.tick(63, &mut bus);
        assert_eq!(timers.timers[0].counter, 0);

        // 1 more cycle (total 64) — should tick once
        timers.tick(1, &mut bus);
        assert_eq!(timers.timers[0].counter, 1);
    }
}
