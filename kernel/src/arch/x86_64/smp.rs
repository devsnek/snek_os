use super::interrupts::LAPIC;
use acpi::{
    platform::{Processor, ProcessorState},
    PlatformInfo,
};
use alloc::alloc::Layout;
use core::{convert::TryInto, time::Duration};
use futures::channel::oneshot;
use maitake::time::sleep;
use x86_64::{
    structures::paging::{page::PageRange, Mapper, Page, Size4KiB},
    PhysAddr, VirtAddr,
};

const AP_ENTRY_ADDRESS: usize = 0x4000;

fn start_application_processor(processor: &Processor, bootstrap_code_buf_ptr: *mut u8) {
    let processor_id = processor.processor_uid;
    println!("[SMP] starting processor {}", processor_id);

    unsafe {
        LAPIC
            .lock()
            .as_mut()
            .unwrap()
            .send_init_ipi(processor.local_apic_id);
    }

    let wait_after_init = sleep(Duration::from_millis(10));

    unsafe {
        core::ptr::copy_nonoverlapping(
            pointers::ap_entry as *mut u8,
            bootstrap_code_buf_ptr,
            pointers::ap_end as usize - pointers::ap_entry as usize,
        );
    }

    let (boot_code, mut init_finished_future) = {
        let (tx, rx) = oneshot::channel();
        let boot_code = move || -> ! {
            let _ = tx.send(());
            crate::ap_main(crate::ProcessorInfo { id: processor_id });
        };
        (boot_code, rx)
    };

    let ap_after_boot_param = {
        let boxed = Box::new(Box::new(boot_code) as Box<_>);
        let param_value: ApAfterBootParam = Box::into_raw(boxed);
        param_value as usize
    };

    let mut stack_top = unsafe {
        let stack_size = 10 * 1024 * 1024usize;
        let layout = Layout::from_size_align(stack_size, 0x1000).unwrap();
        let ptr = alloc::alloc::alloc(layout);
        assert!(!ptr.is_null());
        (ptr as usize + stack_size) as *mut usize
    };

    unsafe {
        stack_top.write(ap_after_boot_param);
        stack_top = stack_top.sub(1);
        stack_top.write(ap_after_boot as usize);
        stack_top = stack_top.sub(1);
    }

    unsafe {
        let pml4_offset = pointers::pml4 as usize - pointers::ap_entry as usize;
        let long_mode_jump_offset = pointers::long_mode_jump as usize - pointers::ap_entry as usize;
        let long_mode_offset = pointers::long_mode as usize - pointers::ap_entry as usize;
        let gdt_offset = pointers::gdt as usize - pointers::ap_entry as usize;

        // set page table
        let pml4_ptr = bootstrap_code_buf_ptr.add(pml4_offset);
        assert_eq!(
            core::slice::from_raw_parts(pml4_ptr as *const u8, 6),
            &[0x66, 0xba, 0x78, 0x56, 0x34, 0x12]
        );
        let pml4_ptr = pml4_ptr.add(2) as *mut u32;
        assert_eq!(pml4_ptr.read_unaligned(), 0x12345678);
        pml4_ptr.write_unaligned(
            x86_64::registers::control::Cr3::read()
                .0
                .start_address()
                .as_u64()
                .try_into()
                .unwrap(),
        );

        // set gdt descriptor
        let gdt_limit_ptr = bootstrap_code_buf_ptr.add(gdt_offset) as *mut u16;
        let gdt_base_ptr = bootstrap_code_buf_ptr.add(gdt_offset + 2) as *mut u64;
        assert_eq!(gdt_limit_ptr.read(), 15);
        assert_eq!(gdt_base_ptr.read(), 0xFFFF_0000_FFFF_0000);
        let desc = x86_64::instructions::tables::sgdt();
        gdt_limit_ptr.write(desc.limit);
        gdt_base_ptr.write(desc.base.as_u64());

        // set long mode jump location
        let long_mode_jump_ptr = bootstrap_code_buf_ptr.add(long_mode_jump_offset);
        assert_eq!(
            core::slice::from_raw_parts(long_mode_jump_ptr as *const u8, 7),
            &[0x66, 0xea, 0x78, 0x56, 0x34, 0x12, 0x08]
        );
        let long_mode_jump_ptr = long_mode_jump_ptr.add(2) as *mut u32;
        assert_eq!(long_mode_jump_ptr.read_unaligned(), 0x12345678);
        long_mode_jump_ptr.write_unaligned(
            (bootstrap_code_buf_ptr.add(long_mode_offset) as usize)
                .try_into()
                .unwrap(),
        );

        // set stack pointer
        let long_mode_ptr = bootstrap_code_buf_ptr.add(long_mode_offset);
        assert_eq!(
            core::slice::from_raw_parts(long_mode_ptr as *const u8, 10),
            &[0x48, 0xbc, 0x78, 0x56, 0x34, 0x12, 0x78, 0x56, 0x34, 0x12]
        );
        let long_mode_ptr = long_mode_ptr.add(2) as *mut u64;
        assert_eq!(long_mode_ptr.read_unaligned(), 0x1234567812345678);
        long_mode_ptr.write_unaligned(stack_top as u64);
    }

    crate::task::spawn_blocking(wait_after_init);

    let mut attempts = 0;
    loop {
        if attempts >= 2 {
            let param = ap_after_boot_param as ApAfterBootParam;
            let _ = unsafe { Box::from_raw(param) };
            println!("[SMP] failed to initialize processor {processor_id}");
            break;
        }
        attempts += 1;

        let boot_fn = AP_ENTRY_ADDRESS;
        assert_eq!((boot_fn >> 12) << 12, boot_fn);
        assert!((boot_fn >> 12) <= core::u8::MAX as usize);
        unsafe {
            LAPIC
                .lock()
                .as_mut()
                .unwrap()
                .send_sipi((boot_fn >> 12) as u8, processor.local_apic_id);
        }

        let ap_ready_timeout = sleep(Duration::from_secs(1));
        futures::pin_mut!(ap_ready_timeout);
        match crate::task::spawn_blocking(futures::future::select(
            ap_ready_timeout,
            &mut init_finished_future,
        )) {
            futures::future::Either::Left(_) => continue,
            futures::future::Either::Right(_) => {
                break;
            }
        }
    }
}

pub fn init(acpi_platform_info: &PlatformInfo) {
    assert!((pointers::ap_end as usize) - (pointers::ap_entry as usize) < 0x1000);

    let bootstrap_code_buf_ptr = super::memory::map_pages_from(
        PhysAddr::new(AP_ENTRY_ADDRESS as u64),
        0x1000,
        PageRange {
            start: Page::from_start_address(VirtAddr::new(AP_ENTRY_ADDRESS as u64)).unwrap(),
            end: Page::containing_address(VirtAddr::new(AP_ENTRY_ADDRESS as u64 + 0x2000)),
        },
    );

    for processor in &acpi_platform_info
        .processor_info
        .as_ref()
        .unwrap()
        .application_processors
    {
        if processor.state != ProcessorState::WaitingForSipi {
            continue;
        }
        start_application_processor(processor, bootstrap_code_buf_ptr.as_mut_ptr());
    }

    {
        let mut mapper = super::memory::MAPPER.lock();
        let mapper = mapper.as_mut().unwrap();
        let page = Page::<Size4KiB>::containing_address(bootstrap_code_buf_ptr);
        let (_frame, flusher) = mapper.unmap(page).unwrap();
        flusher.flush();
    }

    println!("[SMP] initialized");
}

global_asm!(
    r#"
.align 0x1000

.code16
ap_entry:
    // When we enter here, the CS register is set to the value that we passed through the
    // SIPI, and the IP register is set to `0`.
    movw %cs, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %fs
    movw %ax, %gs
    movw %ax, %ss

    movl $0, %eax
    or $(1 << 10), %eax             // Set SIMD floating point exceptions bit.
    or $(1 << 9), %eax              // Set OSFXSR bit, which enables SIMD.
    or $(1 << 5), %eax              // Set physical address extension (PAE) bit.
    movl %eax, %cr4

pml4:
    mov $0x12345678, %edx
    mov %edx, %cr3

    // Enable the EFER.LMA bit, which enables compatibility mode and will make us switch
    // to long mode when we update the CS register.
    mov $0xc0000080, %ecx
    rdmsr
    or $(1 << 8), %eax
    wrmsr

    // Set the appropriate CR0 flags: Paging, Extension Type (math co-processor), and
    // Protected Mode.
    movl $((1 << 31) | (1 << 4) | (1 << 0)), %eax
    movl %eax, %cr0

    // Set up the GDT.
    lgdtl ((gdt - ap_entry) + 0x4000)

long_mode_jump:
    // A long jump is necessary in order to update the CS registry and properly switch to
    // long mode.
    ljmpl $8, $0x12345678

.code64
long_mode:
    // Set up the stack.
    // The stack contains the ap_after_boot and ap_after_boot_param already,
    // so we need to pop them after.
    movq $0x1234567812345678, %rsp

    // This is the parameter that we pass to `ap_after_boot`
    popq %rax

    movw $0, %bx
    movw %bx, %ds
    movw %bx, %es
    movw %bx, %fs
    movw %bx, %gs
    movw %bx, %ss

    // In the x86-64 calling convention, the RDI register is used to store the value of
    // the first parameter to pass to a function.
    movq %rax, %rdi

    // We do an indirect call in order to force the assembler to use the absolute address
    // rather than a relative call.
    popq %rdx
    call *%rdx

    cli
    hlt

.align 8
gdt:
    .short 15
    .long 0xFFFF0000, 0xFFFF0000

ap_end:
"#,
    options(att_syntax)
);

mod pointers {
    extern "C" {
        pub fn ap_entry();
        pub fn pml4();
        pub fn long_mode_jump();
        pub fn long_mode();
        pub fn gdt();
        pub fn ap_end();
    }
}

type ApAfterBootParam = *mut Box<dyn FnOnce() -> ! + Send + 'static>;

extern "C" fn ap_after_boot(to_exec: usize) -> ! {
    // local_apics.init_local();
    // interrupts::load_idt();
    unsafe {
        let to_exec = to_exec as ApAfterBootParam;
        let to_exec = Box::from_raw(to_exec);
        (*to_exec)();
    }
}
