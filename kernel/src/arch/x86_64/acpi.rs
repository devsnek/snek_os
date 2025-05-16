use super::interrupts::{set_interrupt_static, InterruptType};
use crate::{arch::PciDevice, stack_allocator::StackAllocator};
use acpi::{
    fadt::Fadt,
    hpet::HpetInfo,
    mcfg::PciConfigRegions,
    platform::PlatformInfo,
    sdt::{SdtHeader, Signature},
    AcpiHandler, AcpiTables, PhysicalMapping,
};
use conquer_once::spin::OnceCell;
use core::convert::TryInto;
use x86_64::{instructions::port::Port, VirtAddr};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0:?}")]
    Lai(lai::Error),
}

pub type AcpiAllocator = StackAllocator<2048>;

struct AcpiHolder(AcpiTables<AcpiHandlerImpl>);

unsafe impl Sync for AcpiHolder {}

static ACPI_TABLES: OnceCell<AcpiHolder> = OnceCell::uninit();

fn get_tables() -> &'static AcpiTables<AcpiHandlerImpl> {
    &ACPI_TABLES.get().unwrap().0
}

pub fn early_init(
    allocator: &AcpiAllocator,
    rsdp_address: VirtAddr,
) -> PlatformInfo<'_, &AcpiAllocator> {
    ACPI_TABLES
        .try_init_once(|| unsafe {
            AcpiHolder(AcpiTables::from_rsdp(AcpiHandlerImpl, rsdp_address.as_u64() as _).unwrap())
        })
        .unwrap();

    debug!("[ACPI] early initialized");

    get_tables().platform_info_in(allocator).unwrap()
}

static HOST: Host = Host;

pub fn late_init() {
    lai::init(&HOST);

    lai::set_acpi_revision(get_tables().revision() as _);
    lai::create_namespace();

    let fadt = get_tables().find_table::<Fadt>().unwrap();
    core::mem::forget(set_interrupt_static(
        fadt.sci_interrupt as _,
        InterruptType::LevelLow,
        handle_interrupt,
    ));

    lai::enable_acpi(lai::PICMethod::APIC);

    debug!("[ACPI] late initialized");
}

fn handle_interrupt() {
    let event = lai::get_sci_event();
    if event.contains(lai::SciEvent::POWER_BUTTON) {
        shutdown();
    }
}

pub fn pci_route_pin(device: &PciDevice) -> Result<u8, Error> {
    let resource = lai::pci_route_pin(
        device.address.segment(),
        device.address.bus(),
        device.address.device(),
        device.address.function(),
        device.interrupt_pin,
    )
    .map_err(Error::Lai)?;
    Ok(resource.base as _)
}

pub fn shutdown() {
    lai::enter_sleep(lai::SleepState::Shutdown).unwrap();
}

pub fn reboot() {
    lai::reset().unwrap();
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

struct Host;

impl Host {
    fn pci_address(&self, segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> usize {
        let base = get_pci_config_regions()
            .physical_address(segment, bus, device, function)
            .unwrap() as usize;
        base + (offset as usize)
    }
}

impl lai::Host for Host {
    fn log(&self, level: lai::LogLevel, message: &str) {
        match level {
            lai::LogLevel::Debug => debug!("[LAI] {}", message),
            lai::LogLevel::Warn => warn!("[LAI] {}", message),
        }
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

    fn pci_writeb(&self, seg: u16, bus: u8, slot: u8, fun: u8, offset: u16, value: u8) {
        unsafe { (self.pci_address(seg, bus, slot, fun, offset) as *mut u8).write_volatile(value) }
    }

    fn pci_writew(&self, seg: u16, bus: u8, slot: u8, fun: u8, offset: u16, value: u16) {
        unsafe { (self.pci_address(seg, bus, slot, fun, offset) as *mut u16).write_volatile(value) }
    }

    fn pci_writed(&self, seg: u16, bus: u8, slot: u8, fun: u8, offset: u16, value: u32) {
        unsafe { (self.pci_address(seg, bus, slot, fun, offset) as *mut u32).write_volatile(value) }
    }

    fn map(&self, address: usize, _count: usize) -> *mut u8 {
        address as _
    }

    fn unmap(&self, _address: usize, _count: usize) {}

    fn sleep(&self, _ms: u64) {}

    fn timer(&self) -> u64 {
        unimplemented!()
    }
}

pub fn get_pci_config_regions() -> PciConfigRegions<'static, alloc::alloc::Global> {
    PciConfigRegions::new(get_tables()).unwrap()
}

pub fn get_century_register() -> u8 {
    let fadt = get_tables().find_table::<Fadt>().unwrap();
    fadt.century
}

pub fn get_platform_info<G: alloc::alloc::Allocator>(g: &G) -> PlatformInfo<'static, &G> {
    get_tables().platform_info_in(g).unwrap()
}

pub fn get_hpet() -> Option<HpetInfo> {
    HpetInfo::new(get_tables()).ok()
}
