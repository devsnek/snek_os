#[naked]
pub unsafe extern "C" fn enter_user_mode() {
    asm!(
        "
        cli
        mov $0x20 | 0x3, %ax
        mov %ax, %ds
        mov %ax, %es
        mov %ax, %fs
        mov %ax, %gs

        mov %rsp, %rax
        push $0x20 | 0x3 # ss
        push %rax        # esp
        pushfq           # eflags
        pop %rax
        or $0x200, %rax  # set IF
        push %rax
        push $0x18 | 0x3 # cs
        push $1f         # eip
        iretq
        1:
        ",
        options(att_syntax, noreturn)
    );
}
