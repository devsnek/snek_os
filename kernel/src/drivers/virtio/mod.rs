use super::dma::Dma;
use crate::arch::{map_address, translate_phys_addr, translate_virt_addr, PciDevice};
use virtio_drivers::transport::pci::{
    bus::{Command, DeviceFunction},
    PciTransport,
};
use x86_64::{PhysAddr, VirtAddr};

pub mod net;
pub mod sock;

struct Hal;

unsafe impl virtio_drivers::Hal for Hal {
    fn dma_alloc(
        pages: usize,
        _direction: virtio_drivers::BufferDirection,
    ) -> (virtio_drivers::PhysAddr, core::ptr::NonNull<u8>) {
        let dma = Dma::<[u8]>::new_zeroed_slice(pages * 4096, 4096);
        let phys = dma.phys_addr();
        let virt = core::ptr::NonNull::new(dma.leak() as _).unwrap();
        (phys, virt)
    }

    unsafe fn dma_dealloc(
        _paddr: virtio_drivers::PhysAddr,
        _vaddr: core::ptr::NonNull<u8>,
        _pages: usize,
    ) -> i32 {
        0
    }

    unsafe fn mmio_phys_to_virt(
        paddr: virtio_drivers::PhysAddr,
        size: usize,
    ) -> core::ptr::NonNull<u8> {
        core::ptr::NonNull::new(map_address(PhysAddr::new(paddr as _), size).as_u64() as _).unwrap()
    }

    unsafe fn share(
        buffer: core::ptr::NonNull<[u8]>,
        _direction: virtio_drivers::BufferDirection,
    ) -> virtio_drivers::PhysAddr {
        translate_virt_addr(VirtAddr::new(buffer.as_ptr() as *mut u8 as _))
            .unwrap()
            .as_u64() as _
    }

    unsafe fn unshare(
        _paddr: virtio_drivers::PhysAddr,
        _buffer: core::ptr::NonNull<[u8]>,
        _direction: virtio_drivers::BufferDirection,
    ) {
    }
}

struct ConfigAccess {
    segment: u16,
    regions: acpi::PciConfigRegions<'static, alloc::alloc::Global>,
}

impl ConfigAccess {
    fn physical_address(&self, bus: u8, device: u8, function: u8, offset: u16) -> u64 {
        let addr = self
            .regions
            .physical_address(self.segment, bus, device, function)
            .unwrap();
        addr + (offset as u64)
    }
}

impl virtio_drivers::transport::pci::bus::ConfigurationAccess for ConfigAccess {
    fn read_word(
        &self,
        device_function: virtio_drivers::transport::pci::bus::DeviceFunction,
        register_offset: u8,
    ) -> u32 {
        let addr = self.physical_address(
            device_function.bus,
            device_function.device,
            device_function.function,
            register_offset as _,
        );
        let addr = translate_phys_addr(PhysAddr::new(addr)).as_u64();
        unsafe { (addr as *mut u32).read_volatile() }
    }

    fn write_word(
        &mut self,
        device_function: virtio_drivers::transport::pci::bus::DeviceFunction,
        register_offset: u8,
        data: u32,
    ) {
        let addr = self.physical_address(
            device_function.bus,
            device_function.device,
            device_function.function,
            register_offset as _,
        );
        let addr = translate_phys_addr(PhysAddr::new(addr)).as_u64();
        unsafe {
            (addr as *mut u32).write_volatile(data);
        }
    }

    unsafe fn unsafe_clone(&self) -> Self {
        Self {
            segment: self.segment,
            regions: crate::arch::get_pci_config_regions(),
        }
    }
}

pub fn create_transport(header: &PciDevice) -> Result<PciTransport, anyhow::Error> {
    let config_access = ConfigAccess {
        segment: header.address.segment(),
        regions: crate::arch::get_pci_config_regions(),
    };

    let mut root = virtio_drivers::transport::pci::bus::PciRoot::new(config_access);

    let device = DeviceFunction {
        bus: header.address.bus(),
        device: header.address.device(),
        function: header.address.function(),
    };

    root.set_command(
        device,
        Command::IO_SPACE | Command::MEMORY_SPACE | Command::BUS_MASTER,
    );

    Ok(PciTransport::new::<Hal, _>(&mut root, device)?)
}
