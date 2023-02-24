use acpi::PciConfigRegions;
use alloc::collections::BTreeMap;
use pci_types::{Bar, ConfigRegionAccess, EndpointHeader, HeaderType, PciAddress, PciHeader};
use x86_64::VirtAddr;

struct Resolver {
    regions: PciConfigRegions,
    offset: u64,
    devices: BTreeMap<PciAddress, PciDevice>,
}

impl ConfigRegionAccess for Resolver {
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

impl Resolver {
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
                    let bars = {
                        let mut bars = [None; 6];

                        let mut skip_next = false;
                        for i in 0..6 {
                            if skip_next {
                                continue;
                            }

                            let bar = endpoint_header.bar(i, self);
                            skip_next = match bar {
                                Some(Bar::Memory64 { .. }) => true,
                                _ => false,
                            };
                            bars[i as usize] = bar;
                        }

                        bars
                    };

                    self.devices.insert(
                        address,
                        PciDevice {
                            vendor_id,
                            device_id,
                            revision,
                            class,
                            sub_class,
                            interface,
                            bars,
                        },
                    );
                }
                HeaderType::PciPciBridge => {
                    // let header = PciPciBridgeHeader::from_header(header, self).unwrap();
                    // self.scan_bus(header.secondary_bus_number(self));
                }
                _ => {
                    // unknown header
                }
            }
        }
    }
}

pub struct PciDevice {
    pub vendor_id: u16,
    pub device_id: u16,
    pub revision: u8,
    pub class: u8,
    pub sub_class: u8,
    pub interface: u8,
    pub bars: [Option<Bar>; 6],
}

impl PciDevice {
    pub fn class_info(&self) -> &'static str {
        match (self.class, self.sub_class, self.interface) {
            (0x00, 0x00, _) => "Non-VGA-Compatible Unclassified Device",
            (0x00, 0x01, _) => "VGA-Compatible Unclassified Device",
            (0x00, _, _) => "Unclassified Device",
            (0x01, 0x01, _) => "IDE Controller",
            (0x01, 0x02, _) => "Floppy Disk Controller",
            (0x01, 0x05, _) => "ATA Controller",
            (0x01, 0x06, _) => "SATA Controller",
            (0x01, 0x07, _) => "Serial Attached SCSI Controller",
            (0x01, 0x08, 0x01) => "NVMHCI Device",
            (0x01, 0x08, 0x02) => "NVME Device",
            (0x01, 0x08, _) => "Non-Volatile Memory Controller",
            (0x01, _, _) => "Mass Storage Controller",
            (0x02, 0x00, _) => "Ethernet Controller",
            (0x02, _, _) => "Network Controller",
            (0x03, 0x00, _) => "VGA-Compatible Controller",
            (0x03, _, _) => "Display Controller",
            (0x04, _, _) => "Multimedia Controller",
            (0x05, _, _) => "Memory Controller",
            (0x06, 0x00, _) => "Host Bridge",
            (0x06, 0x01, _) => "ISA Bridge",
            (0x06, 0x02, _) => "EISA Bridge",
            (0x06, 0x03, _) => "MCA Bridge",
            (0x06, 0x04, _) => "PCI-to-PCI Bridge",
            (0x06, 0x05, _) => "PCMCIA Bridge",
            (0x06, 0x06, _) => "NuBus Bridge",
            (0x06, 0x07, _) => "CardBus Bridge",
            (0x06, 0x08, _) => "RACEway Bridge",
            (0x06, 0x09, _) => "PCI-to-PCI Bridge",
            (0x06, 0x0A, _) => "InfiniBand-to-PCI Host Bridge",
            (0x06, _, _) => "Bridge",
            (0x07, _, _) => "Simple Communication Controller",
            (0x08, _, _) => "Base System Peripheral",
            (0x09, _, _) => "Input Device Controller",
            (0x0a, _, _) => "Docking Station",
            (0x0b, _, _) => "Processor",
            (0x0c, 0x00, _) => "FireWire Controller",
            (0x0c, 0x02, _) => "SSA",
            (0x0c, 0x03, 0x00) => "UHCI controller",
            (0x0c, 0x03, 0x10) => "OHCI controller",
            (0x0c, 0x03, 0x20) => "EHCI controller",
            (0x0c, 0x03, 0x30) => "XHCI controller",
            (0x0c, 0x03, 0xFE) => "USB device",
            (0x0c, 0x03, _) => "USB Controller",
            (0x0c, 0x04, _) => "Fibre Controller",
            (0x0c, 0x05, _) => "SMBus Controller",
            (0x0c, 0x06, _) => "InfiniBand Controller",
            (0x0c, 0x07, _) => "IPMI Interface",
            (0x0c, _, _) => "Serial Bus Controller",
            (0x0d, _, _) => "Wireless Controller",
            (0x0e, _, _) => "Intelligent Controller",
            (0x0f, _, _) => "Satellite Communication Controller",
            (0x10, _, _) => "Encryption Controller",
            (0x11, _, _) => "Signal Processing Controller",
            (0x12, _, _) => "Processing Accelerator",
            (0x13, _, _) => "Non-Essential Instrumentation",
            (0x40, _, _) => "Co-Processor",
            _ => "Unknown Device",
        }
    }
}

pub fn init(regions: PciConfigRegions, physical_offset: Option<u64>) {
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
            device.class_info()
        );
    }

    println!("[PCI] initialized");
}
