#![no_std]
#![doc=include_str!("../README.md")]

use core::ptr::NonNull;
use volatile::{access::ReadOnly, VolatilePtr};

mycelium_bitfield::bitfield! {
    struct CapabilitiesAndID<u64> {
        /// Function revision
        const REV_ID: u8;
        /// Number of timers
        const NUM_TIM_CAP = 4;
        /// false = 32 bits, true = 64 bits
        const COUNT_SIZE_CAP: bool;
        const _RESERVED = 1;
        /// Indicates that irq 0 is supported
        const LEG_ROUTE_CAP: bool;
        /// Same as if this was a PCI function
        const VENDOR_ID: u16;
        /// Period of counter increments in femtoseconds(!)
        const COUNTER_CLK_PERIOD: u32;
    }
}

mycelium_bitfield::bitfield! {
    struct Configuration<u64> {
        /// false - halt counter and disable interrupts
        /// true - allow main counter to run and allow interrupts
        const ENABLE_CNF: bool;
        /// Enable legacy interrupt (irq 0)
        const LEG_ROUTE_CNF: bool;
        const _RESERVED = 62;
    }
}

mycelium_bitfield::bitfield! {
    struct InterruptStatus<u64> {
        /// Tn_INT_STS 0-31
        const T_INT_STS: u32;
        const _RESERVED = 32;
    }
}

mycelium_bitfield::bitfield! {
    struct ConfigurationAndCapabilities<u64> {
        const _RESERVED0 = 1;
        const TN_INT_TYPE_CNF: bool;
        const TN_INT_ENB_CNF: bool;
        const TN_TYPE_CNF: bool;
        const TN_PER_INT_CAP: bool;
        const TN_SIZE_CAP: bool;
        const TN_VAL_SET_CNF: bool;
        const _RESERVED1 = 1;
        const TN_32MODE_CNF: bool;
        const TN_INT_ROUTE_CNF = 4;
        const TN_FSB_EN_CNF: bool;
        const TN_FSB_INT_DEL_CAP: bool;
        const _RESERVED3 = 16;
        const TN_INT_ROUTE_CAP: u32;
    }
}

/// Represents the HPET in memory.
#[derive(Debug)]
#[repr(C)]
pub struct Hpet {
    capabilities_and_id: CapabilitiesAndID,
    padding0: u64,
    configuration: Configuration,
    padding1: u64,
    general_interrupt_status: InterruptStatus,
    padding2: [u64; 25],
    counter_value: u64,
    padding3: u64,
    timers: [HpetTimer; 32],
}

/// Represents an HPET timer in memory.
#[derive(Debug)]
#[repr(C)]
pub struct HpetTimer {
    configuration_and_capabilities: ConfigurationAndCapabilities,
    comparator_value: u64,
    fsb_interrupt: u64,
}

impl Hpet {
    fn capabilities_and_id(&self) -> VolatilePtr<CapabilitiesAndID, ReadOnly> {
        unsafe { VolatilePtr::new_read_only(NonNull::from(&self.capabilities_and_id)) }
    }

    fn configuration(&self) -> VolatilePtr<Configuration> {
        unsafe { VolatilePtr::new(NonNull::from(&self.configuration)) }
    }

    fn general_interrupt_status(&self) -> VolatilePtr<InterruptStatus> {
        unsafe { VolatilePtr::new(NonNull::from(&self.general_interrupt_status)) }
    }

    /// Get the counter period in femtoseconds.
    pub fn counter_period(&self) -> u32 {
        self.capabilities_and_id()
            .read()
            .get(CapabilitiesAndID::COUNTER_CLK_PERIOD)
    }

    /// Get the current counter value.
    pub fn counter_value(&self) -> u64 {
        unsafe { (&self.counter_value as *const u64).read_volatile() }
    }

    /// If using level-triggered interrupts, get whether a timer is asserting.
    pub fn interrupt_status(&self, timer: u8) -> bool {
        let timer = timer as u32;
        self.general_interrupt_status()
            .read()
            .get(InterruptStatus::T_INT_STS)
            & timer
            == timer
    }

    /// If using level-triggered interrupts, clear an asserting timer.
    pub fn clear_interrupt_status(&mut self, timer: u8) {
        self.general_interrupt_status().update(|mut s| {
            let mut sts = s.get(InterruptStatus::T_INT_STS);
            sts |= 1 << timer;
            s.set(InterruptStatus::T_INT_STS, sts);
            s
        })
    }

    /// Enable or disable the counter.
    pub fn set_counter_enable(&mut self, enable: bool) {
        self.configuration().update(|mut c| {
            c.set(Configuration::ENABLE_CNF, enable);
            c
        })
    }

    /// Get the available timers.
    pub fn timers(&self) -> &[HpetTimer] {
        let last_timer_index = self
            .capabilities_and_id()
            .read()
            .get(CapabilitiesAndID::NUM_TIM_CAP) as usize;
        &self.timers[..=last_timer_index]
    }
}
