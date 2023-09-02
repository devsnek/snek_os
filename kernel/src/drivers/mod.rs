use crate::arch::PciDevice;

pub mod e1000;
pub mod i8042;

type DriverInit = fn(&PciDevice) -> bool;

static DRIVER_INITS: &[DriverInit] = &[e1000::init];

pub fn init() {
    i8042::init();

    for (address, device) in crate::arch::get_pci_devices() {
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
