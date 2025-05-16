use super::{create_transport, Hal};
use crate::arch::PciDevice;
use virtio_drivers::{device::socket::VirtIOSocket, transport::pci::PciTransport};

type Device = VirtIOSocket<Hal, PciTransport, 512>;

pub fn init(header: &PciDevice) -> Result<bool, anyhow::Error> {
    if header.vendor_id != 0x1af4 || header.device_id != 0x1053 {
        return Ok(false);
    }

    let transport = create_transport(header)?;

    let _device = Device::new(transport)?;

    Ok(true)
}
