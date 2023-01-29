use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use core::fmt::Write;
use noto_sans_mono_bitmap::{
    get_raster, FontWeight, RasterHeight, RasterizedChar,
};
use spin::Mutex;

static mut EMPTY_BUF: [u8; 0] = [];

lazy_static! {
    static ref WRITER: Mutex<Writer> = Mutex::new(Writer {
        buffer: unsafe { &mut EMPTY_BUF },
        info: FrameBufferInfo {
            byte_len: 0,
            width: 0,
            height: 0,
            pixel_format: PixelFormat::U8,
            bytes_per_pixel: 0,
            stride: 0,
        },
        x_pos: 0,
        y_pos: 0,
    });
}

pub(crate) fn init(info: FrameBufferInfo, buffer: &'static mut [u8]) {
    {
        let mut writer = WRITER.lock();
        writer.info = info;
        writer.buffer = buffer;
        writer.clear();

        writer.write_rgba(include_bytes!("../../../logo_text.rgba"));
    }

    println!("[FRAMEBUFFER] initialized");
}

#[doc(hidden)]
pub fn _print(args: core::fmt::Arguments) {
    crate::arch::without_interrupts(|| {
        WRITER.lock().write_fmt(args).unwrap();
    });
}

struct Writer {
    buffer: &'static mut [u8],
    info: FrameBufferInfo,
    x_pos: usize,
    y_pos: usize,
}

unsafe impl Send for Writer {}

const LINE_SPACING: usize = 2;
const LETTER_SPACING: usize = 0;
const BORDER_PADDING: usize = 1;
const CHAR_RASTER_HEIGHT: RasterHeight = RasterHeight::Size16;
const BACKUP_CHAR: char = '�';
const FONT_WEIGHT: FontWeight = FontWeight::Regular;
const LINE_HEIGHT: usize = CHAR_RASTER_HEIGHT.val() + LINE_SPACING;

impl Writer {
    fn newline(&mut self) {
        self.y_pos += LINE_HEIGHT;
        self.carriage_return();
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
        let bytes_per_pixel = self.info.bytes_per_pixel;

        let next_line_offset = (self.info.stride * LINE_HEIGHT) * bytes_per_pixel;

        let pixel_height = self.info.height - LINE_HEIGHT;
        let pixels = self.info.stride * pixel_height;
        let bytes = pixels * bytes_per_pixel;

        unsafe {
            core::ptr::copy(
                self.buffer.as_ptr().add(next_line_offset),
                self.buffer.as_mut_ptr(),
                bytes,
            );
        }
    }

    fn write_pixel(&mut self, x: usize, y: usize, mut r: u8, mut g: u8, mut b: u8, a: u8) {
        if a == 0 {
            return;
        }

        let pixel_offset = y * self.info.stride + x;
        let bytes_per_pixel = self.info.bytes_per_pixel;
        let byte_offset = pixel_offset * bytes_per_pixel;

        if a != 255 {
            let bytes = &self.buffer[byte_offset..(byte_offset + bytes_per_pixel)];
            let (r0, g0, b0) = match self.info.pixel_format {
                PixelFormat::Rgb => (bytes[0], bytes[1], bytes[2]),
                PixelFormat::Bgr => (bytes[2], bytes[1], bytes[0]),
                PixelFormat::U8 => (bytes[0], bytes[0], bytes[0]),
                other => {
                    self.info.pixel_format = PixelFormat::Rgb;
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

        let color = match self.info.pixel_format {
            PixelFormat::Rgb => [r, g, b, 0],
            PixelFormat::Bgr => [b, g, r, 0],
            PixelFormat::U8 => {
                let lum = 0.2126 * (r as f64) + 0.7125 * (g as f64) + 0.722 * (b as f64);
                [if lum > 200.0 { 0xf } else { 0 }, 0, 0, 0]
            }
            other => {
                self.info.pixel_format = PixelFormat::Rgb;
                panic!("pixel format {:?} not supported in logger", other)
            }
        };

        let bytes_per_pixel = self.info.bytes_per_pixel;
        let byte_offset = pixel_offset * bytes_per_pixel;

        self.buffer[byte_offset..(byte_offset + bytes_per_pixel)]
            .copy_from_slice(&color[..bytes_per_pixel]);
        let _ = unsafe { core::ptr::read_volatile(&self.buffer[byte_offset]) };
    }

    fn write_char(&mut self, c: char) {
        match c {
            '\n' => self.newline(),
            '\r' => self.carriage_return(),
            c => {
                let rendered_char = get_char_raster(c);
                let width = rendered_char.width() + LETTER_SPACING;

                let new_xpos = self.x_pos + width;
                if new_xpos >= self.info.width {
                    self.newline();
                }

                if self.y_pos + LINE_HEIGHT >= self.info.height {
                    self.scroll();
                    self.y_pos -= LINE_HEIGHT;
                }

                for (y, row) in rendered_char.raster().iter().enumerate() {
                    for (x, byte) in row.iter().enumerate() {
                        let r = *byte;
                        let g = *byte / 2;
                        let b = *byte;
                        self.write_pixel(self.x_pos + x, self.y_pos + y, r, g, b, 255);
                    }
                }

                self.x_pos += width;
            }
        }
    }

    fn write_rgba(&mut self, bytes: &[u8]) {
        let mut image = bytes.into_iter();

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

impl Write for Writer {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for c in s.chars() {
            self.write_char(c);
        }
        Ok(())
    }
}

fn get_char_raster(c: char) -> RasterizedChar {
    fn get(c: char) -> Option<RasterizedChar> {
        get_raster(c, FONT_WEIGHT, CHAR_RASTER_HEIGHT)
    }
    get(c).unwrap_or_else(|| get(BACKUP_CHAR).expect("Should get raster of backup char."))
}
