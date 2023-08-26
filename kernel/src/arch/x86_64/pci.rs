use super::acpi::AcpiAllocator;
use acpi::PciConfigRegions;
use alloc::collections::BTreeMap;
use pci_ids::{Device as DeviceInfo, Subclass as SubclassInfo};
use pci_types::{
    Bar, ConfigRegionAccess, EndpointHeader, HeaderType, PciAddress, PciHeader, PciPciBridgeHeader,
};
use x86_64::{
    instructions::port::Port,
    structures::port::{PortRead, PortWrite},
    VirtAddr,
};

struct Resolver<'a> {
    regions: PciConfigRegions<'a, AcpiAllocator>,
    offset: u64,
    devices: BTreeMap<PciAddress, PciDevice>,
}

impl<'a> ConfigRegionAccess for Resolver<'a> {
    fn function_exists(&self, address: PciAddress) -> bool {
        self.regions
            .physical_address(
                address.segment(),
                address.bus(),
                address.device(),
                address.device(),
            )
            .is_some()
    }

    unsafe fn read(&self, address: PciAddress, offset: u16) -> u32 {
        let phys = self
            .regions
            .physical_address(
                address.segment(),
                address.bus(),
                address.device(),
                address.function(),
            )
            .unwrap();
        let ptr = VirtAddr::new(self.offset + phys + offset as u64).as_ptr();
        core::ptr::read_volatile(ptr)
    }

    unsafe fn write(&self, address: PciAddress, offset: u16, value: u32) {
        let phys = self
            .regions
            .physical_address(
                address.segment(),
                address.bus(),
                address.device(),
                address.function(),
            )
            .unwrap();
        let ptr = VirtAddr::new(self.offset + phys + offset as u64).as_mut_ptr();
        core::ptr::write_volatile(ptr, value);
    }
}

impl<'a> Resolver<'a> {
    fn resolve(mut self) -> BTreeMap<PciAddress, PciDevice> {
        if PciHeader::new(PciAddress::new(0, 0, 0, 0)).has_multiple_functions(&self) {
            for i in 0..8 {
                self.scan_bus(i);
            }
        } else {
            self.scan_bus(0);
        }

        self.devices
    }

    fn scan_bus(&mut self, bus: u8) {
        for device in 0..32 {
            let address = PciAddress::new(0, bus, device, 0);
            if self.function_exists(address) {
                self.scan_function(bus, device, 0);
                let header = PciHeader::new(address);
                if header.has_multiple_functions(self) {
                    for function in 1..8 {
                        self.scan_function(bus, device, function);
                    }
                }
            }
        }
    }

    fn scan_function(&mut self, bus: u8, device: u8, function: u8) {
        let address = PciAddress::new(0, bus, device, function);
        if self.function_exists(address) {
            let header = PciHeader::new(address);
            let (vendor_id, device_id) = header.id(self);
            let (revision, class, sub_class, interface) = header.revision_and_class(self);

            if vendor_id == 0xffff {
                return;
            }

            match header.header_type(self) {
                HeaderType::Endpoint => {
                    let endpoint_header = EndpointHeader::from_header(header, self).unwrap();
                    let (sub_vendor_id, sub_device_id) = endpoint_header.subsystem(self);
                    let bars = {
                        let mut bars = [None; 6];

                        let mut skip_next = false;
                        for i in 0..6 {
                            if skip_next {
                                continue;
                            }

                            let bar = endpoint_header.bar(i, self);
                            skip_next = matches!(bar, Some(Bar::Memory64 { .. }));
                            bars[i as usize] = bar;
                        }

                        bars
                    };

                    let (interrupt_pin, interrupt_line) = endpoint_header.interrupt(self);

                    self.devices.insert(
                        address,
                        PciDevice {
                            physical_offset: self.offset as _,
                            address,
                            class,
                            sub_class,
                            interface,
                            vendor_id,
                            device_id,
                            sub_vendor_id,
                            sub_device_id,
                            revision,
                            bars,
                            interrupt_pin,
                            interrupt_line,
                        },
                    );
                }
                HeaderType::PciPciBridge => {
                    let header = PciPciBridgeHeader::from_header(header, self).unwrap();
                    let start = header.secondary_bus_number(self);
                    let end = header.subordinate_bus_number(self);
                    for bus_id in start..=end {
                        self.scan_bus(bus_id);
                    }
                }
                _ => {
                    // unknown header
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct PciDevice {
    pub physical_offset: usize,
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
}

const PCI_CONFIG_ADDRESS_PORT: u16 = 0xCF8;
const PCI_CONFIG_DATA_PORT: u16 = 0xCFC;

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

    fn address(&self, offset: u32) -> u32 {
        ((self.address.bus() as u32) << 16)
            | ((self.address.device() as u32) << 11)
            | ((self.address.function() as u32) << 8)
            | (offset & 0xFC)
            | 0x80000000
    }

    unsafe fn read<T: PciReadWrite>(&self, offset: u32) -> T {
        Port::new(PCI_CONFIG_ADDRESS_PORT).write(self.address(offset));

        let value = Port::new(PCI_CONFIG_DATA_PORT).read();
        T::read(offset, value)
    }

    unsafe fn write<T: PciReadWrite>(&self, offset: u32, value: T) {
        Port::new(PCI_CONFIG_ADDRESS_PORT).write(self.address(offset));
        let current = self.read::<u32>(offset);
        Port::new(PCI_CONFIG_DATA_PORT).write(T::write(offset, current, value));
    }

    pub fn enable_mmio(&self) {
        let command = unsafe { self.read::<u16>(0x04) };
        unsafe { self.write::<u16>(0x04, command | (1 << 1)) }
    }

    pub fn enable_bus_mastering(&self) {
        let command = unsafe { self.read::<u16>(0x04) };
        unsafe { self.write::<u16>(0x04, command | (1 << 2)) }
    }
}

static mut DEVICES: BTreeMap<PciAddress, PciDevice> = BTreeMap::new();

pub fn get_devices() -> &'static BTreeMap<PciAddress, PciDevice> {
    unsafe { &DEVICES }
}

pub fn init(regions: PciConfigRegions<AcpiAllocator>, physical_offset: u64) {
    let resolver = Resolver {
        offset: physical_offset,
        regions,
        devices: BTreeMap::new(),
    };

    let mut devices = resolver.resolve();

    unsafe {
        DEVICES.append(&mut devices);
    }

    println!("[PCI] initialized");
}

trait PciReadWrite: PortRead + PortWrite + Sized {
    fn read(offset: u32, value: u32) -> Self;
    fn write(offset: u32, current: u32, value: Self) -> u32;
}

impl PciReadWrite for u8 {
    fn read(offset: u32, value: u32) -> Self {
        (value >> offset) as _
    }

    fn write(offset: u32, current: u32, value: Self) -> u32 {
        let mask = !(0xffu32 << offset);
        (current & mask) | ((value as u32 & 0xff) << offset)
    }
}

impl PciReadWrite for u16 {
    fn read(offset: u32, value: u32) -> Self {
        (value >> offset) as _
    }

    fn write(offset: u32, current: u32, value: Self) -> u32 {
        let noffset = (offset & 0b11) * 8;
        let mask = !(0xffffu32 << noffset);
        (current & mask) | ((value as u32 & 0xffff) << noffset)
    }
}

impl PciReadWrite for u32 {
    fn read(_offset: u32, value: u32) -> Self {
        value
    }

    fn write(_offset: u32, _current: u32, value: Self) -> u32 {
        value
    }
}
