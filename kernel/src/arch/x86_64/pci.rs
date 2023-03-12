use super::acpi::AcpiAllocator;
use acpi::PciConfigRegions;
use alloc::collections::BTreeMap;
use pci_ids::{Device as DeviceInfo, Subclass as SubclassInfo};
use pci_types::{
    Bar, ConfigRegionAccess, EndpointHeader, HeaderType, PciAddress, PciHeader, PciPciBridgeHeader,
};
use x86_64::VirtAddr;

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

                    self.devices.insert(
                        address,
                        PciDevice {
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

pub struct PciDevice {
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
}

pub fn init(regions: PciConfigRegions<AcpiAllocator>, physical_offset: Option<u64>) {
    let resolver = Resolver {
        offset: physical_offset.unwrap_or(0),
        regions,
        devices: BTreeMap::new(),
    };

    let devices = resolver.resolve();
    for (address, device) in devices {
        println!(
            "PCI {address} {:04x}:{:04x} {}",
            device.vendor_id,
            device.device_id,
            device.name()
        );
    }

    println!("[PCI] initialized");
}
