use super::stack_allocator::StackAllocator;
use acpi::{
    mcfg::PciConfigRegions, platform::PlatformInfo, AcpiHandler, AcpiTables, PhysicalMapping,
};
use x86_64::VirtAddr;

pub type AcpiAllocator = StackAllocator<256>;

pub fn init(
    allocator: &AcpiAllocator,
    rsdp_address: VirtAddr,
) -> (
    PlatformInfo<'_, AcpiAllocator>,
    PciConfigRegions<'_, AcpiAllocator>,
) {
    let acpi_tables = allocator.own(
        unsafe { AcpiTables::from_rsdp(AcpiHandlerImpl, rsdp_address.as_u64() as _) }.unwrap(),
    );

    let pci_regions = PciConfigRegions::new_in(acpi_tables, allocator).unwrap();

    (
        acpi_tables.platform_info_in(allocator).unwrap(),
        pci_regions,
    )
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
