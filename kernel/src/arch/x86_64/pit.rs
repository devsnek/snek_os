// MIT License
//
// Copyright (c) 2022 Eliza Weisman
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use super::interrupts::{set_interrupt_static, InterruptType};
use core::sync::atomic::{AtomicBool, Ordering};
use core::time::Duration;
use spin::Mutex;
use x86_64::instructions::port::Port;

const BASE_FREQUENCY_HZ: usize = 1193182;

lazy_static! {
    pub static ref PIT: Mutex<Pit> = {
        core::mem::forget(set_interrupt_static(0, InterruptType::EdgeHigh, on_tick));
        Mutex::new(Pit::new())
    };
}

pub static SLEEPING: AtomicBool = AtomicBool::new(false);

pub fn on_tick() {
    let _was_sleeping = super::pit::SLEEPING
        .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
        .is_ok();
}

pub struct Pit {
    channel0: Port<u8>,
    // channel1: Port<u8>,
    // channel2: Port<u8>,
    command: Port<u8>,
}

impl Pit {
    fn new() -> Self {
        let base = 0x40;
        Self {
            channel0: Port::new(base),
            // channel1: Port::new(base + 1),
            // channel2: Port::new(base + 2),
            command: Port::new(base + 3),
        }
    }

    pub fn sleep(&mut self, duration: Duration) {
        SLEEPING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .unwrap();

        let duration_ms = duration.as_millis() as u64 as f64;
        let ticks_per_ms = BASE_FREQUENCY_HZ as f64 / 1000.0;
        let target_time = ticks_per_ms * duration_ms;
        let divisor = libm::round(target_time) as u16;

        let interrupts_enabled = x86_64::instructions::interrupts::are_enabled();
        x86_64::instructions::interrupts::disable();

        let command = Command::new()
            .with(Command::BCD_BINARY, false)
            .with(Command::MODE, OperatingMode::Interrupt)
            .with(Command::ACCESS, AccessMode::Both)
            .with(Command::CHANNEL, ChannelSelect::Channel0);
        self.send_command(command);
        self.set_divisor(divisor);

        while SLEEPING.load(Ordering::Acquire) {
            x86_64::instructions::interrupts::enable_and_hlt();
        }
        if !interrupts_enabled {
            x86_64::instructions::interrupts::disable();
        }
    }

    fn set_divisor(&mut self, divisor: u16) {
        let low = divisor as u8;
        let high = (divisor >> 8) as u8;
        unsafe {
            self.channel0.write(low);
            self.channel0.write(high);
        }
    }

    fn send_command(&mut self, command: Command) {
        unsafe {
            self.command.write(command.bits());
        }
    }
}

mycelium_bitfield::bitfield! {
    struct Command<u8> {
        const BCD_BINARY: bool;
        const MODE: OperatingMode;
        const ACCESS: AccessMode;
        const CHANNEL: ChannelSelect;
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
#[repr(u8)]
enum OperatingMode {
    Interrupt = 0b000,
    HwOneshot = 0b001,
    RateGenerator = 0b010,
    SquareWave = 0b011,
    SwStrobe = 0b100,
    HwStrobe = 0b101,
    RateGenerator2 = 0b110,
    SquareWave2 = 0b111,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
#[repr(u8)]
enum AccessMode {
    LatchCount = 0b00,
    LowByte = 0b01,
    HighByte = 0b10,
    Both = 0b11,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
#[repr(u8)]
enum ChannelSelect {
    Channel0 = 0b00,
    Channel1 = 0b01,
    Channel2 = 0b10,
    Readback = 0b11,
}

impl mycelium_bitfield::FromBits<u8> for OperatingMode {
    const BITS: u32 = 3;
    type Error = core::convert::Infallible;

    fn try_from_bits(bits: u8) -> Result<Self, Self::Error> {
        Ok(match bits {
            0b000 => Self::Interrupt,
            0b001 => Self::HwOneshot,
            0b010 => Self::RateGenerator,
            0b011 => Self::SquareWave,
            0b100 => Self::SwStrobe,
            0b101 => Self::HwStrobe,
            0b110 => Self::RateGenerator2,
            0b111 => Self::SquareWave2,
            bits => unreachable!(
                "unexpected bitpattern for `AccessMode`: {:#b} (this \
                    should never happen as all 2-bit patterns are covered!)",
                bits
            ),
        })
    }

    fn into_bits(self) -> u8 {
        self as u8
    }
}

impl mycelium_bitfield::FromBits<u8> for AccessMode {
    const BITS: u32 = 2;
    type Error = core::convert::Infallible;

    fn try_from_bits(bits: u8) -> Result<Self, Self::Error> {
        Ok(match bits {
            0b00 => Self::LatchCount,
            0b01 => Self::LowByte,
            0b10 => Self::HighByte,
            0b11 => Self::Both,
            bits => unreachable!(
                "unexpected bitpattern for `AccessMode`: {:#b} (this \
                    should never happen as all 2-bit patterns are covered!)",
                bits
            ),
        })
    }

    fn into_bits(self) -> u8 {
        self as u8
    }
}

impl mycelium_bitfield::FromBits<u8> for ChannelSelect {
    const BITS: u32 = 2;
    type Error = core::convert::Infallible;

    fn try_from_bits(bits: u8) -> Result<Self, Self::Error> {
        Ok(match bits {
            0b00 => Self::Channel0,
            0b01 => Self::Channel1,
            0b10 => Self::Channel2,
            0b11 => Self::Readback,
            bits => unreachable!(
                "unexpected bitpattern for `ChannelSelect`: {:#b} (this \
                    should never happen as all 2-bit patterns are covered!)",
                bits
            ),
        })
    }

    fn into_bits(self) -> u8 {
        self as u8
    }
}
