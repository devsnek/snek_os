mod dma;
mod e1000;
#[cfg(target_arch = "x86_64")]
pub mod i8042;
pub mod keyboard;
mod nvme;

use crate::arch::PciDevice;

type DriverInit = fn(&PciDevice) -> bool;

static DRIVER_INITS: &[DriverInit] = &[e1000::init /*, nvme::init*/];

pub fn init() {
    #[cfg(target_arch = "x86_64")]
    i8042::init();

    for (address, device) in &*crate::arch::get_pci_devices() {
        println!(
            "PCI {address} {:04x}:{:04x} {}",
            device.vendor_id,
            device.device_id,
            device.name()
        );

        for init in DRIVER_INITS {
            if init(device) {
                break;
            }
        }
    }
}
