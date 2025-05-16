use pci_ids::{Device as DeviceInfo, Subclass as SubclassInfo};
use pci_types::{capability::PciCapability, Bar, PciAddress};

bitflags::bitflags! {
    #[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
    pub struct PciCommand: u16 {
        const IO_SPACE = 1 << 0;
        const MEMORY_SPACE = 1 << 1;
        const BUS_MASTER = 1 << 2;
        const SPECIAL_CYCLES = 1 << 3;
        const MEMORY_WRITE_AND_INVALIDATE_ENABLE = 1 << 4;
        const VGA_PALETTE_SNOOP = 1 << 5;
        const PARITY_ERROR_RESPONSE = 1 << 6;
        const SERR_ENABLE = 1 << 8;
        const FAST_BACK_TO_BACK_ENABLE = 1 << 9;
        const INTERRUPT_DISABLE = 1 << 10;
    }
}

bitflags::bitflags! {
    #[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
    pub struct PciStatus: u16 {
        const INTERRUPT_STATUS = 1 << 3;
        const CAPABILITIES_LIST = 1 << 4;
        const MHZ_66_CAPABLE = 1 << 5;
        const FAST_BACK_TO_BACK_CAPABLE = 1 << 7;
        const MASTER_DATA_PARITY_ERROR = 1 << 8;
        const SIGNALED_TARGET_ABORT = 1 << 11;
        const RECEIVED_TARGET_ABORT = 1 << 12;
        const RECEIVED_MASTER_ABORT = 1 << 13;
        const SIGNALED_SYSTEM_ERROR = 1 << 14;
        const DETECTED_PARITY_ERROR = 1 << 15;
    }
}

#[derive(Debug, Clone)]
pub struct PciDevice {
    pub physical_offset: usize,
    pub configuration_address: usize,
    pub address: PciAddress,
    pub class: u8,
    pub sub_class: u8,
    pub interface: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub sub_vendor_id: u16,
    pub sub_device_id: u16,
    pub revision: u8,
    pub bars: [Option<Bar>; 6],
    pub interrupt_pin: u8,
    pub interrupt_line: u8,
    pub capabilities: Vec<PciCapability>,
}

impl PciDevice {
    pub fn name(&self) -> String {
        if let Some(device) = DeviceInfo::from_vid_pid(self.vendor_id, self.device_id) {
            format!("{} {}", device.vendor().name(), device.name())
        } else {
            SubclassInfo::from_cid_sid(self.class, self.sub_class)
                .map(|subclass| {
                    subclass
                        .prog_ifs()
                        .find(|i| i.id() == self.interface)
                        .map(|i| i.name())
                        .unwrap_or(subclass.name())
                })
                .unwrap_or("Unknown Device")
                .to_owned()
        }
    }

    pub unsafe fn read<T: Sized + Copy>(&self, offset: u16) -> T {
        ((self.configuration_address + offset as usize) as *const T).read_volatile()
    }

    pub unsafe fn write<T: Sized + Copy>(&self, offset: u16, value: T) {
        ((self.configuration_address + offset as usize) as *mut T).write_volatile(value)
    }

    pub fn command(&self) -> PciCommand {
        PciCommand::from_bits_truncate(unsafe { self.read::<u16>(0x04) })
    }

    pub fn set_command(&self, cmd: PciCommand) {
        unsafe { self.write(0x04, cmd) }
    }

    pub fn status(&self) -> PciStatus {
        PciStatus::from_bits_truncate(unsafe { self.read::<u16>(0x06) })
    }

    pub fn capabilities_offset(&self) -> Option<u8> {
        if self.status().contains(PciStatus::CAPABILITIES_LIST) {
            Some((unsafe { self.read::<u32>(0x34) } & 0xFC) as u8)
        } else {
            None
        }
    }
}
