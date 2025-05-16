use crate::local::Local;
use core::{
    any::Any,
    panic::PanicInfo,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use limine::{KernelAddressRequest, KernelFileRequest};
use rustc_demangle::demangle;
use unwinding::abi::{UnwindContext, UnwindReasonCode, _Unwind_Backtrace, _Unwind_GetIP};
use xmas_elf::{
    program::Type as ProgramType,
    sections::{SectionData, ShType},
    symbol_table::Entry,
    ElfFile,
};

static KERNEL_FILE: KernelFileRequest = KernelFileRequest::new(0);
static KERNEL_ADDRESS: KernelAddressRequest = KernelAddressRequest::new(0);
static PANIC_COUNT: Local<AtomicUsize> = Local::new(|| AtomicUsize::new(0));
static mut KERNEL_SLICE: &[u8] = &[];
static KERNEL_BASE: AtomicUsize = AtomicUsize::new(0);
static FULL: AtomicBool = AtomicBool::new(false);

pub fn init() {
    let kernel_file = KERNEL_FILE
        .get_response()
        .get()
        .unwrap()
        .kernel_file
        .get()
        .unwrap();

    let kernel_file_base = kernel_file.base.as_ptr().unwrap();

    unsafe {
        KERNEL_SLICE = core::slice::from_raw_parts(kernel_file_base as _, kernel_file.length as _);
    }

    let kernel_address = KERNEL_ADDRESS.get_response().get().unwrap();
    KERNEL_BASE.store(kernel_address.virtual_base as _, Ordering::Relaxed);

    debug!("[PANIC] init (kbase={:x})", kernel_address.virtual_base);
}

pub fn late_init() {
    FULL.store(true, Ordering::Relaxed);
}

struct PanicData {
    info: String,
    stack_frames: Vec<StackFrame>,
}

impl Drop for PanicData {
    fn drop(&mut self) {
        PANIC_COUNT.with(|c| {
            c.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    if !FULL.load(Ordering::Relaxed) {
        error!("PANIC DURING INIT {}", info);
        stack_trace(16, |frame| {
            print_stack_frames(&[frame]);
        });
        crate::arch::halt_loop();
    }

    if PANIC_COUNT.with(|c| c.fetch_add(1, Ordering::SeqCst) > 0) {
        error!("PANIC IN PANIC: {}", info);
        crate::arch::halt_loop();
    }

    let mut stack_frames = Vec::new();
    stack_trace(16, |frame| {
        stack_frames.push(frame);
    });

    if !info.can_unwind() {
        error!("UNHANDLED (no-unwind) {}", info);
        print_stack_frames(&stack_frames);
        crate::arch::halt_loop();
    }

    let code = unwinding::panic::begin_panic(Box::new(PanicData {
        info: format!("{}", info),
        stack_frames: stack_frames.clone(),
    }));
    error!(
        "UNHANDLED ({}) {}",
        match code {
            UnwindReasonCode::NO_REASON => "no reason",
            UnwindReasonCode::FOREIGN_EXCEPTION_CAUGHT => "foreign exception caught",
            UnwindReasonCode::FATAL_PHASE1_ERROR => "fatal phase 1 error",
            UnwindReasonCode::FATAL_PHASE2_ERROR => "fatal phase 2 error",
            UnwindReasonCode::NORMAL_STOP => "normal stop",
            UnwindReasonCode::END_OF_STACK => "end of stack",
            UnwindReasonCode::HANDLER_FOUND => "handler found",
            UnwindReasonCode::INSTALL_CONTEXT => "install context",
            UnwindReasonCode::CONTINUE_UNWIND => "continue unwind",
            _ => "??",
        },
        info
    );
    print_stack_frames(&stack_frames);
    crate::arch::halt_loop();
}

fn print_stack_frames(stack_frames: &[StackFrame]) {
    for frame in stack_frames {
        if let Some((f_addr, f_name)) = frame.symbol {
            error!(
                "  0x{:016x} {:#}+0x{:x}",
                frame.address,
                demangle(f_name),
                frame.address - f_addr
            );
        } else {
            error!("  0x{:016x}", frame.address);
        }
        error!(
            "      at {}:{}:{}",
            frame.file.as_deref().unwrap_or_default(),
            frame.line.unwrap_or_default(),
            frame.column.unwrap_or_default()
        );
    }
}

pub fn inspect(e: &Box<dyn Any + Send>) {
    if let Some(data) = e.downcast_ref::<PanicData>() {
        error!("{}", data.info);
        print_stack_frames(&data.stack_frames);
    } else if let Some(data) = e.downcast_ref::<&'static str>() {
        error!("{data}");
    } else if let Some(data) = e.downcast_ref::<String>() {
        error!("{data}");
    } else {
        error!("external panic, halting");
    }
}

pub fn catch_unwind<F, T>(f: F) -> T
where
    F: FnOnce() -> T,
{
    match unwinding::panic::catch_unwind(f) {
        Ok(v) => v,
        Err(e) => {
            inspect(&e);
            crate::arch::halt_loop();
        }
    }
}

#[derive(Debug, Clone)]
pub struct StackFrame {
    pub address: usize,
    pub symbol: Option<(usize, &'static str)>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

pub fn stack_trace<F>(limit: usize, mut f: F)
where
    F: FnMut(StackFrame),
{
    let Ok(file) = ElfFile::new(unsafe { KERNEL_SLICE }) else {
        return;
    };

    let sections = addr2line::gimli::DwarfSections::load(|id| {
        Ok::<_, !>(match file.find_section_by_name(id.name()) {
            Some(data) => data.raw_data(&file),
            None => &[],
        })
    })
    .unwrap();
    let dwarf = sections.borrow(|section| {
        addr2line::gimli::EndianSlice::new(section, addr2line::gimli::RunTimeEndian::Little)
    });

    let context = addr2line::Context::from_dwarf(dwarf).ok();

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
    let load_base = KERNEL_BASE.load(Ordering::SeqCst);
    let offset = load_base - link_base;

    #[derive(Default)]
    struct DebugInfo<'a> {
        symbol: Option<(usize, &'static str)>,
        file: Option<&'a str>,
        line: Option<u32>,
        column: Option<u32>,
    }

    let get_symbol = |addr: usize| -> DebugInfo {
        let mut info = DebugInfo::default();
        let mapped = addr.saturating_sub(offset);
        if mapped == 0 {
            return info;
        }
        if let Some(location) = context
            .as_ref()
            .and_then(|c| c.find_location(mapped as _).ok())
            .flatten()
        {
            info.file = location.file;
            info.line = location.line;
            info.column = location.column;
        }
        if let Some(table) = &symbols {
            for symbol in table.iter() {
                let start = symbol.value() as usize + offset;
                let end = start + symbol.size() as usize;
                if addr >= start && addr < end {
                    if let Ok(r) = symbol.get_name(&file) {
                        info.symbol = Some((start, r));
                    }
                    break;
                }
            }
        }
        info
    };

    struct Data<'a> {
        limit: usize,
        f: &'a mut dyn FnMut(StackFrame),
        get_symbol: &'a dyn Fn(usize) -> DebugInfo<'a>,
    }

    extern "C" fn callback(
        unwind_ctx: &UnwindContext<'_>,
        arg: *mut core::ffi::c_void,
    ) -> UnwindReasonCode {
        let data = unsafe { &mut *(arg as *mut Data) };
        let address = _Unwind_GetIP(unwind_ctx);
        let info = (data.get_symbol)(address);

        if data.limit > 0 {
            (data.f)(StackFrame {
                address,
                symbol: info.symbol,
                file: info.file.map(|s| s.to_owned()),
                line: info.line,
                column: info.column,
            });
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
