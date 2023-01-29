fn main() {
    let uefi_path = env!("UEFI_PATH");
    let bios_path = env!("BIOS_PATH");

    let use_bios = std::env::args().find(|a| a == "--bios").is_some();

    let mut cmd = std::process::Command::new("qemu-system-x86_64");
    if use_bios {
        cmd.arg("-drive")
            .arg(format!("format=raw,file={bios_path}"));
    } else {
        cmd.arg("-bios").arg(ovmf_prebuilt::ovmf_pure_efi());
        cmd.arg("-drive")
            .arg(format!("format=raw,file={uefi_path}"));
    }

    let mut child = cmd.spawn().unwrap();
    child.wait().unwrap();
}
