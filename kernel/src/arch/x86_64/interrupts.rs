use super::gdt;
use acpi::{
    platform::interrupt::{Apic as ApicInfo, InterruptModel},
    AcpiHandler, AcpiTables, PhysicalMapping,
};
use pic8259::ChainedPics;
use spin::Mutex;
use x2apic::{
    ioapic::{IoApic, IrqFlags, IrqMode, RedirectionTableEntry},
    lapic::{LocalApic, LocalApicBuilder},
};
use x86_64::{
    addr::{PhysAddr, VirtAddr},
    instructions::port::Port,
    registers::control::Cr2,
    structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode},
};

const PS2_KEYBOARD_IRQ: u8 = 1;
const PIT_TIMER_IRQ: u8 = 2;

const PIC_1_OFFSET: u8 = 32;
const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

const IOAPIC_START: u8 = PIC_2_OFFSET + 8;
const IOAPIC_PS2_KEYBOARD: usize = IOAPIC_START as usize + PS2_KEYBOARD_IRQ as usize;
const IOAPIC_PIT_TIMER: usize = IOAPIC_START as usize + PIT_TIMER_IRQ as usize;

const NUM_VECTORS: usize = 256;
const LOCAL_APIC_ERROR: usize = NUM_VECTORS - 3;
const LOCAL_APIC_TIMER: usize = NUM_VECTORS - 2;
const LOCAL_APIC_SPURIOUS: usize = NUM_VECTORS - 1;

static LAPIC: Mutex<Option<LocalApic>> = Mutex::new(None);

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

        idt[LOCAL_APIC_ERROR].set_handler_fn(error_interrupt_handler);
        idt[LOCAL_APIC_TIMER].set_handler_fn(timer_interrupt_handler);
        idt[LOCAL_APIC_SPURIOUS].set_handler_fn(spurious_interrupt_handler);

        idt[IOAPIC_PS2_KEYBOARD].set_handler_fn(keyboard_interrupt_handler);
        idt[IOAPIC_PIT_TIMER].set_handler_fn(timer_interrupt_handler);

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
    println!("SEGMENT NOT PRESENT {:#?}", stack_frame);
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

extern "x86-interrupt" fn error_interrupt_handler(stack_frame: InterruptStackFrame) {
    println!("ERROR: {:#?}", stack_frame);
    unsafe {
        LAPIC.lock().as_mut().unwrap().end_of_interrupt();
    }
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    // println!("TIMER");
    unsafe {
        LAPIC.lock().as_mut().unwrap().end_of_interrupt();
    }
}

extern "x86-interrupt" fn spurious_interrupt_handler(_stack_frame: InterruptStackFrame) {
    println!("SPURIOUS");
    unsafe {
        LAPIC.lock().as_mut().unwrap().end_of_interrupt();
    }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    crate::task::keyboard::add_scancode(scancode);
    unsafe {
        LAPIC.lock().as_mut().unwrap().end_of_interrupt();
    }
}

#[derive(Clone)]
struct AcpiHandlerImpl;

impl AcpiHandler for AcpiHandlerImpl {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        let virtual_address =
            super::memory::map_address(PhysAddr::new(physical_address as u64), size);

        PhysicalMapping::new(
            physical_address,
            core::ptr::NonNull::new(virtual_address.as_mut_ptr()).unwrap(),
            size,
            size,
            self.clone(),
        )
    }

    fn unmap_physical_region<T>(region: &PhysicalMapping<Self, T>) {
        use os_units::Bytes;
        use x86_64::structures::paging::Mapper;
        use x86_64::structures::paging::{Page, PageSize, Size4KiB};

        let start = VirtAddr::new(region.virtual_start().as_ptr() as u64);
        let object_size = Bytes::new(region.region_length());

        let start_frame_addr = start.align_down(Size4KiB::SIZE);
        let end_frame_addr = (start + object_size.as_usize()).align_down(Size4KiB::SIZE);

        let num_pages =
            Bytes::new((end_frame_addr - start_frame_addr) as usize).as_num_of_pages::<Size4KiB>();

        let mut binding1 = super::memory::MAPPER.lock();
        let mapper = binding1.as_mut().unwrap();

        for i in 0..num_pages.as_usize() {
            let page =
                Page::<Size4KiB>::containing_address(start_frame_addr + Size4KiB::SIZE * i as u64);

            let (_frame, flusher) = mapper.unmap(page).unwrap();
            flusher.flush();
        }
    }
}

fn init_lapic(apic_info: &ApicInfo) {
    println!("[LAPIC] initializing");

    unsafe {
        // We don't actually use the PIC, we just need to disable it so it doesn't interfere with
        // the APIC/IOAPICs
        let mut pics = ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET);
        pics.disable();
    }

    let apic_virtual_address = VirtAddr::new(apic_info.local_apic_address);

    let mut lapic = LocalApicBuilder::new()
        .timer_vector(LOCAL_APIC_TIMER)
        .error_vector(LOCAL_APIC_ERROR)
        .spurious_vector(LOCAL_APIC_SPURIOUS)
        .set_xapic_base(apic_virtual_address.as_u64())
        .build()
        .unwrap();

    unsafe {
        lapic.enable();
    }

    let _ = LAPIC.lock().insert(lapic);

    println!("[LAPIC] initialized");
}

fn init_ioapic(apic_info: &ApicInfo) {
    println!("[IOAPIC] initializing");

    let io_apic_virtual_address =
        super::memory::map_address(PhysAddr::new(apic_info.io_apics[0].address as u64), 4096);

    // let io_apic_virtual_address = VirtAddr::new(apic_info.io_apics[0].address as u64);
    let mut ioapic = unsafe { IoApic::new(io_apic_virtual_address.as_u64()) };

    unsafe {
        ioapic.init(IOAPIC_START);
    }

    for i in 0..16 {
        let mut entry = RedirectionTableEntry::default();
        entry.set_mode(IrqMode::Fixed);
        entry.set_flags(IrqFlags::LEVEL_TRIGGERED | IrqFlags::LOW_ACTIVE | IrqFlags::MASKED);
        entry.set_dest(LOCAL_APIC_SPURIOUS as u8);
        entry.set_vector(IOAPIC_START + i);
        unsafe {
            ioapic.set_table_entry(i, entry);
        }
    }

    unsafe {
        ioapic.enable_irq(PIT_TIMER_IRQ);
        ioapic.enable_irq(PS2_KEYBOARD_IRQ);
    }

    println!("[IOAPIC] initialized");
}

pub fn init(rsdp_address: Option<u64>) {
    println!("[INTERRUPTS] initializing");

    IDT.load();

    let rsdp_address = rsdp_address.expect("RSDP address missing");
    let acpi_tables =
        unsafe { AcpiTables::from_rsdp(AcpiHandlerImpl, rsdp_address as usize) }.unwrap();

    let acpi_platform_info = acpi_tables.platform_info().unwrap();
    let InterruptModel::Apic(apic_info) = acpi_platform_info.interrupt_model else { panic!("unsupported interrupt model") };

    init_lapic(&apic_info);

    init_ioapic(&apic_info);

    x86_64::instructions::interrupts::enable();

    println!("[INTERRUPTS] initialized");
}
