use elf::{endian::AnyEndian, ElfBytes};

static mut KERNEL_SLICE: &[u8] = &[];
static mut KERNEL_IMAGE_OFFSET: usize = 0;

pub fn init(kernel_slice: &'static [u8], kernel_offset: usize) {
    unsafe {
        KERNEL_SLICE = kernel_slice;
        KERNEL_IMAGE_OFFSET = kernel_offset;
    }
}

#[derive(Debug)]
pub struct StackFrame {
    pub address: usize,
    pub function: Option<(usize, &'static str)>,
}

pub fn stack_trace<F>(limit: usize, mut f: F)
where
    F: FnMut(StackFrame),
{
    #[derive(Debug)]
    #[repr(C)]
    struct RawStackFrame {
        rbp: *const RawStackFrame,
        rip: *const (),
    }

    let mut frame: *const RawStackFrame;
    unsafe {
        asm!("mov {}, rbp", out(reg) frame);
    }

    let symbols = ElfBytes::<AnyEndian>::minimal_parse(unsafe { KERNEL_SLICE })
        .ok()
        .and_then(|file| match file.symbol_table() {
            Ok(Some(s)) => Some(s),
            _ => None,
        });

    let get_symbol = |addr: usize| -> Option<(usize, &'static str)> {
        let Some((table, strings)) = &symbols else { return None };
        for symbol in table.iter() {
            let translated_start = symbol.st_value as usize + unsafe { KERNEL_IMAGE_OFFSET };
            let translated_end = translated_start + (symbol.st_size as usize);
            if addr >= translated_start && addr < translated_end {
                if let Ok(r) = strings.get(symbol.st_name as _) {
                    return Some((translated_start, r));
                }
            }
        }
        None
    };

    for _ in 0..limit {
        if frame.is_null() {
            break;
        }

        let address = unsafe { (*frame).rip };
        if address.is_null() {
            break;
        }
        let address = address as usize;

        let function = get_symbol(address);
        f(StackFrame { address, function });

        unsafe {
            frame = (*frame).rbp;
        }
    }
}
