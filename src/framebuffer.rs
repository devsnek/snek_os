use spin::Mutex;
use bootloader::boot_info::FrameBufferInfo;
use noto_sans_mono_bitmap::{
    get_raster, get_raster_width, FontWeight, RasterHeight, RasterizedChar,
};
use core::fmt::Write;

static mut EMPTY_BUF: [u8;0] = [];

lazy_static! {
    static ref WRITER: Mutex<Writer> = Mutex::new(Writer {
        buffer: unsafe { &mut EMPTY_BUF },
        info: FrameBufferInfo {
            byte_len: 0,
            horizontal_resolution: 0,
            vertical_resolution: 0,
            pixel_format: bootloader::boot_info::PixelFormat::U8,
            bytes_per_pixel: 0,
            stride: 0,
        },
        x_pos: 0,
        y_pos: 0,
    });
}

pub(crate) fn init(info: FrameBufferInfo, buffer: &'static mut [u8]) {
    let mut writer = WRITER.lock();
    writer.info = info;
    writer.buffer = buffer;
    writer.clear();
}

#[doc(hidden)]
pub fn _print(args: core::fmt::Arguments) {
    crate::arch::without_interrupts(|| {
        WRITER.lock().write_fmt(args).unwrap();
    });
}

struct Writer {
    buffer: &'static mut [u8],
    info: bootloader::boot_info::FrameBufferInfo,
    x_pos: usize,
    y_pos: usize,
}

unsafe impl Send for Writer {}

const LINE_SPACING: usize = 2;
const LETTER_SPACING: usize = 0;
const BORDER_PADDING: usize = 1;
const CHAR_RASTER_HEIGHT: RasterHeight = RasterHeight::Size16;
const CHAR_RASTER_WIDTH: usize = get_raster_width(FontWeight::Regular, CHAR_RASTER_HEIGHT);
const BACKUP_CHAR: char = '�';
const FONT_WEIGHT: FontWeight = FontWeight::Regular;

impl Writer {
    fn newline(&mut self) {
        self.y_pos += CHAR_RASTER_HEIGHT.val() + LINE_SPACING;
        self.carriage_return()
    }

    fn carriage_return(&mut self) {
        self.x_pos = BORDER_PADDING;
    }

    pub fn clear(&mut self) {
        self.x_pos = BORDER_PADDING;
        self.y_pos = BORDER_PADDING;
        self.buffer.fill(0);
    }

    fn write_rendered_char(&mut self, rendered_char: RasterizedChar) {
        for (y, row) in rendered_char.raster().iter().enumerate() {
            for (x, byte) in row.iter().enumerate() {
                self.write_pixel(self.x_pos + x, self.y_pos + y, *byte);
            }
        }
        self.x_pos += rendered_char.width() + LETTER_SPACING;
    }

    fn write_pixel(&mut self, x: usize, y: usize, intensity: u8) {
        let pixel_offset = y * self.info.stride + x;
        let color = match self.info.pixel_format {
            bootloader::boot_info::PixelFormat::RGB => [intensity, intensity / 2, intensity, 0],
            bootloader::boot_info::PixelFormat::BGR => [intensity, intensity / 2, intensity, 0],
            bootloader::boot_info::PixelFormat::U8 => [if intensity > 200 { 0xf } else { 0 }, 0, 0, 0],
            other => {
                self.info.pixel_format = bootloader::boot_info::PixelFormat::RGB;
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
                let new_xpos = self.x_pos + CHAR_RASTER_WIDTH;
                if new_xpos >= self.info.horizontal_resolution {
                    self.newline();
                }
                let new_ypos =
                    self.y_pos + CHAR_RASTER_HEIGHT.val() + BORDER_PADDING;
                if new_ypos >= self.info.vertical_resolution {
                    self.clear();
                }
                self.write_rendered_char(get_char_raster(c));
            }
        }
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
        get_raster(
            c,
            FONT_WEIGHT,
            CHAR_RASTER_HEIGHT,
        )
    }
    get(c).unwrap_or_else(|| get(BACKUP_CHAR).expect("Should get raster of backup char."))
}
