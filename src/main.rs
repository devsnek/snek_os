fn main() {
    let uefi_path = env!("UEFI_PATH");
    let bios_path = env!("BIOS_PATH");

    let use_bios = std::env::args().any(|a| &a == "--bios");
    let use_kvm = !std::env::args().any(|a| &a == "--no-kvm");
    let gdb = std::env::args().any(|a| &a == "--gdb");

    let mut cmd = std::process::Command::new("qemu-system-x86_64");
    if use_bios {
        cmd.arg("-drive")
            .arg(format!("format=raw,file={bios_path}"));
    } else {
        cmd.arg("-bios").arg(ovmf_prebuilt::ovmf_pure_efi());
        cmd.arg("-drive")
            .arg(format!("format=raw,file={uefi_path}"));
    }

    if use_kvm {
        cmd.arg("-enable-kvm");
    }

    if gdb {
        cmd.arg("-s");
    }

    let mut child = cmd.spawn().unwrap();
    child.wait().unwrap();
}
