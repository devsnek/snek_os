use super::{acpi::AcpiAllocator, gdt, pci::ConfigRegionAccess};
use crate::arch::PciDevice;
use acpi::{
    platform::interrupt::{Apic as ApicInfo, Polarity, TriggerMode},
    InterruptModel, PlatformInfo,
};
use bit_field::BitField;
use core::{
    cmp::Ordering,
    sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering as AtomicOrdering},
    time::Duration,
};
use pci_types::capability::{MsiCapability, MsixCapability, PciCapability};
use pic8259::ChainedPics;
use raw_cpuid::CpuId;
use x2apic::{
    ioapic::{IoApic, IrqFlags},
    lapic::{IpiAllShorthand, LocalApic, LocalApicBuilder, TimerDivide, TimerMode},
};
use x86_64::{
    registers::{
        control::Cr2,
        segmentation::{SegmentSelector, GS},
    },
    structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode},
    PhysAddr,
};

const PIC_1_OFFSET: u8 = 32;
const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

const IOAPIC_START: u8 = 32;

const NUM_VECTORS: usize = 256;
const LOCAL_APIC_TLB_FLUSH: usize = NUM_VECTORS - 4;
const LOCAL_APIC_ERROR: usize = NUM_VECTORS - 3;
const LOCAL_APIC_TIMER: usize = NUM_VECTORS - 2;
const LOCAL_APIC_SPURIOUS: usize = NUM_VECTORS - 1;

pub const TIMER_INTERVAL: Duration = Duration::from_millis(5);

static FIRST_FREE: AtomicU8 = AtomicU8::new(0);
const LAST_FREE: u8 = (LOCAL_APIC_TLB_FLUSH - 1) as u8;

static ALSO_HAS_LEGACY_PICS: AtomicBool = AtomicBool::new(false);

macro_rules! prologue {
    () => {
        unsafe { GS::swap() };
    };
}

macro_rules! epilogue {
    () => {
        unsafe {
            get_lapic().end_of_interrupt();

            GS::swap();
        }
    };
}

pub enum InterruptHandler {
    None,
    Static(fn()),
    Dynamic(Box<dyn FnMut()>),
}

static mut INTERRUPT_HANDLERS: [InterruptHandler; 224] = [const { InterruptHandler::None }; 224];

lazy_static::lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();

        idt.non_maskable_interrupt.set_handler_fn(non_maskable_handler);

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

        macro_rules! handler {
            ($i:expr) => {{
                extern "x86-interrupt" fn handle_interrupt(_stack_frame: InterruptStackFrame) {
                    prologue!();

                    unsafe {
                        match &mut INTERRUPT_HANDLERS[$i - 32] {
                            InterruptHandler::None => {
                                warn!("general interrupt 0x{:02x} with empty handler!", $i);
                            }
                            InterruptHandler::Static(f) => f(),
                            InterruptHandler::Dynamic(f) => f(),
                        }
                    }

                    epilogue!();
                }

                idt[$i].set_handler_fn(handle_interrupt);
            }}
        }

        handler!(32);
        handler!(33);
        handler!(34);
        handler!(35);
        handler!(36);
        handler!(37);
        handler!(38);
        handler!(39);
        handler!(40);
        handler!(41);
        handler!(42);
        handler!(43);
        handler!(44);
        handler!(45);
        handler!(46);
        handler!(47);
        handler!(48);
        handler!(49);
        handler!(50);
        handler!(51);
        handler!(52);
        handler!(53);
        handler!(54);
        handler!(55);
        handler!(56);
        handler!(57);
        handler!(58);
        handler!(59);
        handler!(60);
        handler!(61);
        handler!(62);
        handler!(63);
        handler!(64);
        handler!(65);
        handler!(66);
        handler!(67);
        handler!(68);
        handler!(69);
        handler!(70);
        handler!(71);
        handler!(72);
        handler!(73);
        handler!(74);
        handler!(75);
        handler!(76);
        handler!(77);
        handler!(78);
        handler!(79);
        handler!(80);
        handler!(81);
        handler!(82);
        handler!(83);
        handler!(84);
        handler!(85);
        handler!(86);
        handler!(87);
        handler!(88);
        handler!(89);
        handler!(90);
        handler!(91);
        handler!(92);
        handler!(93);
        handler!(94);
        handler!(95);
        handler!(96);
        handler!(97);
        handler!(98);
        handler!(99);
        handler!(100);
        handler!(101);
        handler!(102);
        handler!(103);
        handler!(104);
        handler!(105);
        handler!(106);
        handler!(107);
        handler!(108);
        handler!(109);
        handler!(110);
        handler!(111);
        handler!(112);
        handler!(113);
        handler!(114);
        handler!(115);
        handler!(116);
        handler!(117);
        handler!(118);
        handler!(119);
        handler!(120);
        handler!(121);
        handler!(122);
        handler!(123);
        handler!(124);
        handler!(125);
        handler!(126);
        handler!(127);
        handler!(128);
        handler!(129);
        handler!(130);
        handler!(131);
        handler!(132);
        handler!(133);
        handler!(134);
        handler!(135);
        handler!(136);
        handler!(137);
        handler!(138);
        handler!(139);
        handler!(140);
        handler!(141);
        handler!(142);
        handler!(143);
        handler!(144);
        handler!(145);
        handler!(146);
        handler!(147);
        handler!(148);
        handler!(149);
        handler!(150);
        handler!(151);
        handler!(152);
        handler!(153);
        handler!(154);
        handler!(155);
        handler!(156);
        handler!(157);
        handler!(158);
        handler!(159);
        handler!(160);
        handler!(161);
        handler!(162);
        handler!(163);
        handler!(164);
        handler!(165);
        handler!(166);
        handler!(167);
        handler!(168);
        handler!(169);
        handler!(170);
        handler!(171);
        handler!(172);
        handler!(173);
        handler!(174);
        handler!(175);
        handler!(176);
        handler!(177);
        handler!(178);
        handler!(179);
        handler!(180);
        handler!(181);
        handler!(182);
        handler!(183);
        handler!(184);
        handler!(185);
        handler!(186);
        handler!(187);
        handler!(188);
        handler!(189);
        handler!(190);
        handler!(191);
        handler!(192);
        handler!(193);
        handler!(194);
        handler!(195);
        handler!(196);
        handler!(197);
        handler!(198);
        handler!(199);
        handler!(200);
        handler!(201);
        handler!(202);
        handler!(203);
        handler!(204);
        handler!(205);
        handler!(206);
        handler!(207);
        handler!(208);
        handler!(209);
        handler!(210);
        handler!(211);
        handler!(212);
        handler!(213);
        handler!(214);
        handler!(215);
        handler!(216);
        handler!(217);
        handler!(218);
        handler!(219);
        handler!(220);
        handler!(221);
        handler!(222);
        handler!(223);
        handler!(224);
        handler!(225);
        handler!(226);
        handler!(227);
        handler!(228);
        handler!(229);
        handler!(230);
        handler!(231);
        handler!(232);
        handler!(233);
        handler!(234);
        handler!(235);
        handler!(236);
        handler!(237);
        handler!(238);
        handler!(239);
        handler!(240);
        handler!(241);
        handler!(242);
        handler!(243);
        handler!(244);
        handler!(245);
        handler!(246);
        handler!(247);
        handler!(248);
        handler!(249);
        handler!(250);
        handler!(251);
        handler!(252);
        handler!(253);
        handler!(254);
        handler!(255);

        idt[LOCAL_APIC_TLB_FLUSH].set_handler_fn(tlb_flush_interrupt_handler);
        idt[LOCAL_APIC_ERROR].set_handler_fn(error_interrupt_handler);
        idt[LOCAL_APIC_TIMER].set_handler_fn(apic_timer_interrupt_handler);
        idt[LOCAL_APIC_SPURIOUS].set_handler_fn(spurious_interrupt_handler);

        idt
    };
}

extern "x86-interrupt" fn non_maskable_handler(_stack_frame: InterruptStackFrame) {
    prologue!();

    epilogue!();
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    prologue!();

    error!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);

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

    if !super::memory::lazy_map(address) {
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

extern "x86-interrupt" fn tlb_flush_interrupt_handler(_stack_frame: InterruptStackFrame) {
    prologue!();

    x86_64::instructions::tlb::flush_all();

    epilogue!();
}

extern "x86-interrupt" fn error_interrupt_handler(stack_frame: InterruptStackFrame) {
    prologue!();

    error!("ERROR: {:#?}", stack_frame);

    epilogue!();
}

extern "x86-interrupt" fn apic_timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    prologue!();

    // timer interrupt wakes all cores but we don't want to tick more than once.
    if super::get_pid() == 0 {
        crate::task::timer::on_tick();
        super::time::on_tick();
    }

    epilogue!();
}

extern "x86-interrupt" fn spurious_interrupt_handler(_stack_frame: InterruptStackFrame) {
    prologue!();

    epilogue!();
}

static LAPIC_ADDRESS: AtomicUsize = AtomicUsize::new(0);

fn get_lapic() -> LocalApic {
    LocalApicBuilder::new()
        .timer_vector(LOCAL_APIC_TIMER)
        .error_vector(LOCAL_APIC_ERROR)
        .spurious_vector(LOCAL_APIC_SPURIOUS)
        .set_xapic_base(LAPIC_ADDRESS.load(AtomicOrdering::SeqCst) as _)
        .build()
        .unwrap()
}

fn disable_pics() {
    unsafe {
        // Disable PIC so it doesn't interfere with LAPIC/IOPICs
        let mut pics = ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET);
        pics.initialize();
        pics.disable();
    }
}

fn init_lapic() {
    let mut lapic = get_lapic();

    unsafe {
        lapic.enable();
        lapic.disable_timer();
    }

    debug!("[LAPIC] initialized");
}

#[derive(Debug)]
struct IoApicWithInfo {
    io_apic: IoApic,
    global_system_interrupt_base: u8,
}

static mut IO_APICS: [Option<IoApicWithInfo>; 32] = [const { None }; 32];

fn get_io_apic(gsi: u8) -> &'static mut IoApicWithInfo {
    for io_apic in unsafe { &mut *core::ptr::addr_of_mut!(IO_APICS) } {
        let Some(io_apic) = io_apic else {
            continue;
        };
        if gsi < io_apic.global_system_interrupt_base {
            continue;
        }
        let vector = gsi - io_apic.global_system_interrupt_base;
        if vector > unsafe { io_apic.io_apic.max_table_entry() } {
            continue;
        }
        return io_apic;
    }
    panic!("no io apic for {gsi}");
}

fn init_ioapic(apic_info: &ApicInfo<&AcpiAllocator>) {
    let mut first_free = 0;

    for (i, io_apic_info) in apic_info.io_apics.iter().enumerate() {
        let io_apic_virtual_address =
            super::memory::map_address(PhysAddr::new(io_apic_info.address as u64), 4096);

        let mut io_apic = unsafe { IoApic::new(io_apic_virtual_address.as_u64()) };

        unsafe {
            io_apic.init(IOAPIC_START + io_apic_info.global_system_interrupt_base as u8);
        }

        let max_table_entry = unsafe { io_apic.max_table_entry() };

        for i in 0..max_table_entry {
            let mut entry = unsafe { io_apic.table_entry(i) };
            entry.set_flags(entry.flags() | IrqFlags::MASKED);

            if i == 2 && io_apic_info.global_system_interrupt_base == 0 {
                entry.set_dest(255);
            }

            unsafe {
                io_apic.set_table_entry(i, entry);
            }
        }

        unsafe {
            IO_APICS[i] = Some(IoApicWithInfo {
                io_apic,
                global_system_interrupt_base: io_apic_info.global_system_interrupt_base as _,
            });
        }

        first_free = first_free.max(
            IOAPIC_START + (io_apic_info.global_system_interrupt_base as u8) + max_table_entry,
        );
    }

    unsafe {
        IO_APICS.sort_unstable_by(|a, b| match (a, b) {
            (None, None) => Ordering::Equal,
            (Some(..), None) => Ordering::Less,
            (None, Some(..)) => Ordering::Greater,
            (Some(ia), Some(ib)) => ia
                .global_system_interrupt_base
                .cmp(&ib.global_system_interrupt_base),
        });
    }

    FIRST_FREE.store(first_free, AtomicOrdering::Relaxed);

    debug!("[IOAPIC] initialized");
}

fn timer_frequency_hz() -> f64 {
    let cpuid = CpuId::new();

    if let Some(undivided_freq_khz) = cpuid
        .get_hypervisor_info()
        .and_then(|hypervisor| hypervisor.apic_frequency())
    {
        let frequency_hz = undivided_freq_khz as f64 / 1000. / 16.;
        if frequency_hz > 0.0 {
            return frequency_hz;
        }
    }

    if let Some(undivided_freq_hz) = cpuid.get_tsc_info().map(|tsc| tsc.nominal_frequency()) {
        let frequency_hz = undivided_freq_hz as f64 / 16.0;
        if frequency_hz > 0.0 {
            return frequency_hz;
        }
    }

    let mut lapic = get_lapic();
    let mut tick_counts = [0; 100];

    for ticks in &mut tick_counts {
        unsafe {
            lapic.set_timer_divide(TimerDivide::Div16);
            lapic.set_timer_initial(u32::MAX);
            lapic.set_timer_mode(TimerMode::OneShot);
            lapic.enable_timer();
        }

        super::pit::PIT.lock().sleep(TIMER_INTERVAL);

        unsafe {
            lapic.disable_timer();
        }

        *ticks = u32::MAX.wrapping_sub(unsafe { lapic.timer_current() });
    }

    fn average_without_some_outliers(data: &mut [u32]) -> f64 {
        data.sort_unstable();

        let q1 = data[data.len() / 4];
        let q3 = data[3 * data.len() / 4];

        let mut sum = 0;
        let mut count = 0;
        for item in data {
            if *item < q1 || *item > q3 {
                continue;
            }
            sum += *item;
            count += 1;
        }

        sum as f64 / count as f64
    }

    let average = average_without_some_outliers(&mut tick_counts);

    average * (1000.0 / (TIMER_INTERVAL.as_millis() as u64 as f64))
}

fn init_timing() {
    let timer_frequency_hz = timer_frequency_hz();
    let ticks_per_ms = timer_frequency_hz / 1000.0;
    let ticks_per_interval =
        libm::round((TIMER_INTERVAL.as_millis() as u64 as f64) * ticks_per_ms) as u32;

    let mut lapic = get_lapic();

    unsafe {
        lapic.set_timer_divide(TimerDivide::Div16);
        lapic.set_timer_initial(ticks_per_interval);
        lapic.set_timer_mode(TimerMode::Periodic);
        lapic.enable_timer();
    }

    debug!("[TIMING] initialized");
}

pub fn init(acpi_platform_info: &PlatformInfo<&AcpiAllocator>) {
    let InterruptModel::Apic(ref apic_info) = acpi_platform_info.interrupt_model else {
        panic!("unsupported interrupt model")
    };

    IDT.load();

    if apic_info.also_has_legacy_pics {
        ALSO_HAS_LEGACY_PICS.store(true, AtomicOrdering::Relaxed);
        disable_pics();
    }

    LAPIC_ADDRESS.store(apic_info.local_apic_address as _, AtomicOrdering::Relaxed);
    init_lapic();

    init_ioapic(apic_info);

    crate::task::timer::init();
    init_timing();

    x86_64::instructions::interrupts::enable();

    debug!("[INTERRUPTS] initialized");
}

pub fn init_smp() {
    IDT.load();

    if ALSO_HAS_LEGACY_PICS.load(AtomicOrdering::Relaxed) {
        disable_pics();
    }

    init_lapic();

    init_timing();

    x86_64::instructions::interrupts::enable();
}

#[must_use]
pub enum InterruptGuard {
    IoApic(u8),
    Msi(MsiCapability, u8),
    MsiX(MsixCapability, u8),
}

impl Drop for InterruptGuard {
    fn drop(&mut self) {
        let gsi = match *self {
            Self::IoApic(gsi) => {
                let io_apic = get_io_apic(gsi);
                let vector = gsi - io_apic.global_system_interrupt_base;

                unsafe {
                    io_apic.io_apic.disable_irq(vector);
                }

                gsi
            }
            Self::Msi(cap, gsi) => {
                let access = ConfigRegionAccess::default();
                cap.set_enabled(false, &access);
                gsi
            }
            Self::MsiX(mut cap, gsi) => {
                let access = ConfigRegionAccess::default();
                cap.set_enabled(false, &access);
                gsi
            }
        };

        unsafe {
            INTERRUPT_HANDLERS[gsi as usize] = InterruptHandler::None;
        }
    }
}

pub enum InterruptType {
    EdgeHigh,
    EdgeLow,
    LevelHigh,
    LevelLow,
}

pub fn set_interrupt_static(gsi: u8, itype: InterruptType, f: fn()) -> InterruptGuard {
    set_interrupt(gsi, itype, InterruptHandler::Static(f))
}

pub fn set_interrupt_dyn(gsi: u8, itype: InterruptType, f: Box<dyn FnMut()>) -> InterruptGuard {
    set_interrupt(gsi, itype, InterruptHandler::Dynamic(f))
}

pub fn set_interrupt_msi(header: PciDevice, f: Box<dyn FnMut()>) -> Option<InterruptGuard> {
    let first_free = FIRST_FREE.load(AtomicOrdering::Relaxed);
    let mut gsi = 0u8;
    for i in first_free..=LAST_FREE {
        if matches!(
            unsafe { &INTERRUPT_HANDLERS[(i - 32) as usize] },
            InterruptHandler::None
        ) {
            gsi = i;
            break;
        }
    }
    if gsi == 0 {
        return None;
    }

    let mut mapped = None;
    for cap in header.capabilities {
        match cap {
            PciCapability::Msi(cap) => {
                let access = ConfigRegionAccess::default();

                let mut mar = 0u64;
                mar.set_bit(2, false);
                mar.set_bit(3, false);
                mar.set_bits(12..20, 0);
                mar.set_bits(20..32, 0x0FEE);

                let mut mdr = 0u32;
                mdr.set_bits(0..8, gsi as _);

                cap.set_message_info(mar, mdr, &access);
                cap.set_enabled(true, &access);

                mapped = Some(InterruptGuard::Msi(cap, gsi));
                break;
            }
            PciCapability::MsiX(mut cap) => {
                let access = ConfigRegionAccess::default();

                let mut mar = 0u32;
                mar.set_bit(2, false);
                mar.set_bit(3, false);
                mar.set_bits(12..20, 0);
                mar.set_bits(20..32, 0x0FEE);

                let mut mdr = 0u32;
                mdr.set_bits(0..8, gsi as _);
                mdr.set_bits(8..11, 0b000);
                mdr.set_bit(14, false);
                mdr.set_bit(15, false);

                let bar = header.bars[cap.table_bar() as usize].unwrap();
                let (addr, size) = bar.unwrap_mem();
                let addr = super::map_address(PhysAddr::new(addr as _), size).as_mut_ptr::<u32>();
                let addr = unsafe { addr.byte_add(cap.table_offset() as _) };

                unsafe {
                    addr.byte_add(0x00).write_volatile(mar);
                    addr.byte_add(0x04).write_volatile(0);
                    addr.byte_add(0x08).write_volatile(mdr);
                    addr.byte_add(0x0C).write_volatile(0);
                }

                cap.set_enabled(true, &access);

                mapped = Some(InterruptGuard::MsiX(cap, gsi));
                break;
            }
            _ => {}
        }
    }

    if mapped.is_some() {
        unsafe {
            INTERRUPT_HANDLERS[(gsi - 32) as usize] = InterruptHandler::Dynamic(f);
        }
    }

    mapped
}

fn set_interrupt(mut gsi: u8, mut itype: InterruptType, h: InterruptHandler) -> InterruptGuard {
    let acpi_allocator = AcpiAllocator::new();

    let platform_info = super::acpi::get_platform_info(&acpi_allocator);
    let InterruptModel::Apic(ref apic_info) = platform_info.interrupt_model else {
        panic!("unsupported interrupt model")
    };

    for iso in apic_info.interrupt_source_overrides.iter() {
        if iso.isa_source == gsi {
            gsi = iso.global_system_interrupt as _;
            itype = match (iso.trigger_mode, iso.polarity) {
                (TriggerMode::Edge, Polarity::ActiveHigh) => InterruptType::EdgeHigh,
                (TriggerMode::Edge, Polarity::ActiveLow) => InterruptType::EdgeLow,
                (TriggerMode::Level, Polarity::ActiveHigh) => InterruptType::LevelHigh,
                (TriggerMode::Level, Polarity::ActiveLow) => InterruptType::LevelLow,
                _ => itype,
            };
            break;
        }
    }

    let io_apic = get_io_apic(gsi);
    let vector = gsi - io_apic.global_system_interrupt_base;
    let mut entry = unsafe { io_apic.io_apic.table_entry(vector) };
    let mut flags = entry.flags();
    match itype {
        InterruptType::EdgeHigh => {
            flags.remove(IrqFlags::LEVEL_TRIGGERED);
            flags.remove(IrqFlags::LOW_ACTIVE);
        }
        InterruptType::EdgeLow => {
            flags.remove(IrqFlags::LEVEL_TRIGGERED);
            flags.insert(IrqFlags::LOW_ACTIVE);
        }
        InterruptType::LevelHigh => {
            flags.insert(IrqFlags::LEVEL_TRIGGERED);
            flags.remove(IrqFlags::LOW_ACTIVE);
        }
        InterruptType::LevelLow => {
            flags.insert(IrqFlags::LEVEL_TRIGGERED);
            flags.insert(IrqFlags::LOW_ACTIVE);
        }
    };
    entry.set_flags(flags);

    unsafe {
        io_apic.io_apic.set_table_entry(vector, entry);

        INTERRUPT_HANDLERS[gsi as usize] = h;

        io_apic.io_apic.enable_irq(vector);
    }

    InterruptGuard::IoApic(gsi)
}

pub fn send_flush_tlb() {
    unsafe {
        get_lapic().send_ipi_all(LOCAL_APIC_TLB_FLUSH as _, IpiAllShorthand::AllExcludingSelf);
    }
}
