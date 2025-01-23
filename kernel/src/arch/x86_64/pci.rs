use crate::arch::PciDevice;
use acpi::PciConfigRegions;
use alloc::collections::BTreeMap;
use pci_types::{
    Bar, ConfigRegionAccess, EndpointHeader, HeaderType, PciAddress, PciHeader, PciPciBridgeHeader,
};
use spin::{rwlock::RwLockReadGuard, RwLock};
use x86_64::VirtAddr;

struct Resolver<'a> {
    regions: PciConfigRegions<'a, &'static alloc::alloc::Global>,
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
        let segments = self
            .regions
            .iter()
            .map(|region| region.segment_group)
            .collect::<Vec<_>>();

        segments.into_iter().for_each(|segment| {
            self.scan_segment(segment);
        });

        self.devices
    }

    fn scan_segment(&mut self, segment: u16) {
        if PciHeader::new(PciAddress::new(segment, 0, 0, 0)).has_multiple_functions(self) {
            for i in 0..8 {
                self.scan_bus(segment, i);
            }
        } else {
            self.scan_bus(segment, 0);
        }
    }

    fn scan_bus(&mut self, segment: u16, bus: u8) {
        for device in 0..32 {
            let address = PciAddress::new(segment, bus, device, 0);
            if self.function_exists(address) {
                self.scan_function(segment, bus, device, 0);
                let header = PciHeader::new(address);
                if header.has_multiple_functions(self) {
                    for function in 1..8 {
                        self.scan_function(segment, bus, device, function);
                    }
                }
            }
        }
    }

    fn scan_function(&mut self, segment: u16, bus: u8, device: u8, function: u8) {
        let address = PciAddress::new(segment, bus, device, function);
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
                                skip_next = false;
                                continue;
                            }

                            let bar = endpoint_header.bar(i, self);
                            skip_next = matches!(bar, Some(Bar::Memory64 { .. }));
                            bars[i as usize] = bar;
                        }

                        bars
                    };

                    let (interrupt_pin, interrupt_line) = endpoint_header.interrupt(self);
                    let configuration_address = self
                        .regions
                        .physical_address(
                            address.segment(),
                            address.bus(),
                            address.device(),
                            address.device(),
                        )
                        .unwrap();

                    self.devices.insert(
                        address,
                        PciDevice {
                            physical_offset: self.offset as _,
                            configuration_address: configuration_address as _,
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
                        self.scan_bus(segment, bus_id);
                    }
                }
                _ => {
                    // unknown header
                }
            }
        }
    }
}

static DEVICES: RwLock<BTreeMap<PciAddress, PciDevice>> = RwLock::new(BTreeMap::new());

pub fn get_devices() -> RwLockReadGuard<'static, BTreeMap<PciAddress, PciDevice>> {
    DEVICES.read()
}

pub fn init(physical_offset: u64) {
    let regions = super::acpi::get_pci_config_regions();

    let resolver = Resolver {
        offset: physical_offset,
        regions,
        devices: BTreeMap::new(),
    };

    let mut devices = resolver.resolve();

    DEVICES.write().append(&mut devices);

    println!("[PCI] initialized");
}
