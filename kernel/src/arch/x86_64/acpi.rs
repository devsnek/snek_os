use super::stack_allocator::StackAllocator;
use acpi::{
    fadt::Fadt, mcfg::PciConfigRegions, platform::PlatformInfo, AcpiHandler, AcpiTables,
    PhysicalMapping,
};
use aml::{AmlContext, AmlName, AmlValue, DebugVerbosity};
use x86_64::{instructions::port::Port, VirtAddr};

pub type AcpiAllocator = StackAllocator<2048>;

static mut RSDP_ADDRESS: VirtAddr = VirtAddr::zero();

pub fn init(
    allocator: &AcpiAllocator,
    rsdp_address: VirtAddr,
) -> (
    PlatformInfo<'_, AcpiAllocator>,
    PciConfigRegions<'_, AcpiAllocator>,
) {
    unsafe {
        RSDP_ADDRESS = rsdp_address;
    }

    let acpi_tables = allocator.own(
        unsafe { AcpiTables::from_rsdp(AcpiHandlerImpl, rsdp_address.as_u64() as _) }.unwrap(),
    );

    let pci_regions = PciConfigRegions::new_in(acpi_tables, allocator).unwrap();

    (
        acpi_tables.platform_info_in(allocator).unwrap(),
        pci_regions,
    )
}

pub fn pci_route_pin(device: u16, function: u16, pin: u8) {
    let acpi_tables =
        unsafe { AcpiTables::from_rsdp(AcpiHandlerImpl, RSDP_ADDRESS.as_u64() as _) }.unwrap();

    let dsdt = acpi_tables.dsdt().unwrap();
    let table =
        unsafe { core::slice::from_raw_parts(dsdt.address as *mut u8, dsdt.length as usize) };

    let mut aml = AmlContext::new(Box::new(AmlHandler), DebugVerbosity::None);
    aml.parse_table(table).unwrap();
    aml.initialize_objects().unwrap();

    // find_bus(d);
}

pub fn shutdown() {
    let acpi_tables =
        unsafe { AcpiTables::from_rsdp(AcpiHandlerImpl, RSDP_ADDRESS.as_u64() as _) }.unwrap();

    let fadt = acpi_tables.find_table::<Fadt>().unwrap();
    let pm1a_ctl = fadt.pm1a_control_block().unwrap().address;

    let dsdt = acpi_tables.dsdt().unwrap();
    let table =
        unsafe { core::slice::from_raw_parts(dsdt.address as *mut u8, dsdt.length as usize) };

    let mut aml = AmlContext::new(Box::new(AmlHandler), DebugVerbosity::None);
    aml.parse_table(table).unwrap();
    aml.initialize_objects().unwrap();
    let name = AmlName::from_str("\\_S5").unwrap();
    if let Ok(AmlValue::Package(s5)) = aml.namespace.get_by_path(&name) {
        if let AmlValue::Integer(value) = s5[0] {
            let slp_typa = value as u16;
            let slp_en: u16 = 1 << 13;

            unsafe {
                Port::<u16>::new(pm1a_ctl as u16).write(slp_typa | slp_en);
            }
        }
    }
}

#[derive(Clone)]
struct AcpiHandlerImpl;

impl AcpiHandler for AcpiHandlerImpl {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        // limine has already mapped everything for us :)
        PhysicalMapping::new(
            physical_address,
            core::ptr::NonNull::new(physical_address as _).unwrap(),
            size,
            size,
            self.clone(),
        )
    }

    fn unmap_physical_region<T>(_region: &PhysicalMapping<Self, T>) {
        // we didn't map anything, so don't unmap anything
    }
}

struct AmlHandler;

impl aml::Handler for AmlHandler {
    fn read_u8(&self, address: usize) -> u8 {
        unsafe { core::ptr::read(address as *mut u8) }
    }
    fn read_u16(&self, address: usize) -> u16 {
        unsafe { core::ptr::read(address as *mut u16) }
    }
    fn read_u32(&self, address: usize) -> u32 {
        unsafe { core::ptr::read(address as *mut u32) }
    }
    fn read_u64(&self, address: usize) -> u64 {
        unsafe { core::ptr::read(address as *mut u64) }
    }
    fn write_u8(&mut self, _address: usize, _value: u8) {
        unimplemented!()
    }
    fn write_u16(&mut self, _address: usize, _value: u16) {
        unimplemented!()
    }
    fn write_u32(&mut self, _address: usize, _value: u32) {
        unimplemented!()
    }
    fn write_u64(&mut self, _address: usize, _value: u64) {
        unimplemented!()
    }
    fn read_io_u8(&self, _port: u16) -> u8 {
        unimplemented!()
    }
    fn read_io_u16(&self, _port: u16) -> u16 {
        unimplemented!()
    }
    fn read_io_u32(&self, _port: u16) -> u32 {
        unimplemented!()
    }
    fn write_io_u8(&self, _port: u16, _value: u8) {
        unimplemented!()
    }
    fn write_io_u16(&self, _port: u16, _value: u16) {
        unimplemented!()
    }
    fn write_io_u32(&self, _port: u16, _value: u32) {
        unimplemented!()
    }
    fn read_pci_u8(&self, _segment: u16, _bus: u8, _device: u8, _function: u8, _offset: u16) -> u8 {
        unimplemented!()
    }
    fn read_pci_u16(
        &self,
        _segment: u16,
        _bus: u8,
        _device: u8,
        _function: u8,
        _offset: u16,
    ) -> u16 {
        unimplemented!()
    }
    fn read_pci_u32(
        &self,
        _segment: u16,
        _bus: u8,
        _device: u8,
        _function: u8,
        _offset: u16,
    ) -> u32 {
        unimplemented!()
    }
    fn write_pci_u8(
        &self,
        _segment: u16,
        _bus: u8,
        _device: u8,
        _function: u8,
        _offset: u16,
        _value: u8,
    ) {
        unimplemented!()
    }
    fn write_pci_u16(
        &self,
        _segment: u16,
        _bus: u8,
        _device: u8,
        _function: u8,
        _offset: u16,
        _value: u16,
    ) {
        unimplemented!()
    }
    fn write_pci_u32(
        &self,
        _segment: u16,
        _bus: u8,
        _device: u8,
        _function: u8,
        _offset: u16,
        _value: u32,
    ) {
        unimplemented!()
    }
}
