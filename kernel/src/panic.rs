use core::{any::Any, panic::PanicInfo};
use rustc_demangle::demangle;
use unwinding::abi::{UnwindContext, UnwindReasonCode, _Unwind_Backtrace, _Unwind_GetIP};
use x86_64::VirtAddr;
use xmas_elf::{
    program::Type as ProgramType,
    sections::{SectionData, ShType},
    symbol_table::Entry,
    ElfFile,
};

static mut IN_PANIC: bool = false;
static mut KERNEL_SLICE: &[u8] = &[];
static mut KERNEL_BASE: usize = 0;

pub fn init(kernel_slice: &'static [u8], kernel_base: VirtAddr) {
    unsafe {
        KERNEL_SLICE = kernel_slice;
        KERNEL_BASE = kernel_base.as_u64() as _;
    }
}

struct PanicData {
    info: String,
    stack_frames: Vec<StackFrame>,
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe {
        if IN_PANIC {
            println!("PANIC IN PANIC: {}", info);
            crate::arch::halt_loop();
        }
        IN_PANIC = true;
    }

    if !info.can_unwind() {
        println!("{}", info);
        println!("non-unwinding panic, halting");
        crate::arch::halt_loop();
    }

    let mut stack_frames = Vec::new();
    stack_trace(32, |frame| {
        stack_frames.push(frame);
    });

    let code = unwinding::panic::begin_panic(Box::new(PanicData {
        info: format!("{}", info),
        stack_frames,
    }));
    println!("failed to panic, error {}", code.0);
    crate::arch::halt_loop();
}

pub fn inspect(e: &(dyn Any + Send)) {
    if let Some(data) = e.downcast_ref::<PanicData>() {
        println!("{}", data.info);
        for frame in &data.stack_frames {
            if let Some((f_addr, f_name)) = frame.function {
                println!(
                    "  at 0x{:016x} {:#}+0x{:x}",
                    frame.address,
                    demangle(f_name),
                    frame.address - f_addr
                );
            } else {
                println!("  at 0x{:016x}", frame.address);
            }
        }
    } else {
        println!("external panic, halting");
    }
}

pub fn catch_unwind<F, T>(f: F) -> T
where
    F: FnOnce() -> T,
{
    match unwinding::panic::catch_unwind(f) {
        Ok(v) => v,
        Err(e) => {
            inspect(&*e);
            crate::arch::halt_loop();
        }
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
    let Ok(file) = ElfFile::new(unsafe { KERNEL_SLICE }) else {
        return;
    };

    let symbols = file.section_iter().find_map(|s| {
        if s.get_type() != Ok(ShType::SymTab) {
            return None;
        }
        let Ok(data) = s.get_data(&file) else {
            return None;
        };
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
            let start = symbol.value() as usize + offset;
            let end = start + symbol.size() as usize;
            if addr >= start && addr < end {
                if let Ok(r) = symbol.get_name(&file) {
                    return Some((start, r));
                }
            }
        }
        None
    };

    struct Data<'a> {
        limit: usize,
        f: &'a mut dyn FnMut(StackFrame),
        get_symbol: &'a dyn Fn(usize) -> Option<(usize, &'static str)>,
    }

    extern "C" fn callback(
        unwind_ctx: &UnwindContext<'_>,
        arg: *mut core::ffi::c_void,
    ) -> UnwindReasonCode {
        let data = unsafe { &mut *(arg as *mut Data) };
        let address = _Unwind_GetIP(unwind_ctx);
        let function = (data.get_symbol)(address);

        if data.limit > 0 {
            (data.f)(StackFrame { address, function });
            data.limit -= 1;
        }

        UnwindReasonCode::NO_REASON
    }

    let mut data = Data {
        limit,
        f: &mut f,
        get_symbol: &get_symbol,
    };

    _Unwind_Backtrace(callback, core::ptr::addr_of_mut!(data) as _);
}
