use super::{get_pci_config_regions, memory::translate_phys_addr};
use crate::arch::PciDevice;
use acpi::PciConfigRegions;
use alloc::collections::BTreeMap;
use pci_types::{Bar, EndpointHeader, HeaderType, PciAddress, PciHeader, PciPciBridgeHeader};
use spin::{rwlock::RwLockReadGuard, RwLock};
use x86_64::PhysAddr;

pub struct ConfigRegionAccess {
    regions: PciConfigRegions<'static, alloc::alloc::Global>,
}

impl Default for ConfigRegionAccess {
    fn default() -> Self {
        Self {
            regions: get_pci_config_regions(),
        }
    }
}

impl pci_types::ConfigRegionAccess for ConfigRegionAccess {
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
        let ptr = translate_phys_addr(PhysAddr::new(phys))
            .as_ptr::<u32>()
            .byte_add(offset as _);
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
        let ptr = translate_phys_addr(PhysAddr::new(phys))
            .as_mut_ptr::<u32>()
            .byte_add(offset as _);
        core::ptr::write_volatile(ptr, value);
    }
}

struct Resolver {
    access: ConfigRegionAccess,
    offset: u64,
    devices: BTreeMap<PciAddress, PciDevice>,
}

impl Resolver {
    fn resolve(mut self) -> BTreeMap<PciAddress, PciDevice> {
        let segments = self
            .access
            .regions
            .iter()
            .map(|region| region.segment_group)
            .collect::<Vec<_>>();

        segments.into_iter().for_each(|segment| {
            self.scan_segment(segment);
        });

        self.devices
    }

    fn function_exists(&self, address: PciAddress) -> bool {
        self.access
            .regions
            .physical_address(
                address.segment(),
                address.bus(),
                address.device(),
                address.function(),
            )
            .is_some()
    }

    fn scan_segment(&mut self, segment: u16) {
        let address = PciAddress::new(segment, 0, 0, 0);
        if self.function_exists(address)
            && PciHeader::new(address).has_multiple_functions(&self.access)
        {
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
                if header.has_multiple_functions(&self.access) {
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
            let (vendor_id, device_id) = header.id(&self.access);

            if vendor_id == 0xffff || device_id == 0xffff {
                return;
            }

            let (revision, class, sub_class, interface) = header.revision_and_class(&self.access);

            match header.header_type(&self.access) {
                HeaderType::Endpoint => {
                    let endpoint_header =
                        EndpointHeader::from_header(header, &self.access).unwrap();
                    let (sub_vendor_id, sub_device_id) = endpoint_header.subsystem(&self.access);
                    let bars = {
                        let mut bars = [None; 6];

                        let mut skip_next = false;
                        for i in 0..6 {
                            if skip_next {
                                skip_next = false;
                                continue;
                            }

                            let bar = endpoint_header.bar(i, &self.access);
                            skip_next = matches!(bar, Some(Bar::Memory64 { .. }));
                            bars[i as usize] = bar;
                        }

                        bars
                    };

                    let (interrupt_pin, interrupt_line) = endpoint_header.interrupt(&self.access);
                    let configuration_address = self
                        .access
                        .regions
                        .physical_address(
                            address.segment(),
                            address.bus(),
                            address.device(),
                            address.function(),
                        )
                        .unwrap();

                    let capabilities = endpoint_header.capabilities(&self.access).collect();

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
                            capabilities,
                        },
                    );
                }
                HeaderType::PciPciBridge => {
                    let header = PciPciBridgeHeader::from_header(header, &self.access).unwrap();
                    let start = header.secondary_bus_number(&self.access);
                    let end = header.subordinate_bus_number(&self.access);
                    for bus_id in start..=end {
                        self.scan_bus(segment, bus_id);
                    }
                }
                _ => {}
            }
        }
    }
}

static DEVICES: RwLock<BTreeMap<PciAddress, PciDevice>> = RwLock::new(BTreeMap::new());

pub fn get_devices() -> RwLockReadGuard<'static, BTreeMap<PciAddress, PciDevice>> {
    DEVICES.read()
}

pub fn init(physical_offset: u64) {
    let resolver = Resolver {
        access: Default::default(),
        offset: physical_offset,
        devices: BTreeMap::new(),
    };

    let mut devices = resolver.resolve();

    DEVICES.write().append(&mut devices);

    debug!("[PCI] initialized");
}
