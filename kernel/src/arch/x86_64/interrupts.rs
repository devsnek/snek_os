use super::gdt;
use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::instructions::port::Port;
use x86_64::registers::control::Cr2;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

const PIC_1_OFFSET: u8 = 32;
const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum InterruptID {
    Timer = PIC_1_OFFSET,
    Keyboard,
}

impl Into<u8> for InterruptID {
    fn into(self) -> u8 {
        self as u8
    }
}

impl Into<usize> for InterruptID {
    fn into(self) -> usize {
        usize::from(Into::<u8>::into(self))
    }
}

static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);

        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
            idt.page_fault
                .set_handler_fn(page_fault_handler)
                .set_stack_index(gdt::PAGE_FAULT_IST_INDEX);
        }

        idt.invalid_tss.set_handler_fn(invalid_tss_handler);
        idt.segment_not_present.set_handler_fn(segment_not_present_handler);
        // idt.stack_segment_fault.set_handler_fn(stack_segment_fault_handler);
        idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
        idt.alignment_check.set_handler_fn(alignment_check_handler);

        idt[InterruptID::Timer.into()].set_handler_fn(timer_interrupt_handler);
        idt[InterruptID::Keyboard.into()].set_handler_fn(keyboard_interrupt_handler);

        idt
    };
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn invalid_tss_handler(stack_frame: InterruptStackFrame, _error_code: u64) {
    panic!("INVALID TSS {:#?}", stack_frame);
}

extern "x86-interrupt" fn segment_not_present_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) {
    panic!("SEGMENT NOT PRESENT {:#?}", stack_frame);
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    println!(
        "GENERAL PROTECTION FAULT: {} {:#?}",
        error_code, stack_frame
    );
    crate::arch::halt_loop();
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    let address = Cr2::read();
    let protv = error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION);
    let write = error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE);
    let user = error_code.contains(PageFaultErrorCode::USER_MODE);
    let malformed = error_code.contains(PageFaultErrorCode::MALFORMED_TABLE);
    let ins = error_code.contains(PageFaultErrorCode::INSTRUCTION_FETCH);
    println!(
        "PAGE FAULT ({}{}{}{}{}at 0x{:x?})\n{:#?}",
        if protv { "protection-violation " } else { "" },
        if write { "read-only " } else { "" },
        if user { "user-mode " } else { "" },
        if malformed { "reserved " } else { "" },
        if ins { "fetch " } else { "" },
        address.as_u64(),
        stack_frame
    );
    crate::arch::halt_loop();
}

extern "x86-interrupt" fn alignment_check_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) {
    panic!("ALIGNMENT CHECK {:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    println!("DOUBLE FAULT: {:#?}", stack_frame);
    crate::arch::halt_loop();
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptID::Timer.into())
    }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    crate::task::keyboard::add_scancode(scancode);
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptID::Keyboard.into())
    }
}

pub fn init() {
    println!("[INTERRUPTS] initializing");

    IDT.load();
    unsafe { PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();

    println!("[INTERRUPTS] initialized");
}
