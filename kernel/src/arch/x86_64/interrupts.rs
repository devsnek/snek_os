use super::{acpi::AcpiAllocator, gdt};
use acpi::{platform::interrupt::Apic as ApicInfo, InterruptModel, PlatformInfo};
use pic8259::ChainedPics;
use raw_cpuid::CpuId;
use x2apic::{
    ioapic::{IoApic, IrqFlags, IrqMode, RedirectionTableEntry},
    lapic::{LocalApic, LocalApicBuilder, TimerDivide, TimerMode},
};
use x86_64::{
    registers::{
        control::Cr2,
        segmentation::{SegmentSelector, GS},
    },
    structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode},
    PhysAddr,
};

// mod table;

const PS2_KEYBOARD_IRQ: u8 = 1;
const PIT_TIMER_IRQ: u8 = 2;
const PS2_MOUSE_IRQ: u8 = 12;

const PIC_1_OFFSET: u8 = 32;
const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

const IOAPIC_START: u8 = PIC_2_OFFSET + 8;
const IOAPIC_PS2_KEYBOARD: usize = IOAPIC_START as usize + PS2_KEYBOARD_IRQ as usize;
const IOAPIC_PIT_TIMER: usize = IOAPIC_START as usize + PIT_TIMER_IRQ as usize;
const IOAPIC_PS2_MOUSE: usize = IOAPIC_START as usize + PS2_MOUSE_IRQ as usize;

const NUM_VECTORS: usize = 256;
const LOCAL_APIC_ERROR: usize = NUM_VECTORS - 3;
const LOCAL_APIC_TIMER: usize = NUM_VECTORS - 2;
const LOCAL_APIC_SPURIOUS: usize = NUM_VECTORS - 1;

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

        idt[IOAPIC_PS2_KEYBOARD].set_handler_fn(keyboard_interrupt_handler);
        idt[IOAPIC_PIT_TIMER].set_handler_fn(pit_timer_interrupt_handler);
        idt[IOAPIC_PS2_MOUSE].set_handler_fn(mouse_interrupt_handler);

        idt[LOCAL_APIC_ERROR].set_handler_fn(error_interrupt_handler);
        idt[LOCAL_APIC_TIMER].set_handler_fn(apic_timer_interrupt_handler);
        idt[LOCAL_APIC_SPURIOUS].set_handler_fn(spurious_interrupt_handler);

        idt
    };
}

macro_rules! prologue {
    () => {
        unsafe { GS::swap() };
    };
}

macro_rules! epilogue {
    () => {
        unsafe { GS::swap() }
    };
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    prologue!();

    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);

    epilogue!();
}

extern "x86-interrupt" fn invalid_tss_handler(stack_frame: InterruptStackFrame, _error_code: u64) {
    prologue!();

    panic!("INVALID TSS {:#?}", stack_frame);
}

extern "x86-interrupt" fn segment_not_present_handler(
    stack_frame: InterruptStackFrame,
    segment_selector: u64,
) {
    prologue!();

    panic!(
        "SEGMENT NOT PRESENT {:?} {:?}",
        SegmentSelector(segment_selector as _),
        stack_frame
    );
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    prologue!();

    panic!(
        "GENERAL PROTECTION FAULT: {} {:#?}",
        error_code, stack_frame
    );
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    prologue!();

    let address = Cr2::read();

    if !super::allocator::lazy_map(address) {
        let protv = error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION);
        let write = error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE);
        let user = error_code.contains(PageFaultErrorCode::USER_MODE);
        let malformed = error_code.contains(PageFaultErrorCode::MALFORMED_TABLE);
        let ins = error_code.contains(PageFaultErrorCode::INSTRUCTION_FETCH);
        panic!(
            "PAGE FAULT ({}{}{}{}{}at 0x{:x?})\n{:#?}",
            if protv { "protection-violation " } else { "" },
            if write { "read-only " } else { "" },
            if user { "user-mode " } else { "" },
            if malformed { "reserved " } else { "" },
            if ins { "fetch " } else { "" },
            address.as_u64(),
            stack_frame
        );
    }

    epilogue!();
}

extern "x86-interrupt" fn alignment_check_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) {
    prologue!();

    panic!("ALIGNMENT CHECK {:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    prologue!();

    panic!("DOUBLE FAULT: {:#?}", stack_frame);
}

extern "x86-interrupt" fn error_interrupt_handler(stack_frame: InterruptStackFrame) {
    prologue!();

    println!("ERROR: {:#?}", stack_frame);
    unsafe {
        end_of_interrupt();
    }

    epilogue!();
}

extern "x86-interrupt" fn apic_timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    prologue!();

    crate::task::timer::on_tick();
    unsafe {
        end_of_interrupt();
    }

    epilogue!();
}

extern "x86-interrupt" fn pit_timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    prologue!();

    super::pit::on_tick();
    unsafe {
        end_of_interrupt();
    }

    epilogue!();
}

extern "x86-interrupt" fn spurious_interrupt_handler(_stack_frame: InterruptStackFrame) {
    prologue!();

    unsafe {
        end_of_interrupt();
    }

    epilogue!();
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    prologue!();

    crate::drivers::i8042::interrupt(i8042::Irq::Irq1);

    unsafe {
        end_of_interrupt();
    }

    epilogue!();
}

extern "x86-interrupt" fn mouse_interrupt_handler(_stack_frame: InterruptStackFrame) {
    prologue!();

    crate::drivers::i8042::interrupt(i8042::Irq::Irq12);

    unsafe {
        end_of_interrupt();
    }

    epilogue!();
}

#[inline(always)]
unsafe fn end_of_interrupt() {
    get_lapic().end_of_interrupt();
}

static mut LAPIC_ADDRESS: usize = 0;

fn get_lapic() -> LocalApic {
    LocalApicBuilder::new()
        .timer_vector(LOCAL_APIC_TIMER)
        .error_vector(LOCAL_APIC_ERROR)
        .spurious_vector(LOCAL_APIC_SPURIOUS)
        .set_xapic_base(unsafe { LAPIC_ADDRESS } as _)
        .build()
        .unwrap()
}

fn init_lapic() {
    unsafe {
        // Disable PIC so it doesn't interfere with LAPIC/IOPICs
        let mut pics = ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET);
        pics.disable();
    }

    let mut lapic = get_lapic();

    unsafe {
        lapic.enable();
        lapic.disable_timer();
    }

    println!("[LAPIC] initialized");
}

fn init_ioapic(apic_info: &ApicInfo<AcpiAllocator>) {
    let io_apic_virtual_address =
        super::memory::map_address(PhysAddr::new(apic_info.io_apics[0].address as u64), 4096);

    let mut ioapic = unsafe { IoApic::new(io_apic_virtual_address.as_u64()) };

    unsafe {
        ioapic.init(IOAPIC_START);
    }

    for i in 0..16 {
        let mut entry = RedirectionTableEntry::default();
        entry.set_mode(IrqMode::Fixed);
        entry.set_flags(IrqFlags::LEVEL_TRIGGERED | IrqFlags::LOW_ACTIVE | IrqFlags::MASKED);
        entry.set_vector(IOAPIC_START + i);
        let dest = if i == PS2_MOUSE_IRQ || i == PS2_KEYBOARD_IRQ {
            0
        } else {
            LOCAL_APIC_SPURIOUS
        };
        entry.set_dest(dest as u8);
        unsafe {
            ioapic.set_table_entry(i, entry);
        }
    }

    unsafe {
        ioapic.enable_irq(PS2_MOUSE_IRQ);
        ioapic.enable_irq(PIT_TIMER_IRQ);
        ioapic.enable_irq(PS2_KEYBOARD_IRQ);
    }

    println!("[IOAPIC] initialized");
}

fn timer_frequency_hz() -> u32 {
    let cpuid = CpuId::new();

    if let Some(undivided_freq_khz) = cpuid
        .get_hypervisor_info()
        .and_then(|hypervisor| hypervisor.apic_frequency())
    {
        let frequency_hz = undivided_freq_khz / 1000 / 16;
        return frequency_hz;
    }

    if let Some(undivided_freq_hz) = cpuid.get_tsc_info().map(|tsc| tsc.nominal_frequency()) {
        let frequency_hz = undivided_freq_hz / 16;
        return frequency_hz;
    }

    let mut lapic = get_lapic();

    unsafe {
        lapic.set_timer_divide(TimerDivide::Div16);
        lapic.set_timer_initial(-1i32 as u32);
        lapic.set_timer_mode(TimerMode::OneShot);
        lapic.enable_timer();
    }

    super::pit::PIT
        .lock()
        .sleep(crate::task::timer::TIMER_INTERVAL);

    unsafe {
        lapic.disable_timer();
    }

    let elapsed_ticks = unsafe { lapic.timer_current() };
    let ticks_per_10ms = (-1i32 as u32).wrapping_sub(elapsed_ticks);
    ticks_per_10ms * 100
}

fn init_timing() {
    let interval = core::time::Duration::from_millis(10);
    let timer_frequency_hz = timer_frequency_hz();
    let ticks_per_ms = timer_frequency_hz / 1000;

    let interval_ms = interval.as_millis() as u32;
    let ticks_per_interval = interval_ms.checked_mul(ticks_per_ms).unwrap();

    let mut lapic = get_lapic();

    unsafe {
        lapic.set_timer_divide(TimerDivide::Div16);
        lapic.set_timer_initial(ticks_per_interval);
        lapic.set_timer_mode(TimerMode::Periodic);
        lapic.enable_timer();
    }

    println!("[TIMING] initialized");
}

pub fn init(acpi_platform_info: &PlatformInfo<AcpiAllocator>) {
    IDT.load();

    let InterruptModel::Apic(ref apic_info) = acpi_platform_info.interrupt_model else {
        panic!("unsupported interrupt model")
    };

    unsafe {
        LAPIC_ADDRESS =
            super::memory::map_address(PhysAddr::new(apic_info.local_apic_address), 4096).as_u64()
                as _;
    }
    init_lapic();

    init_ioapic(apic_info);

    crate::task::timer::init();
    init_timing();

    x86_64::instructions::interrupts::enable();

    println!("[INTERRUPTS] initialized");
}

pub fn init_smp() {
    IDT.load();
    init_lapic();
    init_timing();
    x86_64::instructions::interrupts::enable();
}
