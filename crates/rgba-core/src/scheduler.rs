use serde::{Deserialize, Serialize};

/// Types of scheduled events
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EventKind {
    /// HBlank start (end of drawing)
    HBlank,
    /// HBlank end (start of next scanline)
    HBlankEnd,
    /// VBlank start
    VBlank,
    /// VBlank end (start of new frame)
    VBlankEnd,
    /// Timer 0 overflow
    Timer0Overflow,
    /// Timer 1 overflow
    Timer1Overflow,
    /// Timer 2 overflow
    Timer2Overflow,
    /// Timer 3 overflow
    Timer3Overflow,
    /// DMA channel 0-3
    Dma0,
    Dma1,
    Dma2,
    Dma3,
    /// APU sample generation
    ApuSample,
    /// APU channel sequencer tick
    ApuSequencer,
}

/// A scheduled event with a cycle timestamp
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub kind: EventKind,
    pub timestamp: u64,
}

impl Event {
    pub fn new(kind: EventKind, timestamp: u64) -> Self {
        Self { kind, timestamp }
    }
}

/// Cycle-accurate event scheduler
///
/// Events are kept in a sorted list (by timestamp). The scheduler drives
/// all hardware timing in the emulator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scheduler {
    /// Current cycle count (global monotonic counter)
    pub current_cycle: u64,
    /// Sorted list of pending events (earliest first)
    events: Vec<Event>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            current_cycle: 0,
            events: Vec::with_capacity(32),
        }
    }

    /// Schedule an event at an absolute cycle timestamp
    pub fn schedule(&mut self, kind: EventKind, timestamp: u64) {
        // Remove any existing event of the same kind
        self.events.retain(|e| e.kind != kind);

        let event = Event::new(kind, timestamp);

        // Insert sorted by timestamp (earliest first)
        let pos = self.events.partition_point(|e| e.timestamp <= timestamp);
        self.events.insert(pos, event);
    }

    /// Schedule an event relative to the current cycle
    pub fn schedule_relative(&mut self, kind: EventKind, cycles_from_now: u64) {
        self.schedule(kind, self.current_cycle + cycles_from_now);
    }

    /// Cancel a pending event of the given kind
    pub fn cancel(&mut self, kind: EventKind) {
        self.events.retain(|e| e.kind != kind);
    }

    /// Get the next event without removing it
    pub fn peek(&self) -> Option<&Event> {
        self.events.first()
    }

    /// How many cycles until the next event (0 if past due)
    pub fn cycles_until_next(&self) -> u64 {
        match self.events.first() {
            Some(event) => event.timestamp.saturating_sub(self.current_cycle),
            None => u64::MAX,
        }
    }

    /// Pop the next event if it's due (timestamp <= current_cycle)
    pub fn pop_pending(&mut self) -> Option<Event> {
        if let Some(event) = self.events.first() {
            if event.timestamp <= self.current_cycle {
                return Some(self.events.remove(0));
            }
        }
        None
    }

    /// Advance the current cycle count
    pub fn advance(&mut self, cycles: u64) {
        self.current_cycle += cycles;
    }

    /// Check if a specific event kind is scheduled
    pub fn is_scheduled(&self, kind: EventKind) -> bool {
        self.events.iter().any(|e| e.kind == kind)
    }

    /// Get the number of pending events
    pub fn pending_count(&self) -> usize {
        self.events.len()
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schedule_and_pop() {
        let mut sched = Scheduler::new();
        sched.schedule(EventKind::HBlank, 100);
        sched.schedule(EventKind::VBlank, 50);

        // VBlank should be first (earlier)
        assert_eq!(sched.peek().unwrap().kind, EventKind::VBlank);

        // Not yet due
        assert!(sched.pop_pending().is_none());

        // Advance to 50
        sched.advance(50);
        let event = sched.pop_pending().unwrap();
        assert_eq!(event.kind, EventKind::VBlank);

        // HBlank not yet due
        assert!(sched.pop_pending().is_none());

        // Advance to 100
        sched.advance(50);
        let event = sched.pop_pending().unwrap();
        assert_eq!(event.kind, EventKind::HBlank);
    }

    #[test]
    fn test_cancel_event() {
        let mut sched = Scheduler::new();
        sched.schedule(EventKind::Timer0Overflow, 200);
        assert!(sched.is_scheduled(EventKind::Timer0Overflow));

        sched.cancel(EventKind::Timer0Overflow);
        assert!(!sched.is_scheduled(EventKind::Timer0Overflow));
    }

    #[test]
    fn test_schedule_relative() {
        let mut sched = Scheduler::new();
        sched.advance(1000);
        sched.schedule_relative(EventKind::ApuSample, 512);

        assert_eq!(sched.peek().unwrap().timestamp, 1512);
    }

    #[test]
    fn test_reschedule_replaces() {
        let mut sched = Scheduler::new();
        sched.schedule(EventKind::HBlank, 100);
        sched.schedule(EventKind::HBlank, 200);

        // Should have only one HBlank event
        assert_eq!(sched.pending_count(), 1);
        assert_eq!(sched.peek().unwrap().timestamp, 200);
    }
}
