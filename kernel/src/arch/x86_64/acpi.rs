use acpi::{
    mcfg::PciConfigRegions, platform::PlatformInfo, AcpiHandler, AcpiTables, PhysicalMapping,
};
use x86_64::{PhysAddr, VirtAddr};

#[derive(Clone)]
struct AcpiHandlerImpl;

impl AcpiHandler for AcpiHandlerImpl {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        let virtual_address =
            super::memory::map_address(PhysAddr::new(physical_address as u64), size);

        PhysicalMapping::new(
            physical_address,
            core::ptr::NonNull::new(virtual_address.as_mut_ptr()).unwrap(),
            size,
            size,
            self.clone(),
        )
    }

    fn unmap_physical_region<T>(region: &PhysicalMapping<Self, T>) {
        use os_units::Bytes;
        use x86_64::structures::paging::Mapper;
        use x86_64::structures::paging::{Page, PageSize, Size4KiB};

        let start = VirtAddr::new(region.virtual_start().as_ptr() as u64);
        let object_size = Bytes::new(region.region_length());

        let start_frame_addr = start.align_down(Size4KiB::SIZE);
        let end_frame_addr = (start + object_size.as_usize()).align_down(Size4KiB::SIZE);

        let num_pages =
            Bytes::new((end_frame_addr - start_frame_addr) as usize).as_num_of_pages::<Size4KiB>();

        let mut binding1 = super::memory::MAPPER.lock();
        let mapper = binding1.as_mut().unwrap();

        for i in 0..num_pages.as_usize() {
            let page =
                Page::<Size4KiB>::containing_address(start_frame_addr + Size4KiB::SIZE * i as u64);

            let (_frame, flusher) = mapper.unmap(page).unwrap();
            flusher.flush();
        }
    }
}

pub fn init(rsdp_address: PhysAddr) -> (PlatformInfo, PciConfigRegions) {
    let acpi_tables =
        unsafe { AcpiTables::from_rsdp(AcpiHandlerImpl, rsdp_address.as_u64() as _) }.unwrap();

    let pci_regions = PciConfigRegions::new(&acpi_tables).unwrap();

    (acpi_tables.platform_info().unwrap(), pci_regions)
}
