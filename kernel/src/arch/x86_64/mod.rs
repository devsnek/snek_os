mod acpi;
mod gdt;
mod hpet;
mod interrupts;
mod local;
mod memory;
mod pci;
mod pit;
mod time;

use conquer_once::spin::OnceCell;
use limine::{HhdmRequest, MemmapRequest, RsdpRequest, SmpInfo, SmpRequest};
use x86_64::{registers::model_specific::Msr, VirtAddr};

static HHDM: HhdmRequest = HhdmRequest::new(0);
static MEMMAP: MemmapRequest = MemmapRequest::new(0);
static RSDP: RsdpRequest = RsdpRequest::new(0);
static SMP: SmpRequest = SmpRequest::new(0);

pub fn init() {
    init_sse();

    set_pid(0);

    gdt::init();

    let memmap = MEMMAP.get_response().get_mut().unwrap().memmap_mut();
    let physical_memory_offset = HHDM.get_response().get().unwrap().offset;

    memory::init(physical_memory_offset, memmap);

    crate::panic::init();

    {
        let acpi_allocator = acpi::AcpiAllocator::new();
        let rsdp_addr = RSDP.get_response().get().unwrap().address.as_ptr().unwrap() as u64;

        let acpi_platform_info = acpi::early_init(&acpi_allocator, VirtAddr::new(rsdp_addr));

        interrupts::init(&acpi_platform_info);
    }

    crate::allocator::init();

    crate::framebuffer::late_init();

    acpi::late_init();

    hpet::init();

    time::init();

    local::init();

    crate::panic::late_init();

    pci::init(physical_memory_offset);

    init_smp();
}

struct ApInfo {
    gdt: gdt::ApInfo,
    wake: u8,
}

static AP_INFO: OnceCell<Vec<ApInfo>> = OnceCell::uninit();

fn init_smp() {
    let smp = SMP.get_response().get_mut().unwrap();

    // allocate on main thread because ap can't set up
    // lazy allocation interrupt until it sets up gdt.
    let infos = smp
        .cpus()
        .iter()
        .map(|_| ApInfo {
            gdt: gdt::allocate_for_ap(),
            wake: 0,
        })
        .collect();
    AP_INFO.try_init_once(|| infos).unwrap();

    let bsp_lapic_id = smp.bsp_lapic_id;
    for cpu in smp.cpus().iter_mut() {
        if cpu.lapic_id == bsp_lapic_id {
            continue;
        }
        cpu.goto_address = ap_entry;
    }
}

extern "C" fn ap_entry(boot_info: *const SmpInfo) -> ! {
    crate::panic::catch_unwind(|| -> ! {
        let boot_info = unsafe { &*boot_info };
        start_smp(boot_info);
    });
}

fn start_smp(boot_info: &SmpInfo) -> ! {
    init_sse();

    set_pid(boot_info.processor_id as _);

    let ap_info = AP_INFO.get().unwrap();
    gdt::init_smp(&ap_info[boot_info.processor_id as usize].gdt);

    interrupts::init_smp();

    local::init();

    crate::ap_main(boot_info.processor_id as _);
}

fn init_sse() {
    // unwinding uses stmxcsr which will UD if this isn't enabled
    unsafe {
        asm!(
            "
            mov rax, cr0
            and ax, 0xFFFB		// clear coprocessor emulation CR0.EM
            or ax, 0x2			  // set coprocessor monitoring  CR0.MP
            mov cr0, rax
            mov rax, cr4
            or ax, 3 << 9		  // set CR4.OSFXSR and CR4.OSXMMEXCPT at the same time
            mov cr4, rax
        ",
            out("rax") _,
        );
    }
}

fn set_pid(pid: u64) {
    let mut msr = Msr::new(0xc0000103);
    unsafe {
        msr.write(pid);
    }
}

pub fn get_pid() -> u64 {
    let mut pid;
    unsafe {
        asm!("rdpid {}", out(reg) pid);
    }
    pid
}

pub use acpi::get_pci_config_regions;
pub use acpi::pci_route_pin;
pub use acpi::reboot;
pub use acpi::shutdown;
pub use interrupts::{
    set_interrupt_dyn, set_interrupt_msi, set_interrupt_static, InterruptGuard, InterruptType,
    TIMER_INTERVAL,
};
pub use local::GsLocalData as LocalData;
pub use memory::{map_address, translate_phys_addr, translate_virt_addr};
pub use pci::get_devices as get_pci_devices;
pub use time::{now, timestamp};

#[inline(always)]
pub fn halt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

#[inline(always)]
pub fn enable_interrupts_and_halt() {
    if let Ok(aps) = AP_INFO.try_get() {
        let pid = get_pid();
        let addr = (&aps[pid as usize].wake) as *const u8;

        unsafe {
            asm!("
            mov rcx, 0
            mov rdx, 0
            # rax set by `inout` below
            monitor

            mov rax, 0
            mov rcx, 0
            sti
            mwait
            ", in("rax") addr, out("rcx") _, out("rdx") _);
        }
    } else {
        unsafe {
            asm!(
                "
            sti
            hlt
            "
            );
        }
    }
}

pub fn print(args: core::fmt::Arguments) {
    if e9::detect() {
        e9::print(args);
    }
}
pub use x86_64::instructions::interrupts::without_interrupts;
