use crate::arch::{translate_phys_addr, translate_virt_addr, PciDevice};
use alloc::{
    alloc::{alloc, dealloc, Layout},
    sync::Arc,
};
use core::{marker::PhantomData, pin::Pin};
use pci_types::Bar;
use spin::Mutex;
use x86_64::{PhysAddr, VirtAddr};

#[derive(Copy, Clone)]
#[repr(usize)]
enum Register {
    Control = 0x0,
    Status = 0x8,

    Eeprom = 0x14,

    ICause = 0xc0,
    IMask = 0xd0,

    RCtrl = 0x100,
    RxDescLo = 0x2800,
    RxDescHi = 0x2804,
    RxDescLen = 0x2808,
    RxDescHead = 0x2810,
    RxDescTail = 0x2818,

    TCtrl = 0x400,
    TxDesLo = 0x3800,
    TxDesHi = 0x3804,
    TxDescLen = 0x3808,
    TxDescHead = 0x3810,
    TxDescTail = 0x3818,
}

bitflags::bitflags! {
    struct ControlFlags: u32 {
        const LRST    = 1 << 3;
        const ASDE    = 1 << 5;
        const SLU     = 1 << 6;
        const ILOS    = 1 << 7;
        const RST     = 1 << 26;
        const VME     = 1 << 30;
        const PHY_RST = 1 << 31;
    }
}

bitflags::bitflags! {
    #[derive(Default, Debug, Clone, Copy)]
    struct TStatus: u8 {
        const DD = 1 << 0; // Descriptor Done
        const EC = 1 << 1; // Excess Collisions
        const LC = 1 << 2; // Late Collision
        const TU = 1 << 3; // Transmit Underrun
    }
}

bitflags::bitflags! {
    #[derive(Default, Clone, Copy, Debug)]
    struct RStatus: u8 {
        const DD    = 1 << 0; // Descriptor Done
        const EOP   = 1 << 1; // End of Packet
        const IXSM  = 1 << 2; // Ignore Checksum
        const VP    = 1 << 3; // 802.1Q
        const RSV   = 1 << 4; // Reserved
        const TCPCS = 1 << 5; // TCP Checksum Calculated on Packet
        const IPCS  = 1 << 6; // IP Checksum Calculated on Packet
        const PIF   = 1 << 7; // Passed in-exact Filter
    }
}

bitflags::bitflags! {
    struct ECtl: u32 {
        const LRST    = 1 << 3;
        const ASDE    = 1 << 5;
        const SLU     = 1 << 6; // Set Link Up
        const ILOS    = 1 << 7;
        const RST     = 1 << 26;
        const VME     = 1 << 30;
        const PHY_RST = 1 << 31;
    }
}

bitflags::bitflags! {
    struct TCtl: u32 {
        const EN     = 1 << 1;  // Transmit Enable
        const PSP    = 1 << 3;  // Pad Short Packets
        const SWXOFF = 1 << 22; // Software XOFF Transmission
        const RTLC   = 1 << 24; // Re-transmit on Late Collision
    }
}

impl TCtl {
    fn set_collision_threshold(&mut self, value: u8) {
        *self = Self::from_bits_retain(self.bits() | ((value as u32) << 4));
    }

    fn set_collision_distance(&mut self, value: u8) {
        *self = Self::from_bits_retain(self.bits() | ((value as u32) << 12));
    }
}

bitflags::bitflags! {
    struct RCtl: u32 {
        const EN            = 1 << 1;  // Receiver Enable
        const SBP           = 1 << 2;  // Store Bad Packets
        const UPE           = 1 << 3;  // Unicast Promiscuous Enabled
        const MPE           = 1 << 4;  // Multicast Promiscuous Enabled
        const LPE           = 1 << 5;  // Long Packet Reception Enable
        const LBM_NONE      = 0 << 6;  // No Loopback
        const LBM_PHY       = 3 << 6;  // PHY or external SerDesc loopback
        const RDMTS_HALF    = 0 << 8;  // Free Buffer Threshold is 1/2 of RDLEN
        const RDMTS_QUARTER = 1 << 8;  // Free Buffer Threshold is 1/4 of RDLEN
        const RDMTS_EIGHTH  = 2 << 8;  // Free Buffer Threshold is 1/8 of RDLEN
        const MO_36         = 0 << 12; // Multicast Offset - bits 47:36
        const MO_35         = 1 << 12; // Multicast Offset - bits 46:35
        const MO_34         = 2 << 12; // Multicast Offset - bits 45:34
        const MO_32         = 3 << 12; // Multicast Offset - bits 43:32
        const BAM           = 1 << 15; // Broadcast Accept Mode
        const VFE           = 1 << 18; // VLAN Filter Enable
        const CFIEN         = 1 << 19; // Canonical Form Indicator Enable
        const CFI           = 1 << 20; // Canonical Form Indicator Bit Value
        const DPF           = 1 << 22; // Discard Pause Frames
        const PMCF          = 1 << 23; // Pass MAC Control Frames
        const SECRC         = 1 << 26; // Strip Ethernet CRC

        // Receive Buffer Size - bits 17:16
        const BSIZE_256     = 3 << 16;
        const BSIZE_512     = 2 << 16;
        const BSIZE_1024    = 1 << 16;
        const BSIZE_2048    = 0 << 16;
        const BSIZE_4096    = (3 << 16) | (1 << 25);
        const BSIZE_8192    = (2 << 16) | (1 << 25);
        const BSIZE_16384   = (1 << 16) | (1 << 25);
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct InterruptFlags: u32 {
        const TXDW    = 1 << 0;  // Transmit Descriptor Written Back
        const TXQE    = 1 << 1;  // Transmit Queue Empty
        const LSC     = 1 << 2;  // Link Status Change
        const RXDMT0  = 1 << 4;  // Receive Descriptor Minimum Threshold
        const DSW     = 1 << 5;  // Disable SW Write Access
        const RXO     = 1 << 6;  // Receiver Overrun
        const RXT0    = 1 << 7;  // Receiver Timer Interrupt
        const MDAC    = 1 << 9;  // MDIO Access Complete
        const PHYINT  = 1 << 12; // PHY Interrupt
        const LSECPN  = 1 << 14; // MACsec Packet Number
        const TXD_LOW = 1 << 15; // Transmit Descriptor Low Threshold hit
        const SRPD    = 1 << 16; // Small Receive Packet Detected
        const ACK     = 1 << 17; // Receive ACK Frame Detected
        const ECCER   = 1 << 22; // ECC Error
    }
}

#[derive(Copy, Clone, Debug)]
enum Error {
    UnknownBar,
    NoEeprom,
}

struct E1000 {
    base: VirtAddr,
    address: smoltcp::wire::EthernetAddress,

    tx_cur: usize,
    tx_ring: *mut [TxDescriptor; TX_DESC_NUM as usize],

    rx_cur: usize,
    rx_ring: *mut [RxDescriptor; RX_DESC_NUM as usize],

    interrupt_guard: Option<crate::arch::InterruptGuard>,
}

#[derive(Debug)]
#[repr(C, packed)]
struct TxDescriptor {
    pub addr: PhysAddr,
    pub length: u16,
    pub cso: u8,
    pub cmd: TxCommand,
    pub status: TStatus,
    pub css: u8,
    pub special: u16,
}

bitflags::bitflags! {
    /// Section 3.3.3.1
    #[derive(Debug, Clone, Copy)]
    struct TxCommand: u8 {
        const EOP  = 1 << 0; // End of Packet
        const IFCS = 1 << 1; // FCS/CRC
        const IC   = 1 << 2; // Insert Checksum
        const RS   = 1 << 3; // Report Status
        const RSV  = 1 << 4; // RESERVED
        const DEXT = 1 << 5; // Extension(?)
        const VLE  = 1 << 6; // VLAN Packet Enable
        const IDE  = 1 << 7; // Interrupt Delay Enable
    }
}

mycelium_bitfield::bitfield! {
    struct Status<u32> {
        const FD: bool; // Full Duplex
        const LU: bool; // Link Up
        const FUNCTION_ID = 2;
        const TXOFF: bool; // Transmission Paused
        const TBIMODE: bool; // TBI Mode/internal SerDes Indication
        const SPEED = 2; // Link speed setting
        const ASDV = 2; // Auto Speed Detection Value
        const _RESERVED1 = 1;
        const PCI66: bool;
        const BUS64: bool;
        const PCIX_MODE: bool;
        const PCIXSPD = 2;
        const _RESERVED2 = 20;
    }
}

#[derive(Debug)]
#[repr(C, packed)]
struct RxDescriptor {
    pub addr: PhysAddr,
    pub length: u16,
    pub checksum: u16,
    pub status: RStatus,
    pub errors: u8,
    pub special: u16,
}

const TX_DESC_NUM: u32 = 32;
const TX_DESC_SIZE: u32 = TX_DESC_NUM * core::mem::size_of::<TxDescriptor>() as u32;
const RX_DESC_NUM: u32 = 32;
const RX_DESC_SIZE: u32 = RX_DESC_NUM * core::mem::size_of::<RxDescriptor>() as u32;

impl E1000 {
    fn new(header: &PciDevice) -> Result<Pin<Box<Self>>, Error> {
        header.enable_bus_mastering();
        header.enable_mmio();

        let registers_addr = match header.bars[0] {
            Some(Bar::Memory64 { address, .. }) => PhysAddr::new(address),
            Some(Bar::Memory32 { address, .. }) => PhysAddr::new(address as u64),
            _ => return Err(Error::UnknownBar),
        };

        let mut this = Box::new(Self {
            base: VirtAddr::new(registers_addr.as_u64() + header.physical_offset as u64),
            address: smoltcp::wire::EthernetAddress([0; 6]),

            tx_cur: 0,
            tx_ring: core::ptr::null_mut(),

            rx_cur: 0,
            rx_ring: core::ptr::null_mut(),

            interrupt_guard: None,
        });

        this.reset();

        if !this.detect_eeprom() {
            return Err(Error::NoEeprom);
        }

        for i in 0..3 {
            let x = this.read_eeprom(i) as u16;
            this.address.0[i as usize * 2] = (x & 0xff) as u8;
            this.address.0[i as usize * 2 + 1] = (x >> 8) as u8;
        }

        this.init_tx()?;
        this.init_rx()?;

        for i in 0..128 {
            this.write_raw(0x5200 + i * 4, 0);
        }

        this.write(
            Register::IMask,
            (InterruptFlags::TXDW
                | InterruptFlags::TXQE
                | InterruptFlags::LSC
                | InterruptFlags::RXDMT0
                | InterruptFlags::DSW
                | InterruptFlags::RXO
                | InterruptFlags::RXT0
                | InterruptFlags::MDAC
                | InterruptFlags::PHYINT
                | InterruptFlags::LSECPN
                | InterruptFlags::TXD_LOW
                | InterruptFlags::SRPD
                | InterruptFlags::ACK
                | InterruptFlags::ECCER)
                .bits(),
        );
        this.read(Register::ICause);

        let this_raw = &mut *this as *mut E1000;
        let mut this = Box::into_pin(this);

        let gsi = crate::arch::pci_route_pin(header);
        this.interrupt_guard = Some(crate::arch::set_interrupt_dyn(
            gsi,
            Box::new(move || {
                let this = unsafe { &mut *this_raw };
                this.handle_irq();
            }),
        ));

        this.link_up();

        Ok(this)
    }

    fn init_tx(&mut self) -> Result<(), Error> {
        let addr = unsafe { alloc(Layout::from_size_align(4096, 4096).unwrap()) };

        let descriptors = unsafe { &mut *(addr as *mut [TxDescriptor; TX_DESC_NUM as usize]) };

        for desc in descriptors {
            *desc = TxDescriptor {
                addr: PhysAddr::zero(),
                length: 0,
                cso: 0,
                cmd: TxCommand::empty(),
                status: TStatus::empty(),
                css: 0,
                special: 0,
            };
        }

        self.tx_ring = addr as _;
        let phys = translate_virt_addr(VirtAddr::new(self.tx_ring as _)).unwrap();

        self.write(Register::TxDesLo, phys.as_u64() as _);
        self.write(Register::TxDesHi, (phys.as_u64() >> 32) as _);
        self.write(Register::TxDescLen, TX_DESC_SIZE);
        self.write(Register::TxDescHead, 0);
        self.write(Register::TxDescTail, 0);

        let mut flags = TCtl::from_bits_retain(1 << 28) | TCtl::EN | TCtl::PSP | TCtl::RTLC;
        flags.set_collision_distance(64);
        flags.set_collision_threshold(15);

        self.write(Register::TCtrl, flags.bits());

        // TODO: TIPG register

        Ok(())
    }

    fn init_rx(&mut self) -> Result<(), Error> {
        let addr = unsafe { alloc(Layout::from_size_align(4096, 4096).unwrap()) };

        let descriptors = unsafe { &mut *(addr as *mut [RxDescriptor; RX_DESC_NUM as usize]) };

        for desc in descriptors {
            let recv_buffer = unsafe { alloc(Layout::from_size_align(4096, 4096).unwrap()) };

            *desc = RxDescriptor {
                addr: translate_virt_addr(VirtAddr::new(recv_buffer as _)).unwrap(),
                length: 0,
                checksum: 0,
                status: RStatus::empty(),
                errors: 0,
                special: 0,
            };
        }

        self.rx_ring = addr as _;
        let phys = translate_virt_addr(VirtAddr::new(self.rx_ring as _)).unwrap();

        self.write(Register::RxDescLo, phys.as_u64() as _);
        self.write(Register::RxDescHi, (phys.as_u64() >> 32) as _);
        self.write(Register::RxDescLen, RX_DESC_SIZE);
        self.write(Register::RxDescHead, 0);
        self.write(Register::RxDescTail, RX_DESC_NUM - 1);

        let flags = RCtl::EN
            | RCtl::SBP
            | RCtl::UPE
            | RCtl::LPE
            | RCtl::MPE
            | RCtl::LBM_NONE
            | RCtl::RDMTS_EIGHTH
            | RCtl::BAM
            | RCtl::SECRC
            | RCtl::BSIZE_4096;

        self.write(Register::RCtrl, flags.bits());
        Ok(())
    }

    fn detect_eeprom(&self) -> bool {
        self.write(Register::Eeprom, 1);

        for _ in 0..1000 {
            let value = self.read(Register::Eeprom);

            if value & (1 << 4) > 0 {
                return true;
            }
        }

        false
    }

    fn read_eeprom(&self, addr: u8) -> u32 {
        self.write(Register::Eeprom, 1 | ((addr as u32) << 8));

        loop {
            let res = self.read(Register::Eeprom);

            if res & (1 << 4) > 0 {
                return (res >> 16) & 0xffff;
            }
        }
    }

    fn reset(&self) {
        self.insert_flags(Register::Control, ControlFlags::RST.bits());

        while ControlFlags::from_bits_truncate(self.read(Register::Control))
            .contains(ControlFlags::RST)
        {
            core::hint::spin_loop();
        }

        self.remove_flags(
            Register::Control,
            (ControlFlags::LRST | ControlFlags::PHY_RST | ControlFlags::VME).bits(),
        );
    }

    fn status(&self) -> Status {
        Status::from_bits(self.read(Register::Status))
    }

    fn link_up(&self) {
        self.insert_flags(Register::Control, ECtl::SLU.bits());

        while !self.status().get(Status::LU) {
            core::hint::spin_loop();
        }
    }

    fn handle_irq(&mut self) {
        InterruptFlags::from_bits_retain(self.read(Register::ICause));
    }

    fn send(&mut self, packet: &[u8]) {
        let cur = self.tx_cur;
        let ring = self.tx_ring();

        ring[cur].addr = translate_virt_addr(VirtAddr::new(packet.as_ptr().addr() as _)).unwrap();
        ring[cur].length = packet.len() as _;
        ring[cur].cmd = TxCommand::RS | TxCommand::IFCS | TxCommand::EOP;
        ring[cur].status = TStatus::empty();

        self.tx_cur = (self.tx_cur + 1) % TX_DESC_NUM as usize;

        self.write(Register::TxDescTail, self.tx_cur as u32);
    }

    fn recv(&mut self) -> Option<(usize, *mut [u8])> {
        let id = self.rx_cur;
        let desc = &mut self.rx_ring()[id];

        if !desc.status.contains(RStatus::DD) {
            return None;
        }

        Some((
            id,
            core::ptr::from_raw_parts_mut(
                translate_phys_addr(desc.addr).as_u64() as _,
                desc.length as usize,
            ),
        ))
    }

    fn recv_end(&mut self, id: usize) {
        let desc = &mut self.rx_ring()[id];

        assert!(desc.status.contains(RStatus::DD));

        desc.status = RStatus::empty();

        let old = self.rx_cur;
        self.rx_cur = (self.rx_cur + 1) % RX_DESC_NUM as usize;
        self.write(Register::RxDescTail, old as u32);
    }

    fn rx_ring(&mut self) -> &mut [RxDescriptor] {
        unsafe { &mut *self.rx_ring }
    }

    fn tx_ring(&mut self) -> &mut [TxDescriptor] {
        unsafe { &mut *self.tx_ring }
    }

    fn remove_flags(&self, register: Register, flag: u32) {
        self.write(register, self.read(register) & !flag);
    }

    fn insert_flags(&self, register: Register, flag: u32) {
        self.write(register, self.read(register) | flag);
    }

    fn read(&self, register: Register) -> u32 {
        unsafe {
            let register = self.base.as_ptr::<u8>().add(register as usize);
            core::ptr::read_volatile(register as *const u32)
        }
    }

    fn write(&self, register: Register, value: u32) {
        self.write_raw(register as _, value);
    }

    fn write_raw(&self, register: u32, value: u32) {
        unsafe {
            let register = self.base.as_mut_ptr::<u8>().add(register as usize);
            core::ptr::write_volatile(register as *mut u32, value);
        }
    }
}

unsafe impl Send for E1000 {}
unsafe impl Sync for E1000 {}

impl Drop for E1000 {
    fn drop(&mut self) {
        unsafe {
            dealloc(
                self.tx_ring as _,
                Layout::from_size_align(4096, 4096).unwrap(),
            );
            for rx_desc in self.rx_ring() {
                let addr = translate_phys_addr(rx_desc.addr).as_u64() as _;
                dealloc(addr, Layout::from_size_align(4096, 4096).unwrap());
            }
            dealloc(
                self.rx_ring as _,
                Layout::from_size_align(4096, 4096).unwrap(),
            );
        };
    }
}

pub struct Driver {
    inner: Arc<Mutex<Pin<Box<E1000>>>>,
}

impl crate::net::Driver for Driver {
    fn address(&self) -> smoltcp::wire::HardwareAddress {
        self.inner.lock().address.into()
    }
}

pub struct TxToken {
    inner: Arc<Mutex<Pin<Box<E1000>>>>,
}

impl smoltcp::phy::TxToken for TxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buffer = vec![0; len];
        let result = f(&mut buffer);
        crate::arch::without_interrupts(|| {
            self.inner.lock().send(&buffer);
        });
        result
    }
}

pub struct RxToken<'a> {
    inner: Arc<Mutex<Pin<Box<E1000>>>>,
    id: usize,
    buffer: *mut [u8],
    phantom: PhantomData<&'a [u8]>,
}

impl<'a> smoltcp::phy::RxToken for RxToken<'a> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let result = f(unsafe { &mut *self.buffer });
        crate::arch::without_interrupts(|| {
            self.inner.lock().recv_end(self.id);
        });
        result
    }
}

impl smoltcp::phy::Device for Driver {
    type RxToken<'a> = RxToken<'a>
    where
        Self: 'a;
    type TxToken<'a> = TxToken
    where
        Self: 'a;

    fn receive(
        &mut self,
        _timestamp: smoltcp::time::Instant,
    ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let mut e1000 = self.inner.lock();
        let (id, buffer) = e1000.recv()?;
        Some((
            RxToken {
                id,
                buffer,
                inner: self.inner.clone(),
                phantom: PhantomData,
            },
            TxToken {
                inner: self.inner.clone(),
            },
        ))
    }

    fn transmit(&mut self, _timestamp: smoltcp::time::Instant) -> Option<Self::TxToken<'_>> {
        Some(TxToken {
            inner: self.inner.clone(),
        })
    }

    fn capabilities(&self) -> smoltcp::phy::DeviceCapabilities {
        let mut capabilities = smoltcp::phy::DeviceCapabilities::default();
        capabilities.medium = smoltcp::phy::Medium::Ethernet;
        capabilities.max_transmission_unit = 1500;
        capabilities.max_burst_size = Some(RX_DESC_NUM as _);
        capabilities
    }
}

pub fn init(header: &PciDevice) -> bool {
    if header.vendor_id != 0x8086 || header.device_id != 0x100e {
        return false;
    }

    let e1000 = E1000::new(header).unwrap();
    let driver = Driver {
        inner: Arc::new(Mutex::new(e1000)),
    };

    crate::net::register(driver);

    true
}
