use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());

    // let kernel = PathBuf::from(std::env::var_os("CARGO_BIN_FILE_SNEK_KERNEL_snek_kernel").unwrap());
    /*
    assert!(std::process::Command::new("cargo")
        .args(["build", "-p", "snek_kernel", "--target", "x86_64-unknown-none"])
        .status()
        .unwrap()
        .success());
    */
    let kernel = std::fs::canonicalize(PathBuf::from("./target/x86_64-unknown-none/debug/snek_kernel")).unwrap();

    let uefi_path = out_dir.join("uefi.img");
    bootloader::UefiBoot::new(&kernel)
        .create_disk_image(&uefi_path)
        .unwrap();

    let bios_path = out_dir.join("bios.img");
    bootloader::BiosBoot::new(&kernel)
        .create_disk_image(&bios_path)
        .unwrap();

    println!("cargo:rerun-if-changed={}", uefi_path.display());
    println!("cargo:rerun-if-changed={}", bios_path.display());

    println!("cargo:rustc-env=UEFI_PATH={}", uefi_path.display());
    println!("cargo:rustc-env=BIOS_PATH={}", bios_path.display());

    let target_dir = PathBuf::from("./target").join(std::env::var_os("PROFILE").unwrap());
    std::fs::copy(bios_path, target_dir.join("bios.img")).unwrap();
    std::fs::copy(uefi_path, target_dir.join("uefi.img")).unwrap();
}
