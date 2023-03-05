// This code is published under the terms of the 2-clause BSD licence (see below)
//
// Copyright (c) 2014, John Hodge (thePowersGang)
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are met:
//
// 1. Redistributions of source code must retain the above copyright notice,
//    this list of conditions and the following disclaimer.
//
// 2. Redistributions in binary form must reproduce the above copyright notice,
//    this list of conditions and the following disclaimer in the documentation
//    and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
// AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
// ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE
// LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
// CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
// SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
// INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
// CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
// ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
// POSSIBILITY OF SUCH DAMAGE.

#![no_std]

mod keyboard;
mod mouse;

use keyboard::{Keyboard, KeyboardKind};
pub use mouse::MouseState;
use mouse::{Mouse, MouseKind};
pub use pc_keyboard::DecodedKey;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The driver timed out while waiting to send data to or receive data from the controller.
    TimedOut,
    /// The controller failed the self test.
    SelfTestFailed,
    /// No available ports were found.
    NoPorts,
}

/// Identifies which IRQ is being processed.
#[derive(Debug, PartialEq, Eq)]
pub enum Irq {
    Irq1,
    Irq12,
}

/// This trait provides the actual functionality of communicating with the 8042 controller to the
/// driver instance.
///
/// One possible implementation would use x86 `inb` and `outb` instructions to
/// communicate on ports `0x60` and `0x64`:
/// ```rust
/// struct MyImpl;
///
/// impl Impl for MyImpl {
///     fn write_cmd(&mut self, cmd: u8) {
///         unsafe { asm!("outb 064h, al", in(al) cmd) };
///     }
///     fn read_status(&mut self) -> u8 {
///         let mut status;
///         unsafe { asm!("inb al, 064h", out(al) status) };
///         status
///     }
///     fn write_data(&mut self, cmd: u8) {
///         unsafe { asm!("outb 060h, al", in(al) cmd) };
///     }
///     fn read_data(&mut self) -> u8 {
///         let mut status;
///         unsafe { asm!("inb al, 060h", out(al) status) };
///         status
///     }
/// }
/// ```
pub trait Impl: core::fmt::Debug {
    fn write_cmd(&mut self, cmd: u8);
    fn read_status(&mut self) -> u8;
    fn write_data(&mut self, data: u8);
    fn read_data(&mut self) -> u8;
}

/// A driver for the Intel 8042 PS/2 Controller.
#[derive(Debug)]
pub struct Driver8042<T: Impl> {
    r#impl: T,
    port1: Option<Port>,
    port2: Option<Port>,
}

impl<T: Impl> Driver8042<T> {
    /// Create a new instance of the driver. An implementation of `Impl` must be passed in to
    /// provide the driver with hooks for communicating with the 8042 controller.
    pub const fn new(r#impl: T) -> Self {
        Self {
            r#impl,
            port1: None,
            port2: None,
        }
    }

    /// Initialize the driver.
    ///
    /// SAFETY: You must ensure that your system has an 8042 controller. One possible way of doing
    /// this is to check the presence via ACPI tables.
    pub unsafe fn init(&mut self) -> Result<(), Error> {
        // Disable controller
        self.write_cmd(0xAD)?;
        self.write_cmd(0xA7)?;
        self.flush()?;

        // Set config
        self.write_cmd(0x20)?;
        let mut config = self.read_data()?;
        config &= !((1 << 0) | (1 << 1) | (1 << 6));
        let can_have_second_port = config & (1 << 5) != 0;
        self.write_cmd(0x60)?;
        self.write_data(config)?;

        // Self test
        self.write_cmd(0xAA)?;
        match self.read_data() {
            Ok(0x55) => {}
            _ => {
                return Err(Error::SelfTestFailed);
            }
        }

        let has_second_port = if can_have_second_port {
            // Enable and disable 2nd port, see if the config changes in response
            self.write_cmd(0xA8)?;
            self.write_cmd(0x20)?;
            let config = self.read_data()?;
            self.write_cmd(0xA7)?;
            config & (1 << 5) == 0
        } else {
            false
        };

        self.flush()?;

        let port1_works = {
            self.write_cmd(0xAB)?;
            self.read_data()? == 0x00
        };
        let port2_works = if has_second_port {
            self.write_cmd(0xA9)?;
            self.read_data()? == 0x00
        } else {
            false
        };

        if !port1_works && !port2_works {
            return Err(Error::NoPorts);
        }

        self.write_cmd(0x20)?;
        let mut config = self.read_data()?;
        if port1_works {
            config |= 1 << 0;
        }
        if port2_works {
            config |= 1 << 1;
        }
        self.write_cmd(0x60)?;
        self.write_data(config)?;

        if port1_works {
            self.port1 = Some(Port::new());
            self.write_cmd(0xAE)?;
            self.write_data(0xFF)?;
        }
        if port2_works {
            self.port2 = Some(Port::new());
            self.write_cmd(0xA8)?;
            self.write_cmd(0xD4)?;
            self.write_data(0xFF)?;
        }

        Ok(())
    }

    /// The 8042 controller is connected to IRQ1 and IRQ12. You should call this function when
    /// these are asserted in order to update the driver with new data and receive the resulting
    /// processed keyboard and mouse events.
    pub fn interrupt(&mut self, irq: Irq) -> Option<Change> {
        let mask = match irq {
            Irq::Irq1 => 0x01,
            Irq::Irq12 => 0x20,
        };
        if self.r#impl.read_status() & mask == 0 {
            return None;
        }
        let data = self.r#impl.read_data();
        let port = match irq {
            Irq::Irq1 => &mut self.port1,
            Irq::Irq12 => &mut self.port2,
        };
        if let Some(port) = port {
            let (data, key) = port.handle_data(data);
            if let Some(data) = data {
                if irq == Irq::Irq12 {
                    self.r#impl.write_cmd(0xD4);
                }
                self.r#impl.write_data(data);
            }
            return key;
        }
        None
    }

    fn poll_out(&mut self) -> bool {
        self.r#impl.read_status() & 2 == 0
    }

    fn poll_in(&mut self) -> bool {
        self.r#impl.read_status() & 1 != 0
    }

    fn wait_out(&mut self) -> Result<(), Error> {
        const MAX_SPINS: usize = 1000;
        let mut spin_count = 0;
        while !self.poll_out() {
            spin_count += 1;
            if spin_count == MAX_SPINS {
                return Err(Error::TimedOut);
            }
        }
        Ok(())
    }

    fn wait_in(&mut self) -> Result<(), Error> {
        const MAX_SPINS: usize = 100 * 1000;
        let mut spin_count = 0;
        while !self.poll_in() {
            spin_count += 1;
            if spin_count == MAX_SPINS {
                return Err(Error::TimedOut);
            }
        }
        Ok(())
    }

    fn write_cmd(&mut self, cmd: u8) -> Result<(), Error> {
        self.wait_out()?;
        self.r#impl.write_cmd(cmd);
        Ok(())
    }

    fn write_data(&mut self, data: u8) -> Result<(), Error> {
        self.wait_out()?;
        self.r#impl.write_data(data);
        Ok(())
    }

    fn read_data(&mut self) -> Result<u8, Error> {
        self.wait_in()?;
        Ok(self.r#impl.read_data())
    }

    fn flush(&mut self) -> Result<(), Error> {
        while self.poll_in() {
            self.r#impl.read_data();
        }
        Ok(())
    }
}

/// A change in the state of the keyboard or the mouse.
#[derive(Debug)]
pub enum Change {
    /// A change in the state of the keyboard.
    Keyboard(DecodedKey),
    /// A change in the state of the mouse.
    Mouse(MouseState),
}

#[derive(Debug, Clone, Copy)]
enum EnumWaitState {
    DSAck,
    IdentAck,
    IdentB1,
    IdentB2(u8),
}

#[derive(Debug)]
enum PortState {
    None,
    Unknown,
    Enumerating(EnumWaitState),
    Keyboard(Keyboard),
    Mouse(Mouse),
}

#[derive(Debug)]
struct Port {
    state: PortState,
}

impl Port {
    fn new() -> Self {
        Self {
            state: PortState::None,
        }
    }

    fn handle_data(&mut self, data: u8) -> (Option<u8>, Option<Change>) {
        let (rv, state) = match self.state {
            PortState::None => {
                if data == 0xFA {
                    (None, None)
                } else if data == 0xAA {
                    (
                        Some(0xF5),
                        Some(PortState::Enumerating(EnumWaitState::DSAck)),
                    )
                } else {
                    (None, None)
                }
            }
            PortState::Unknown => (None, None),
            PortState::Enumerating(state) => match state {
                EnumWaitState::DSAck => {
                    if data == 0xFA {
                        (
                            Some(0xF2),
                            Some(PortState::Enumerating(EnumWaitState::IdentAck)),
                        )
                    } else if data == 0x00 {
                        (None, None)
                    } else {
                        (None, Some(PortState::Unknown))
                    }
                }
                EnumWaitState::IdentAck => {
                    if data == 0xFA {
                        (None, Some(PortState::Enumerating(EnumWaitState::IdentB1)))
                    } else {
                        (None, Some(PortState::Unknown))
                    }
                }
                EnumWaitState::IdentB1 => match data {
                    0x00 => Self::new_mouse(MouseKind::Standard),
                    0x03 => Self::new_mouse(MouseKind::Scroll),
                    0x04 => Self::new_mouse(MouseKind::FiveButton),
                    0xAB => (
                        None,
                        Some(PortState::Enumerating(EnumWaitState::IdentB2(data))),
                    ),
                    _ => (None, Some(PortState::Unknown)),
                },
                EnumWaitState::IdentB2(b1) => match (b1, data) {
                    (0xAB, 0x41) => Self::new_keyboard(KeyboardKind::MF2Emul),
                    (0xAB, 0x83) => Self::new_keyboard(KeyboardKind::MF2),
                    (0xAB, 0x84) => Self::new_keyboard(KeyboardKind::Short),
                    (0xAB, 0x85) => Self::new_keyboard(KeyboardKind::N97),
                    (0xAB, 0x86) => Self::new_keyboard(KeyboardKind::K122),
                    (0xAB, 0x90) => Self::new_keyboard(KeyboardKind::JapG),
                    (0xAB, 0x91) => Self::new_keyboard(KeyboardKind::JapP),
                    (0xAB, 0x92) => Self::new_keyboard(KeyboardKind::JapA),
                    (0xAB, 0xA1) => Self::new_keyboard(KeyboardKind::Sun),
                    (0xAB, 0xC1) => Self::new_keyboard(KeyboardKind::MF2Emul),
                    _ => (None, Some(PortState::Unknown)),
                },
            },
            PortState::Keyboard(ref mut keyboard) => {
                let (data, key) = keyboard.handle_data(data);
                return (data, key.map(Change::Keyboard));
            }
            PortState::Mouse(ref mut mouse) => {
                let state = mouse.handle_data(data);
                return (None, state.map(Change::Mouse));
            }
        };

        if let Some(state) = state {
            self.state = state;
        }

        (rv, None)
    }

    fn new_keyboard(kind: KeyboardKind) -> (Option<u8>, Option<PortState>) {
        let (data, keyboard) = Keyboard::new(kind);
        (data, Some(PortState::Keyboard(keyboard)))
    }

    fn new_mouse(kind: MouseKind) -> (Option<u8>, Option<PortState>) {
        let (data, mouse) = Mouse::new(kind);
        (data, Some(PortState::Mouse(mouse)))
    }
}
