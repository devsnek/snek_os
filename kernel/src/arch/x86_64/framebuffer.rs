use core::fmt::Write;
use limine::Framebuffer;
use noto_sans_mono_bitmap::{get_raster, FontWeight, RasterHeight, RasterizedChar};
use spin::Mutex;

use embedded_graphics::{pixelcolor::Rgb888, prelude::*, primitives::Rectangle};

static mut EMPTY_BUF: [u8; 0] = [];

#[derive(Debug, Clone, Copy)]
pub enum PixelFormat {
    Rgb,
    Bgr,
    U8,
    Unknown,
}

lazy_static! {
    pub static ref DISPLAY: Mutex<Display> = Mutex::new(Display {
        buffer: unsafe { &mut EMPTY_BUF },
        width: 0,
        height: 0,
        bytes_per_pixel: 0,
        pixel_format: PixelFormat::Bgr,
        stride: 0,
        x_pos: 0,
        y_pos: 0,
    });
}

pub fn init(info: &Framebuffer) {
    {
        let mut display = DISPLAY.lock();
        display.buffer =
            unsafe { core::slice::from_raw_parts_mut(info.address.as_ptr().unwrap(), info.size()) };
        display.width = info.width as _;
        display.height = info.height as _;
        display.bytes_per_pixel = (info.bpp / 8) as _;
        display.stride = (info.pitch / 4) as _;
        display.pixel_format = match (
            info.red_mask_shift,
            info.green_mask_shift,
            info.blue_mask_shift,
        ) {
            (0x00, 0x08, 0x10) => PixelFormat::Rgb,
            (0x10, 0x08, 0x00) => PixelFormat::Bgr,
            (0x00, 0x00, 0x00) => PixelFormat::U8,
            _ => PixelFormat::Unknown,
        };
        display.clear();

        display.write_rgba(include_bytes!(concat!(env!("OUT_DIR"), "/logo_text.rgba")));
    }

    println!("[FRAMEBUFFER] initialized");
}

#[doc(hidden)]
pub fn _print(args: core::fmt::Arguments) {
    crate::arch::without_interrupts(|| {
        DISPLAY.lock().write_fmt(args).unwrap();
    });
}

const LINE_SPACING: usize = 2;
const BORDER_PADDING: usize = 1;
const CHAR_RASTER_HEIGHT: RasterHeight = RasterHeight::Size16;
const FONT_WEIGHT: FontWeight = FontWeight::Regular;
const LINE_HEIGHT: usize = CHAR_RASTER_HEIGHT.val() + LINE_SPACING;
const BACKUP_CHAR: char = '\u{FFFD}';

fn get_char_raster(c: char) -> RasterizedChar {
    fn get(c: char) -> Option<RasterizedChar> {
        get_raster(c, FONT_WEIGHT, CHAR_RASTER_HEIGHT)
    }
    get(c).unwrap_or_else(|| get(BACKUP_CHAR).expect("Should get raster of backup char."))
}

pub struct Display {
    buffer: &'static mut [u8],
    width: usize,
    height: usize,
    stride: usize,
    bytes_per_pixel: usize,
    pixel_format: PixelFormat,
    x_pos: usize,
    y_pos: usize,
}

unsafe impl Send for Display {}

impl Display {
    fn newline(&mut self) {
        self.y_pos += LINE_HEIGHT;
        self.carriage_return();

        if self.y_pos + LINE_HEIGHT >= self.height {
            self.scroll();
            self.y_pos -= LINE_HEIGHT;
        }
    }

    fn carriage_return(&mut self) {
        self.x_pos = BORDER_PADDING;
    }

    fn clear(&mut self) {
        self.x_pos = BORDER_PADDING;
        self.y_pos = BORDER_PADDING;
        self.buffer.fill(0);
    }

    fn scroll(&mut self) {
        let bytes_per_pixel = self.bytes_per_pixel;

        let next_line_offset = (self.stride * LINE_HEIGHT) * bytes_per_pixel;

        let pixel_height = self.height - LINE_HEIGHT;
        let pixels = self.stride * pixel_height;
        let bytes = pixels * bytes_per_pixel;

        self.buffer.copy_within(next_line_offset.., 0);
        self.buffer[(bytes + 1)..].fill(0);
    }

    fn write_pixel(&mut self, x: usize, y: usize, mut r: u8, mut g: u8, mut b: u8, a: u8) {
        if a == 0 {
            return;
        }

        let pixel_offset = y * self.stride + x;
        let bytes_per_pixel = self.bytes_per_pixel;
        let byte_offset = pixel_offset * bytes_per_pixel;

        if a != 255 {
            let bytes = &self.buffer[byte_offset..(byte_offset + bytes_per_pixel)];
            let (r0, g0, b0) = match self.pixel_format {
                PixelFormat::Rgb => (bytes[0], bytes[1], bytes[2]),
                PixelFormat::Bgr => (bytes[2], bytes[1], bytes[0]),
                PixelFormat::U8 => (bytes[0], bytes[0], bytes[0]),
                other => {
                    self.pixel_format = PixelFormat::Rgb;
                    panic!("pixel format {:?} not supported in logger", other)
                }
            };

            let blend = |old: u8, new: u8| {
                let alpha = a as f64 / 255.0;
                let x = alpha * (new as f64);
                let z = (1.0 - alpha) * (old as f64);
                (x + z) as u8
            };

            r = blend(r0, r);
            g = blend(g0, g);
            b = blend(b0, b);
        }

        let color = match self.pixel_format {
            PixelFormat::Rgb => [r, g, b, 0],
            PixelFormat::Bgr => [b, g, r, 0],
            PixelFormat::U8 => {
                let lum = 0.2126 * (r as f64) + 0.7125 * (g as f64) + 0.722 * (b as f64);
                [if lum > 200.0 { 0xf } else { 0 }, 0, 0, 0]
            }
            other => {
                self.pixel_format = PixelFormat::Rgb;
                panic!("pixel format {:?} not supported in logger", other)
            }
        };

        self.buffer[byte_offset..(byte_offset + bytes_per_pixel)]
            .copy_from_slice(&color[..bytes_per_pixel]);
    }

    fn write_char(&mut self, c: char) {
        match c {
            '\n' => self.newline(),
            '\r' => self.carriage_return(),
            c => {
                let rendered_char = get_char_raster(c);
                let width = rendered_char.width();

                let new_xpos = self.x_pos + width;
                if new_xpos >= self.width {
                    self.newline();
                }

                for (y, row) in rendered_char.raster().iter().enumerate() {
                    for (x, byte) in row.iter().enumerate() {
                        let r = *byte;
                        let g = *byte;
                        let b = *byte;
                        let a = 255;
                        self.write_pixel(self.x_pos + x, self.y_pos + y, r, g, b, a);
                    }
                }

                self.x_pos += width;
            }
        }
    }

    fn write_rgba(&mut self, bytes: &[u8]) {
        let mut image = bytes.iter();

        let width = u32::from_be_bytes([(); 4].map(|_| *image.next().unwrap()));
        let height = u32::from_be_bytes([(); 4].map(|_| *image.next().unwrap()));

        for _ in 0..height {
            for x in 0..width {
                let r = *image.next().unwrap();
                let g = *image.next().unwrap();
                let b = *image.next().unwrap();
                let a = *image.next().unwrap();
                self.write_pixel(self.x_pos + x as usize, self.y_pos, r, g, b, a);
            }
            self.y_pos += 1;
        }

        self.x_pos = 0;
        self.y_pos += LINE_SPACING;
    }
}

impl Write for Display {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for c in s.chars() {
            self.write_char(c);
        }
        Ok(())
    }
}

impl Dimensions for Display {
    fn bounding_box(&self) -> Rectangle {
        Rectangle {
            top_left: Point { x: 0, y: 0 },
            size: Size {
                width: self.width as _,
                height: self.height as _,
            },
        }
    }
}

impl DrawTarget for Display {
    type Color = Rgb888;
    type Error = ();

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for pixel in pixels {
            self.write_pixel(
                pixel.0.x as _,
                pixel.0.y as _,
                pixel.1.r(),
                pixel.1.g(),
                pixel.1.b(),
                255,
            );
        }
        Ok(())
    }
}
