use x86_64::{
    registers::model_specific::{Efer, EferFlags, LStar},
    VirtAddr,
};

pub fn init() {
    unsafe {
        LStar::write(VirtAddr::new(syscall_handler as _));

        Efer::update(|flags| {
            flags.insert(EferFlags::SYSTEM_CALL_EXTENSIONS);
        });
    }
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct ScratchRegisters {
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rax: u64,
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct PreservedRegisters {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbp: u64,
    pub rbx: u64,
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct IretRegisters {
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
struct SyscallStack {
    preserved: PreservedRegisters,
    scratch: ScratchRegisters,
    iret: IretRegisters,
}

extern "C" fn dispatch_syscall(stack: &mut SyscallStack) {
    let syscall_number = stack.scratch.rax as usize; // syscall number
    let a = stack.scratch.rdi as usize; // argument 1
    let b = stack.scratch.rsi as usize; // argument 2
    let c = stack.scratch.rdx as usize; // argument 3
    let d = stack.scratch.r10 as usize; // argument 4
    let e = stack.scratch.r8 as usize; // argument 5
    let f = stack.scratch.r9 as usize; // argument 6

    dbg!(syscall_number, a, b, c, d, e, f);

    stack.scratch.rax = 42;
}

global_asm!(r#"
syscall_handler:
    swapgs

    // mov [gs:TSS_TEMP_USTACK_OFF], rsp   // save the user stack pointer
    // mov rsp, [gs:TSS_RSP0_OFF]          // restore the kernel stack pointer
    push 0x10   // push qword USERLAND_SS              // push userspace SS
    push rsp    // push qword [gs:TSS_TEMP_USTACK_OFF] // push userspace stack pointer
    push r11                            // push RFLAGS
    push 0x10   // push qword USERLAND_CS              // push userspace CS
    push rcx                            // push userspace return pointer

    push rax

    // push scratch
    push rcx
    push rdx
    push rdi
    push rsi
    push r8
    push r9
    push r10
    push r11
    // push preserved
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15

    mov rdi, rsp

    cld
    call {dispatch_syscall}
    cli

    // pop preserved
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    // pop scratch
    pop r11
    pop r10
    pop r9
    pop r8
    pop rsi
    pop rdi
    pop rdx
    pop rcx

    pop rax

    // make the sysret frame
    pop rcx
    add rsp, 8
    pop r11
    pop rsp

    // sysret

    add rsp, 8
    push rcx
    swapgs
    sti
    ret
"#,
    dispatch_syscall = sym dispatch_syscall);

extern "C" {
    fn syscall_handler();
}

#[allow(unused)]
#[inline]
pub unsafe fn syscall0(mut n: usize) -> usize {
    asm!(
        "syscall",
        inout("rax") n,
        out("rcx") _,
        out("r11") _,
        options(nostack),
    );

    n
}

#[allow(unused)]
#[inline]
pub unsafe fn syscall1(mut n: usize, a: usize) -> usize {
    asm!(
        "syscall",
        inout("rax") n,
        in("rdi") a,
        out("rcx") _,
        out("r11") _,
        options(nostack),
    );

    n
}

#[allow(unused)]
#[inline]
pub unsafe fn syscall2(mut n: usize, a: usize, b: usize) -> usize {
    asm!(
        "syscall",
        inout("rax") n,
        in("rdi") a,
        in("rsi") b,
        out("rcx") _,
        out("r11") _,
        options(nostack),
    );

    n
}

#[allow(unused)]
#[inline]
pub unsafe fn syscall3(mut n: usize, a: usize, b: usize, c: usize) -> usize {
    asm!(
        "syscall",
        inout("rax") n,
        in("rdi") a,
        in("rsi") b,
        in("rdx") c,
        out("rcx") _,
        out("r11") _,
        options(nostack),
    );

    n
}

#[allow(unused)]
#[inline]
pub unsafe fn syscall4(mut n: usize, a: usize, b: usize, c: usize, d: usize) -> usize {
    asm!(
        "syscall",
        inout("rax") n,
        in("rdi") a,
        in("rsi") b,
        in("rdx") c,
        in("r10") d,
        out("rcx") _,
        out("r11") _,
        options(nostack),
    );

    n
}

#[allow(unused)]
#[inline]
pub unsafe fn syscall5(mut n: usize, a: usize, b: usize, c: usize, d: usize, e: usize) -> usize {
    asm!(
        "syscall",
        inout("rax") n,
        in("rdi") a,
        in("rsi") b,
        in("rdx") c,
        in("r10") d,
        in("r8") e,
        out("rcx") _,
        out("r11") _,
        options(nostack),
    );

    n
}

#[allow(unused)]
#[inline]
pub unsafe fn syscall6(
    mut n: usize,
    a: usize,
    b: usize,
    c: usize,
    d: usize,
    e: usize,
    f: usize,
) -> usize {
    asm!(
        "syscall",
        inout("rax") n,
        in("rdi") a,
        in("rsi") b,
        in("rdx") c,
        in("r10") d,
        in("r8") e,
        in("r9") f,
        out("rcx") _,
        out("r11") _,
        options(nostack),
    );

    n
}
