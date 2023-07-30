use x86_64::VirtAddr;
use xmas_elf::{
    program::Type as ProgramType,
    sections::{SectionData, ShType},
    symbol_table::Entry,
    ElfFile,
};

static mut KERNEL_SLICE: &[u8] = &[];
static mut KERNEL_BASE: usize = 0;

pub fn init(kernel_slice: &'static [u8], kernel_base: VirtAddr) {
    unsafe {
        KERNEL_SLICE = kernel_slice;
        KERNEL_BASE = kernel_base.as_u64() as _;
    }
}

#[derive(Debug)]
pub struct StackFrame {
    pub address: usize,
    pub function: Option<(usize, &'static str)>,
}

#[derive(Debug)]
#[repr(C)]
struct RawStackFrame {
    rbp: *const RawStackFrame,
    rip: *const (),
}

pub fn stack_trace<F>(limit: usize, mut f: F)
where
    F: FnMut(StackFrame),
{
    let mut frame: *const RawStackFrame;
    unsafe {
        asm!("mov {}, rbp", out(reg) frame);
    }

    let file = ElfFile::new(unsafe { KERNEL_SLICE }).unwrap();
    let symbols = file.section_iter().find_map(|s| {
        if s.get_type() != Ok(ShType::SymTab) {
            return None;
        }
        let data = s.get_data(&file).unwrap();
        if let SectionData::SymbolTable64(table) = data {
            return Some(table);
        }
        None
    });

    let link_base = file
        .program_iter()
        .find_map(|h| {
            if h.get_type() != Ok(ProgramType::Load) {
                return None;
            }
            if !h.flags().is_execute() {
                return None;
            }
            Some(h.virtual_addr() as usize)
        })
        .unwrap_or(0xffffffff80000000);
    let load_base = unsafe { KERNEL_BASE };
    let offset = load_base - link_base;

    let get_symbol = |addr: usize| -> Option<(usize, &'static str)> {
        let Some(table) = &symbols else {
            return None;
        };
        for symbol in table.iter() {
            let translated_start = symbol.value() as usize + offset;
            let translated_end = translated_start + symbol.size() as usize;
            if addr >= translated_start && addr < translated_end {
                if let Ok(r) = symbol.get_name(&file) {
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
