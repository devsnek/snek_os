#![no_std]

use core::ptr::NonNull;
use volatile::VolatilePtr;

// https://www.intel.com/content/dam/www/public/us/en/documents/technical-specifications/software-developers-hpet-spec-1-0a.pdf

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
        /// Timer 0 interrupt active (only for level-triggered mode)
        const T0_INT_STS: bool;
        /// ...
        const T1_INT_STS: bool;
        /// ...
        const T2_INT_STS: bool;
        const _RESERVED = 61;
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct Hpet {
    capabilities_and_id: CapabilitiesAndID,
    padding0: u64,
    configuration: Configuration,
    padding1: u64,
    interrupt_status: InterruptStatus,
    padding2: [u64; 25],
    counter_value: u64,
    padding3: u64,
    timers: [HpetTimer; 32],
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

#[derive(Debug)]
#[repr(C)]
pub struct HpetTimer {
    configuration_and_capabilities: ConfigurationAndCapabilities,
    comparator_value: u64,
    fsb_interrupt: u64,
}

impl Hpet {
    fn capabilities_and_id(&self) -> VolatilePtr<CapabilitiesAndID> {
        unsafe { VolatilePtr::new(NonNull::from(&self.capabilities_and_id)) }
    }

    fn configuration(&self) -> VolatilePtr<Configuration> {
        unsafe { VolatilePtr::new(NonNull::from(&self.configuration)) }
    }

    /// Get the number of timers.
    pub fn num_timers(&self) -> u8 {
        self.capabilities_and_id()
            .read()
            .get(CapabilitiesAndID::NUM_TIM_CAP) as u8
            + 1
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

    /// Enable or disable the counter.
    pub fn set_counter_enable(&mut self, enable: bool) {
        self.configuration().update(|mut c| {
            c.set(Configuration::ENABLE_CNF, enable);
            c
        })
    }
}
