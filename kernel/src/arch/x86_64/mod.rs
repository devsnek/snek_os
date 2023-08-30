mod acpi;
mod allocator;
mod framebuffer;
mod gdt;
mod interrupts;
mod local;
mod memory;
mod pci;
mod pit;
mod stack_allocator;

use limine::{
    FramebufferRequest, HhdmRequest, KernelAddressRequest, KernelFileRequest, MemmapRequest,
    RsdpRequest, SmpInfo, SmpRequest,
};
use x86_64::VirtAddr;

static FRAMEBUFFER: FramebufferRequest = FramebufferRequest::new(0);
static HHDM: HhdmRequest = HhdmRequest::new(0);
static MEMMAP: MemmapRequest = MemmapRequest::new(0);
static KERNEL_FILE: KernelFileRequest = KernelFileRequest::new(0);
static KERNEL_ADDRESS: KernelAddressRequest = KernelAddressRequest::new(0);
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
            framebuffer::init(framebuffer);
        }
    }

    gdt::init();

    let memmap = MEMMAP.get_response().get_mut().unwrap().memmap_mut();
    let physical_memory_offset = HHDM.get_response().get().unwrap().offset;

    memory::init(physical_memory_offset, memmap);

    let kernel_file = KERNEL_FILE
        .get_response()
        .get()
        .unwrap()
        .kernel_file
        .get()
        .unwrap();

    let kernel_address = KERNEL_ADDRESS.get_response().get().unwrap();

    let kernel_file_base = kernel_file.base.as_ptr().unwrap();

    crate::panic::init(
        unsafe { core::slice::from_raw_parts(kernel_file_base as _, kernel_file.length as _) },
        VirtAddr::new(kernel_address.virtual_base),
    );

    {
        let acpi_allocator = acpi::AcpiAllocator::new();
        let rsdp_addr = RSDP.get_response().get().unwrap().address.as_ptr().unwrap() as u64;

        let acpi_platform_info = acpi::early_init(&acpi_allocator, VirtAddr::new(rsdp_addr));

        interrupts::init(&acpi_platform_info);
    }

    allocator::init();

    acpi::late_init();

    local::init();

    pci::init(physical_memory_offset);

    crate::main();
}

static mut BOOT_INFO: Option<Vec<Option<gdt::ApInfo>>> = None;

pub fn init_smp() {
    let smp = SMP.get_response().get_mut().unwrap();

    let mut infos = Vec::new();
    for _ in 0..smp.cpus().len() {
        // allocate on main thread because ap can't set up
        // lazy allocation interrupt until it sets up gdt.
        infos.push(Some(gdt::allocate_for_ap()));
    }
    unsafe {
        BOOT_INFO = Some(infos);
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

    gdt::init_smp(unsafe {
        BOOT_INFO.as_mut().unwrap()[boot_info.processor_id as usize]
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
        "
        );
    }
}

pub use acpi::pci_route_pin;
pub use acpi::shutdown;
pub use framebuffer::_print;
pub use local::Local;
pub use memory::translate_virt_addr;
pub use pci::{get_devices as get_pci_devices, PciDevice};

#[inline(always)]
pub fn halt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

#[inline(always)]
pub fn enable_interrupts_and_halt() {
    x86_64::instructions::interrupts::enable_and_hlt();
}

pub use x86_64::instructions::interrupts::disable as disable_interrupts;
pub use x86_64::instructions::interrupts::without_interrupts;
