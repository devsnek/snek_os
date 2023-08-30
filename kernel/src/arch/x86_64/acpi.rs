use super::{pci::PciDevice, stack_allocator::StackAllocator};
use acpi::{
    mcfg::PciConfigRegions,
    platform::PlatformInfo,
    sdt::{SdtHeader, Signature},
    AcpiHandler, AcpiTables, PhysicalMapping,
};
use alloc::sync::Arc;
use conquer_once::spin::OnceCell;
use core::convert::TryInto;
use x86_64::{instructions::port::Port, VirtAddr};

pub type AcpiAllocator = StackAllocator<2048>;

static mut RSDP_ADDRESS: VirtAddr = VirtAddr::zero();
static mut ACPI_TABLES: OnceCell<AcpiTables<AcpiHandlerImpl>> = OnceCell::uninit();

fn get_tables() -> &'static AcpiTables<AcpiHandlerImpl> {
    unsafe { ACPI_TABLES.get().unwrap() }
}

pub fn early_init(
    allocator: &AcpiAllocator,
    rsdp_address: VirtAddr,
) -> PlatformInfo<'_, &AcpiAllocator> {
    unsafe {
        RSDP_ADDRESS = rsdp_address;
        ACPI_TABLES
            .try_init_once(|| {
                AcpiTables::from_rsdp(AcpiHandlerImpl, RSDP_ADDRESS.as_u64() as _).unwrap()
            })
            .unwrap();
    }

    println!("[ACPI] early initialized");

    get_tables().platform_info_in(allocator).unwrap()
}

pub fn late_init() {
    let lai_host = Arc::new(LaiHost);
    lai::init(lai_host);

    lai::set_acpi_revision(get_tables().revision() as _);
    lai::create_namespace();

    println!("[ACPI] late initialized");
}

pub fn get_pci_config_regions() -> PciConfigRegions<'static, &'static alloc::alloc::Global> {
    PciConfigRegions::new_in(get_tables(), &alloc::alloc::Global).unwrap()
}

pub fn pci_route_pin(device: &PciDevice) -> u8 {
    lai::pci_route_pin(
        device.address.segment(),
        device.address.bus(),
        device.address.device(),
        device.address.function(),
        device.interrupt_pin,
    )
    .unwrap()
    .base as _
}

pub fn shutdown() {
    lai::enter_sleep(lai::SleepState::Shutdown).unwrap();
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

struct LaiHost;

impl LaiHost {
    fn pci_address(&self, segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> usize {
        let mcfg = get_tables().find_table::<acpi::mcfg::Mcfg>().unwrap();

        let region = mcfg
            .entries()
            .iter()
            .find(|region| {
                region.pci_segment_group == segment
                    && (region.bus_number_start..=region.bus_number_end).contains(&bus)
            })
            .unwrap();

        let addr = region.base_address as usize
            + ((((bus - region.bus_number_start) as usize) << 20)
                | ((device as usize) << 15)
                | ((function as usize) << 12));

        addr + (offset as usize)
    }
}

impl lai::Host for LaiHost {
    fn log(&self, level: lai::LogLevel, message: &str) {
        println!("[LAI {level:?}] {message}");
    }

    fn scan(&self, signature: &str, index: usize) -> *mut u8 {
        let aml_table_ptr = match signature {
            "DSDT" => get_tables().dsdt().map(|aml| aml.address).unwrap_or(0),
            _ => {
                let signature = Signature::from_raw(signature.as_bytes().try_into().unwrap());
                get_tables()
                    .find_sdt(signature, index)
                    .map(|aml| aml.address)
                    .unwrap_or(0)
            }
        };

        if aml_table_ptr == 0 {
            core::ptr::null_mut()
        } else {
            let sdt_ptr = aml_table_ptr - core::mem::size_of::<SdtHeader>();
            sdt_ptr as _
        }
    }

    fn outb(&self, port: u16, value: u8) {
        unsafe { Port::new(port).write(value) }
    }

    fn outw(&self, port: u16, value: u16) {
        unsafe { Port::new(port).write(value) }
    }

    fn outd(&self, port: u16, value: u32) {
        unsafe { Port::new(port).write(value) }
    }

    fn inb(&self, port: u16) -> u8 {
        unsafe { Port::new(port).read() }
    }

    fn inw(&self, port: u16) -> u16 {
        unsafe { Port::new(port).read() }
    }

    fn ind(&self, port: u16) -> u32 {
        unsafe { Port::new(port).read() }
    }

    fn pci_readb(&self, seg: u16, bus: u8, slot: u8, fun: u8, offset: u16) -> u8 {
        unsafe { (self.pci_address(seg, bus, slot, fun, offset) as *const u8).read_volatile() }
    }

    fn pci_readw(&self, seg: u16, bus: u8, slot: u8, fun: u8, offset: u16) -> u16 {
        unsafe { (self.pci_address(seg, bus, slot, fun, offset) as *const u16).read_volatile() }
    }

    fn pci_readd(&self, seg: u16, bus: u8, slot: u8, fun: u8, offset: u16) -> u32 {
        unsafe { (self.pci_address(seg, bus, slot, fun, offset) as *const u32).read_volatile() }
    }
}
