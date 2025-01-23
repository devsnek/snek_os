use alloc::collections::btree_map::Entry;
use core::sync::atomic::{AtomicUsize, Ordering};
use pci_ids::{Device as DeviceInfo, Subclass as SubclassInfo};
use pci_types::{Bar, PciAddress};
use snalloc::Allocator;
use spin::Lazy;

#[global_allocator]
pub static ALLOCATOR: Allocator = Allocator::new();

pub struct Local<T> {
    id: Lazy<usize>,
    initializer: fn() -> T,
}

impl<T: 'static> Local<T> {
    pub const fn new(initializer: fn() -> T) -> Self {
        Self {
            id: Lazy::new(Self::next_id),
            initializer,
        }
    }

    fn next_id() -> usize {
        static ID: AtomicUsize = AtomicUsize::new(0);
        ID.fetch_add(1, Ordering::Relaxed)
    }

    pub fn with<U, F>(&self, f: F) -> U
    where
        F: FnOnce(&T) -> U,
    {
        let local = super::LocalData::get().expect("failed to load GsLocalData");
        let ptr = match local.data.lock().entry(*self.id) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(v) => {
                let ptr = Box::into_raw(Box::new((self.initializer)())) as *mut ();
                v.insert(ptr);
                ptr
            }
        };
        let data = unsafe { &*(ptr as *const T) };
        f(data)
    }
}

#[derive(Debug)]
pub struct PciDevice {
    pub physical_offset: usize,
    pub configuration_address: usize,
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
    pub interrupt_pin: u8,
    pub interrupt_line: u8,
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

    unsafe fn read<T: Sized + Copy>(&self, offset: u16) -> T {
        ((self.configuration_address + offset as usize) as *const T).read_volatile()
    }

    unsafe fn write<T: Sized + Copy>(&self, offset: u16, value: T) {
        ((self.configuration_address + offset as usize) as *mut T).write_volatile(value)
    }

    pub fn enable_mmio(&self) {
        let command = unsafe { self.read::<u16>(0x04) };
        unsafe { self.write::<u16>(0x04, command | (1 << 1)) }
    }

    pub fn enable_bus_mastering(&self) {
        let command = unsafe { self.read::<u16>(0x04) };
        unsafe { self.write::<u16>(0x04, command | (1 << 2)) }
    }
}
