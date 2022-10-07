use nrf52832_pac as pac;

use crate::ppi::{Event, Task};

/// Note:
/// PRESCALER on page 239 and the BITMODE on page 239 must only be updated when the timer
/// is stopped. If these registers are updated while the TIMER is started then this may result in unpredictable
/// behavior.

#[repr(u8)]
pub enum Frequency {
    // I'd prefer not to prefix these with `F`, but Rust identifiers can't start with digits.
    F16MHz = 0,
    F8MHz = 1,
    F4MHz = 2,
    F2MHz = 3,
    F1MHz = 4,
    F500kHz = 5,
    F250kHz = 6,
    F125kHz = 7,
    F62500Hz = 8,
    F31250Hz = 9,
}

pub enum Bitmode {
    B8 = 1,
    B16 = 0,
    B24 = 2,
    B32 = 3,
}

pub enum Prescaler {}

pub struct NotConfigured;
pub struct CounterType;
pub struct TimerType;

pub fn get_timer() -> Timer<NotConfigured> {
    let timer = Timer {
        _mode: NotConfigured,
        bitmode: Bitmode::B24, // The default bitmode
    };
    timer.stop(); // Initialize the counter at 0.
    timer.clear(); // Appearently necessary for proper functioning!

    // Not really necessary...
    for n in 0..4 {
        let cc = timer.cc(n);
        // Initialize all the shorts as disabled.
        cc.unshort_compare_clear();
        cc.unshort_compare_stop();
        // Initialize the CC registers as 0.
        cc.write(0);
    }
    timer
}

pub struct Timer<MODE> {
    // periph: pac::TIMER0,
    _mode: MODE,
    bitmode: Bitmode,
}

/// These functions may be used by any timer
impl<MODE> Timer<MODE> {
    fn regs(&self) -> &'static pac::timer0::RegisterBlock {
        unsafe { &*(pac::TIMER0::ptr() as *const pac::timer0::RegisterBlock) }
    }

    /// Adjusts the bitmode of the current timer.
    pub fn with_bitmode(self, bitmode: Bitmode) -> Timer<MODE> {
        self.set_bitmode(&bitmode);
        Timer { bitmode, ..self }
    }

    /// Sets the bitmode of the timer.
    fn set_bitmode(&self, bitmode: &Bitmode) {
        self.stop();
        // Set bit width
        self.regs().bitmode.write(|w| match bitmode {
            Bitmode::B8 => w.bitmode()._08bit(),
            Bitmode::B16 => w.bitmode()._16bit(),
            Bitmode::B24 => w.bitmode()._24bit(),
            Bitmode::B32 => w.bitmode()._32bit(),
        });
    }

    /// Starts the timer.
    pub fn start(&self) {
        self.regs().tasks_start.write(|w| unsafe { w.bits(1) })
    }

    /// Stops the timer.
    pub fn stop(&self) {
        self.regs().tasks_stop.write(|w| unsafe { w.bits(1) })
    }

    /// Reset the timer's counter to 0.
    pub fn clear(&self) {
        self.regs().tasks_clear.write(|w| unsafe { w.bits(1) })
    }

    /// Returns the START task, for use with PPI.
    ///
    /// When triggered, this task starts the timer.
    pub fn task_start(&self) -> Task {
        Task::from_reg(&self.regs().tasks_start)
    }

    /// Returns the STOP task, for use with PPI.
    ///
    /// When triggered, this task stops the timer.
    pub fn task_stop(&self) -> Task {
        Task::from_reg(&self.regs().tasks_shutdown)
    }

    /// Returns the CLEAR task, for use with PPI.
    ///
    /// When triggered, this task resets the timer's counter to 0.
    pub fn task_clear(&self) -> Task {
        Task::from_reg(&self.regs().tasks_clear)
    }

    /// Returns this timer's `n`th CC register.
    ///
    /// # Panics
    /// Panics if `n` >= the number of CC registers this timer has (4 for a normal timer, 6 for an extended timer).
    pub fn cc(&self, n: usize) -> Cc {
        if n >= 4 {
            panic!("Cannot get CC register {} of timer with {} CC registers.", n, 4);
        }
        Cc { n }
    }

    // pub(crate) fn new() -> Self {
    //     Self {
    //         enabled: Disabled,
    //         mode: NotConfigured,
    //     }
    // }
}

/// These functions may only be used on Timers (so not counters).
impl Timer<TimerType> {
    /// Change the timer's frequency.
    ///
    /// This will stop the timer if it isn't already stopped,
    /// because the timer may exhibit 'unpredictable behaviour' if it's frequency is changed while it's running.
    pub fn set_frequency(&self, frequency: Frequency) {
        self.stop();
        self.regs()
            .prescaler
            // SAFETY: `frequency` is a variant of `Frequency`,
            // whose values are all in the range of 0-9 (the valid range of `prescaler`).
            .write(|w| unsafe { w.prescaler().bits(frequency as u8) })
    }
}

/// These functions may only be used on Counters (so not timers).
impl Timer<CounterType> {
    /// Returns the COUNT task, for use with PPI.
    ///
    /// When triggered, this task increments the counter.
    pub fn task_count(&self) -> Task {
        Task::from_reg(&self.regs().tasks_count)
    }
}

/// These functions may only be used on non-configured timers.
impl Timer<NotConfigured> {
    pub fn into_counter(self) -> Timer<CounterType> {
        self.regs().mode.write(|w| w.mode().low_power_counter());

        Timer {
            _mode: CounterType,
            bitmode: self.bitmode,
        }
    }

    pub fn into_timer(self) -> Timer<TimerType> {
        self.regs().mode.write(|w| w.mode().timer());

        Timer {
            _mode: TimerType,
            bitmode: self.bitmode,
        }
    }
}

/// A representation of a timer's Capture/Compare (CC) register.
///
/// A CC register holds a 32-bit value.
/// This is used either to store a capture of the timer's current count, or to specify the value for the timer to compare against.
///
/// The timer will fire the register's COMPARE event when its counter reaches the value stored in the register.
/// When the register's CAPTURE task is triggered, the timer will store the current value of its counter in the register
pub struct Cc {
    n: usize,
}

impl Cc {
    fn regs(&self) -> &'static pac::timer0::RegisterBlock {
        unsafe { &*pac::TIMER0::ptr() }
    }

    /// Get the current value stored in the register.
    pub fn read(&self) -> u32 {
        self.regs().cc[self.n].read().cc().bits()
    }

    /// Set the value stored in the register.
    ///
    /// `event_compare` will fire when the timer's counter reaches this value.
    pub fn write(&self, value: u32) {
        // SAFETY: there are no invalid values for the CC register.
        self.regs().cc[self.n].write(|w| unsafe { w.cc().bits(value) })
    }

    /// Capture the current value of the timer's counter in this register, and return it.
    pub fn capture(&self) -> u32 {
        self.regs().tasks_capture[self.n].write(|w| unsafe { w.bits(1) });
        self.read()
    }

    /// Returns this CC register's CAPTURE task, for use with PPI.
    ///
    /// When triggered, this task will capture the current value of the timer's counter in this register.
    pub fn task_capture(&self) -> Task {
        Task::from_reg(&self.regs().tasks_capture)
    }

    /// Returns this CC register's COMPARE event, for use with PPI.
    ///
    /// This event will fire when the timer's counter reaches the value in this CC register.
    pub fn event_compare(&self) -> Event {
        Event::from_reg(&self.regs().events_compare[self.n])
    }

    /// Enable the shortcut between this CC register's COMPARE event and the timer's CLEAR task.
    ///
    /// This means that when the COMPARE event is fired, the CLEAR task will be triggered.
    ///
    /// So, when the timer's counter reaches the value stored in this register, the timer's counter will be reset to 0.
    pub fn short_compare_clear(&self) {
        self.regs()
            .shorts
            .modify(|r, w| unsafe { w.bits(r.bits() | (1 << self.n)) })
    }

    /// Disable the shortcut between this CC register's COMPARE event and the timer's CLEAR task.
    pub fn unshort_compare_clear(&self) {
        self.regs()
            .shorts
            .modify(|r, w| unsafe { w.bits(r.bits() & !(1 << self.n)) })
    }

    /// Enable the shortcut between this CC register's COMPARE event and the timer's STOP task.
    ///
    /// This means that when the COMPARE event is fired, the STOP task will be triggered.
    ///
    /// So, when the timer's counter reaches the value stored in this register, the timer will stop counting up.
    pub fn short_compare_stop(&self) {
        self.regs()
            .shorts
            .modify(|r, w| unsafe { w.bits(r.bits() | (1 << (8 + self.n))) })
    }

    /// Disable the shortcut between this CC register's COMPARE event and the timer's STOP task.
    pub fn unshort_compare_stop(&self) {
        self.regs()
            .shorts
            .modify(|r, w| unsafe { w.bits(r.bits() & !(1 << (8 + self.n))) })
    }
}
