use super::interrupts::LAPIC;
use acpi::{platform::Processor, PlatformInfo};
use alloc::alloc::Layout;
use core::{convert::TryInto, time::Duration};
use futures::channel::oneshot;
use maitake::time::sleep;

fn start_application_processor(processor: &Processor) {
    let processor_id = processor.processor_uid;
    println!("[SMP] starting processor {}", processor_id);

    // In order to boot an associated processor, we must send to it an inter-processor interrupt
    // (IPI) containing the offset of a 4kiB memory page containing the code that it must start
    // executing. The CS register of the target processor will take the value that we send, the
    // IP register will be set to 0, and the processor will start running in 16 bits mode.
    //
    // Since this is 16 bits mode, the processor cannot execute any code (or access any data)
    // above one megabyte of physical memory. Most, if not all, of the kernel is loaded above
    // that limit, and therefore we cannot simply ask the processor to start executing a certain
    // function as we would like to.
    //
    // Instead, what we must do is allocate a buffer below that one megabyte limit, write some
    // x86 machine code in that buffer, and then we can ask the processor to run it. This is
    // implemented by copying a template code into that buffer and tweaking the constants.

    // Get information about the template.
    let code_template = get_template();
    assert!(code_template.marker1_offset < code_template.code.len());
    assert!(code_template.marker2_offset < code_template.code.len());
    assert!(code_template.marker3_offset < code_template.code.len());

    // We start by allocating the buffer where to write the bootstrap code.
    let mut bootstrap_code_buf = unsafe {
        let size = code_template.code.len();
        // Basic sanity check to make sure that nothing's fundamentally wrong.
        assert!(size <= 0x1000);
        let layout = Layout::from_size_align(size, 0x1000).unwrap();
        let mem = alloc::alloc::alloc(layout).cast::<u8>();
        Vec::<u8>::from_raw_parts(mem, 0x1000, 0x1000)
    };

    // Start by sending an INIT IPI to the target so that it reboots.
    // local_apics.send_interprocessor_init(target);

    // Later we will wait for 10ms to have elapsed since the INIT.
    let wait_after_init = sleep(Duration::from_millis(10));

    unsafe {
        // Write the template code to the buffer.
        core::ptr::copy_nonoverlapping(
            code_template.code.as_ptr(),
            bootstrap_code_buf.as_mut_ptr(),
            bootstrap_code_buf.len(),
        );
    }

    // Later, we will want to wait until the AP has finished initializing. To do so, we create
    // a channel and modify `boot_code` to signal that channel before doing anything more.
    let (boot_code, mut init_finished_future) = {
        let (tx, rx) = oneshot::channel();
        let boot_code = move || -> ! {
            // local_apics.init_local();
            // interrupts::load_idt();
            let _ = tx.send(());
            crate::ap_main(crate::ProcessorInfo { id: processor_id });
        };
        (boot_code, rx)
    };

    // We want the processor we bootstrap to call the `ap_after_boot` function defined below.
    // `ap_after_boot` will cast its first parameter into a `Box<Box<dyn FnOnce()>>` and call it.
    // We therefore cast `boot_code` into the proper format, then leak it with the intent to pass
    // this value to `ap_after_boot`, which will then "unleak" it and call it.
    let ap_after_boot_param = {
        let boxed = Box::new(Box::new(boot_code) as Box<_>);
        let param_value: ApAfterBootParam = Box::into_raw(boxed);
        param_value as usize
    };

    // Allocate a stack for the processor. This is the one and unique stack that will be used for
    // everything by this processor.
    let stack_size = 10 * 1024 * 1024usize;
    let stack_top = unsafe {
        let layout = Layout::from_size_align(stack_size, 0x1000).unwrap();
        let ptr = alloc::alloc::alloc(layout);
        assert!(!ptr.is_null());
        ptr as usize + stack_size
    };

    // There exists several placeholders within the template code that we must adjust before it
    // can be executed.
    //
    // The code at marker 1 starts with the following instruction:
    //
    // ```
    // 66 ea ad de ad de 08    ljmpl  $0x8, $0xdeaddead
    // ```
    //
    // The code at marker 3 starts with the following instruction:
    //
    // ```
    // 66 ba dd ba 00 ff    mov $0xff00badd, %edx
    // ```
    //
    // The code at marker 2 starts with the following instructions:
    //
    // ```
    // 48 bc ef cd ab 90 78 56 34 12    movabs $0x1234567890abcdef, %rsp
    // 48 b8 ff ff 22 22 cc cc 99 99    movabs $0x9999cccc2222ffff, %rax
    // ```
    //
    // The values `0xdeaddead`, `0xff00badd`, `0x1234567890abcdef`, and `0x9999cccc2222ffff` are
    // placeholders that we overwrite in the block below.
    unsafe {
        let ap_boot_marker1_loc: *mut u8 = {
            let offset = code_template.marker1_offset;
            bootstrap_code_buf.as_mut_ptr().add(offset)
        };
        let ap_boot_marker2_loc: *mut u8 = {
            let offset = code_template.marker2_offset;
            bootstrap_code_buf.as_mut_ptr().add(offset)
        };
        let ap_boot_marker3_loc: *mut u8 = {
            let offset = code_template.marker3_offset;
            bootstrap_code_buf.as_mut_ptr().add(offset)
        };

        // Perform some sanity check. Since we're doing dark magic, we really want to be sure
        // that we're overwriting the correct code, or we will run into issues that are very hard
        // to debug.
        assert_eq!(
            core::slice::from_raw_parts(ap_boot_marker1_loc as *const u8, 7),
            &[0x66, 0xea, 0xad, 0xde, 0xad, 0xde, 0x08]
        );
        assert_eq!(
            core::slice::from_raw_parts(ap_boot_marker2_loc as *const u8, 20),
            &[
                0x48, 0xbc, 0xef, 0xcd, 0xab, 0x90, 0x78, 0x56, 0x34, 0x12, 0x48, 0xb8, 0xff, 0xff,
                0x22, 0x22, 0xcc, 0xcc, 0x99, 0x99
            ]
        );
        assert_eq!(
            core::slice::from_raw_parts(ap_boot_marker3_loc as *const u8, 6),
            &[0x66, 0xba, 0xdd, 0xba, 0x00, 0xff]
        );

        // Write first constant at marker 2.
        let stack_ptr_ptr = (ap_boot_marker2_loc.add(2)) as *mut u64;
        assert_eq!(stack_ptr_ptr.read_unaligned(), 0x1234567890abcdef);
        stack_ptr_ptr.write_unaligned(stack_top.try_into().unwrap());

        // Write second constant at marker 2.
        let param_ptr = (ap_boot_marker2_loc.add(12)) as *mut u64;
        assert_eq!(param_ptr.read_unaligned(), 0x9999cccc2222ffff);
        param_ptr.write_unaligned(ap_after_boot_param.try_into().unwrap());

        // Write the location of marker 2 into the constant at marker 1.
        let ljmp_target_ptr = (ap_boot_marker1_loc.add(2)) as *mut u32;
        assert_eq!(ljmp_target_ptr.read_unaligned(), 0xdeaddead);
        ljmp_target_ptr.write_unaligned((ap_boot_marker2_loc as usize).try_into().unwrap());

        // Write the value of our `cr3` register to the constant at marker 3.
        let pml_addr_ptr = (ap_boot_marker3_loc.add(2)) as *mut u32;
        assert_eq!(pml_addr_ptr.read_unaligned(), 0xff00badd);
        pml_addr_ptr.write_unaligned({
            let pml_addr = x86_64::registers::control::Cr3::read()
                .0
                .start_address()
                .as_u64();
            pml_addr.try_into().unwrap()
        });
    }

    // Wait for 10ms to have elapsed since we sent the INIT IPI.
    crate::task::spawn_blocking(wait_after_init);

    // Because the APIC doesn't automatically try submitting the SIPI in case the target CPU was
    // busy, we might have to try multiple times.
    let mut attempts = 0;
    loop {
        // Failure to initialize.
        if attempts >= 2 {
            // Before returning an error, we free the closure that was destined to be run by
            // the AP.
            let param = ap_after_boot_param as ApAfterBootParam;
            let _ = unsafe { Box::from_raw(param) };
            break;
        }
        attempts += 1;

        // Send the SINIT IPI, pointing to the bootstrap code that we have carefully crafted.
        // local_apics.send_interprocessor_sipi(target, bootstrap_code_buf.as_mut_ptr() as *const _);

        // Wait for the processor initialization to finish, but with a timeout in order to not
        // wait forever if nothing happens.
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

    // Make sure the buffer is dropped at the end.
    drop(bootstrap_code_buf);
}

pub fn init(acpi_platform_info: &PlatformInfo) {
    println!("[SMP] initializing");

    for processor in &acpi_platform_info
        .processor_info
        .as_ref()
        .unwrap()
        .application_processors
    {
        start_application_processor(processor);
    }

    println!("[SMP] initialized");
}

struct Template {
    code: &'static [u8],
    marker1_offset: usize,
    marker2_offset: usize,
    marker3_offset: usize,
}

fn get_template() -> Template {
    let code_start: usize;
    let code_end: usize;
    let marker1: usize;
    let marker2: usize;
    let marker3: usize;

    // The code here is the template in question. Just like any code, is included in the kernel
    // and will be loaded in memory. However, it is not actually meant be executed. Instead it
    // is meant to be used as a template.
    // Because the associated processor (AP) boot code must be in the first megabyte of memory,
    // we first copy this code somewhere in this first megabyte and adjust it.
    //
    // The `code_start` and `code_end` addresses encompass the template. There exist three other
    // symbols `marker1`, `marker2` and `marker3` that point to instructions that must be adjusted
    // before execution.
    //
    // Within this module, we must be careful to not use any absolute address referring to
    // anything between `code_start` and `code_end`, and to not use any relative address referring
    // to anything outside of this range, as the addresses will then be wrong when the code gets
    // copied.
    unsafe {
        asm!(r#"
            // This jmp is **not** part of the template. It is reached only when `get_template`
            // is called.
            jmp 5f

        .code16
        .align 0x1000
        4:
            // When we enter here, the CS register is set to the value that we passed through the
            // SIPI, and the IP register is set to `0`.
            //
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

        3:
            // The `0xff00badd` constant below is replaced with the address of a PML4 table when
            // the template gets adjusted.
            mov $0xff00badd, %edx
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

            // Set up the GDT. Since the absolute address of the tempalte start is effectively 0
            // according to the CPU in this 16 bits context, we pass an "absolute" address to the
            // GDT by substracting `code_start` from its 32 bits address.
            lgdtl (6f - 4b)

        1:
            // A long jump is necessary in order to update the CS registry and properly switch to
            // long mode.
            // The `0xdeaddead` constant below is replaced with the location of the marker `2`
            // below the template gets adjusted.
            ljmpl $8, $0xdeaddead

        .code64
        2:
            // The constants below are replaced with an actual stack location when the template
            // gets adjusted.
            // Set up the stack.
            movq $0x1234567890abcdef, %rsp

            // This is an opaque value for the purpose of this assembly code. It is the parameter
            // that we pass to `ap_after_boot`
            movq $0x9999cccc2222ffff, %rax

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
            lea {ap_after_boot}, %rdx
            call *%rdx

            cli
            hlt

            // Small structure whose location is passed to the CPU in order to load the GDT.
            // Because we call `lgdt` from 16bits code, we have to define the GDT pointer
            // locally.

        .align 8
        6:
            .short 15
            .long {gdt}

        5:
            // This code is not part of the template and is executed by `get_template`.
            lea (4b), {code_start}
            lea (5b), {code_end}
            lea (1b), {marker1}
            lea (2b), {marker2}
            lea (3b), {marker3}
        "#,
            ap_after_boot = sym ap_after_boot,
            gdt = sym super::gdt::GDT,
            code_start = out(reg) code_start,
            code_end = out(reg) code_end,
            marker1 = out(reg) marker1,
            marker2 = out(reg) marker2,
            marker3 = out(reg) marker3,
            options(pure, nostack, nomem, preserves_flags, att_syntax)
        );

        Template {
            code: core::slice::from_raw_parts(code_start as *const u8, code_end - code_start),
            marker1_offset: marker1 - code_start,
            marker2_offset: marker2 - code_start,
            marker3_offset: marker3 - code_start,
        }
    }
}

type ApAfterBootParam = *mut Box<dyn FnOnce() -> ! + Send + 'static>;

/// Called by the template code after setup.
///
/// When this function is called, the stack and paging have already been properly set up. The
/// first parameter is gathered from the `rdi` register according to the x86_64 calling
/// convention.
extern "C" fn ap_after_boot(to_exec: usize) -> ! {
    unsafe {
        let to_exec = to_exec as ApAfterBootParam;
        let to_exec = Box::from_raw(to_exec);
        (*to_exec)();
    }
}
