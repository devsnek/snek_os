mod acpi;
mod allocator;
mod framebuffer;
mod gdt;
mod hpet;
mod interrupts;
mod local;
mod memory;
mod pci;
mod pit;
mod stack_allocator;
mod time;

use limine::{FramebufferRequest, HhdmRequest, MemmapRequest, RsdpRequest, SmpInfo, SmpRequest};
use x86_64::{registers::model_specific::Msr, VirtAddr};

static FRAMEBUFFER: FramebufferRequest = FramebufferRequest::new(0);
static HHDM: HhdmRequest = HhdmRequest::new(0);
static MEMMAP: MemmapRequest = MemmapRequest::new(0);
static RSDP: RsdpRequest = RsdpRequest::new(0);
static SMP: SmpRequest = SmpRequest::new(0);

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    crate::panic::catch_unwind(start);
}

fn start() -> ! {
    e9::println!("hey there :)");

    init_sse();

    if let Some(framebuffer_response) = FRAMEBUFFER.get_response().get() {
        if framebuffer_response.framebuffer_count > 0 {
            let framebuffer = &framebuffer_response.framebuffers()[0];
            framebuffer::early_init(framebuffer);
        }
    }

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

    allocator::init();

    framebuffer::late_init();

    acpi::late_init();

    hpet::init();

    time::init();

    local::init();

    pci::init(physical_memory_offset);

    crate::main();
}

struct ApInfo {
    gdt: Option<gdt::ApInfo>,
    wake: u8,
}

static mut AP_INFO: Option<Vec<ApInfo>> = None;

pub fn init_smp() {
    let smp = SMP.get_response().get_mut().unwrap();

    // allocate on main thread because ap can't set up
    // lazy allocation interrupt until it sets up gdt.
    let infos = smp
        .cpus()
        .iter()
        .map(|_| ApInfo {
            gdt: Some(gdt::allocate_for_ap()),
            wake: 0,
        })
        .collect();
    unsafe {
        AP_INFO = Some(infos);
    }

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

    gdt::init_smp(unsafe {
        AP_INFO.as_mut().unwrap()[boot_info.processor_id as usize]
            .gdt
            .take()
            .unwrap()
    });

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

pub use acpi::pci_route_pin;
pub use acpi::shutdown;
pub use framebuffer::_print;
pub use interrupts::{
    set_interrupt_dyn, set_interrupt_static, InterruptGuard, InterruptType, TIMER_INTERVAL,
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
    if let Some(aps) = unsafe { AP_INFO.as_ref() } {
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

pub use x86_64::instructions::interrupts::disable as disable_interrupts;
pub use x86_64::instructions::interrupts::without_interrupts;
