#![no_std]

use chrono::{NaiveDate, NaiveDateTime};
use x86_64::instructions::port::Port;

#[derive(Debug)]
pub struct CMOS {
    address_port: Port<u8>,
    data_port: Port<u8>,
}

impl CMOS {
    pub fn new() -> Self {
        Self {
            address_port: Port::<u8>::new(0x70),
            data_port: Port::<u8>::new(0x71),
        }
    }

    fn read(&mut self, reg: u8) -> u8 {
        unsafe {
            self.address_port.write(reg);
            self.data_port.read()
        }
    }

    pub fn read_rtc(&mut self, century_reg: u8) -> NaiveDateTime {
        let mut rtc_time = RTCDateTime {
            second: 0,
            minute: 0,
            hour: 0,
            day: 0,
            month: 0,
            year: 0,
        };
        let mut last_rtc_time = rtc_time;

        loop {
            loop {
                rtc_time.second = self.read(0x00);
                rtc_time.minute = self.read(0x02);
                rtc_time.hour = self.read(0x04);
                rtc_time.day = self.read(0x07);
                rtc_time.month = self.read(0x08);
                rtc_time.year = self.read(0x09) as usize;
                if (self.read(0x0A) & 0x80) == 0 {
                    break;
                }
            }

            if rtc_time == last_rtc_time {
                break;
            }

            last_rtc_time = rtc_time;
        }

        let register_b = self.read(0x0B);

        let mut century = self.read(century_reg);

        if (register_b & 0x04) == 0 {
            rtc_time.second = (rtc_time.second & 0x0F) + ((rtc_time.second / 16) * 10);
            rtc_time.minute = (rtc_time.minute & 0x0F) + ((rtc_time.minute / 16) * 10);
            rtc_time.hour = ((rtc_time.hour & 0x0F) + (((rtc_time.hour & 0x70) / 16) * 10))
                | (rtc_time.hour & 0x80);
            rtc_time.day = (rtc_time.day & 0x0F) + ((rtc_time.day / 16) * 10);
            rtc_time.month = (rtc_time.month & 0x0F) + ((rtc_time.month / 16) * 10);
            rtc_time.year = (rtc_time.year & 0x0F) + ((rtc_time.year / 16) * 10);
            century = (century & 0x0F) + ((century / 16) * 10);
        }

        if ((register_b & 0x02) == 0) && ((rtc_time.hour & 0x80) != 0) {
            rtc_time.hour = ((rtc_time.hour & 0x7F) + 12) % 24;
        }

        rtc_time.year += century as usize * 100;

        NaiveDate::from_ymd_opt(rtc_time.year as _, rtc_time.month as _, rtc_time.day as _)
            .unwrap()
            .and_hms_opt(
                rtc_time.hour as _,
                rtc_time.minute as _,
                rtc_time.second as _,
            )
            .unwrap()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct RTCDateTime {
    pub year: usize,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}
