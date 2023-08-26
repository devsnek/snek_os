use crate::arch::PciDevice;
use conquer_once::spin::OnceCell;

pub mod i8042;

static INITIALIZED: OnceCell<Vec<Box<dyn Driver>>> = OnceCell::uninit();

pub trait Driver: Send + Sync {}

type DriverInit = fn(&PciDevice) -> Option<Box<dyn Driver>>;

static DRIVER_INITS: &[DriverInit] = &[];

pub fn init() {
    i8042::init();

    let mut devices = vec![];

    for (address, device) in crate::arch::get_pci_devices() {
        println!(
            "PCI {address} {:04x}:{:04x} {}",
            device.vendor_id,
            device.device_id,
            device.name()
        );

        for init in DRIVER_INITS {
            if let Some(driver) = init(device) {
                devices.push(driver);
                break;
            }
        }
    }

    INITIALIZED.try_init_once(|| devices).unwrap();
}
