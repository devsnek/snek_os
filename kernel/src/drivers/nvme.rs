use super::dma::Dma;
use super::PciDevice;
use crate::arch::{map_address, translate_virt_addr};
use core::sync::atomic::{AtomicUsize, Ordering};
use pci_types::Bar;
use x86_64::{PhysAddr, VirtAddr};

#[derive(Debug)]
enum Error {
    UnknownBar,
    NotSupported,
    Fatal,
    CommandError(u16),
}

trait Register: Sized + Copy {
    const REG: u32;
}

const REG_INTMS: u32 = 0x0C;
const REG_INTMC: u32 = 0x10;
const REG_Q_DBL_BASE: u32 = 0x1000;

mycelium_bitfield::bitfield! {
    struct Capabilities<u64> {
        /// Maxium Queue Entries Supported
        const MQES: u16;
        /// Contiguous Queues Required
        const CQR: bool;
        /// Arbitration Mechanism Supported
        const AMS_WEIGHTED_ROUND_ROBIN: bool;
        const AMS_VENDOR_SPECIFIC: bool;
        const _RESERVED0 = 5;
        /// Timeout
        const TO: u8;
        /// Doorbell Stride
        const DSTRD = 4;
        /// NVME Subsystem Reset Supported
        const NSSRS: bool;
        /// Command Sets Supported
        const CSS_NVM: bool;
        const _RESERVED1 = 5;
        const CSS_IO: bool;
        const CSS_ADMIN: bool;
        /// Boot Partition Support
        const BPS: bool;
        /// Controller Power Scope
        const CPS = 2;
        /// Memory Page Size Minimum
        const MPSMIN = 4;
        /// Memory Page Size Maximum
        const MPSMAX = 4;
        /// Persistent Memory Region Supported
        const PMRS: bool;
        /// Controller Memory Buffer Supported
        const CMBS: bool;
        /// NVM Subsystem Shutdown Supported
        const NSSS: bool;
        /// Controller Ready Modes Supported
        const CRMS_CRWMS: bool;
        const CRMS_CRIMS: bool;
    }
}

impl Register for Capabilities {
    const REG: u32 = 0x00;
}

mycelium_bitfield::bitfield! {
    struct Version<u32> {
        const TER: u8;
        const MNR: u8;
        const MJR: u16;
    }
}

impl Register for Version {
    const REG: u32 = 0x08;
}

mycelium_bitfield::bitfield! {
    struct ControllerConfiguration<u32> {
        /// Enable
        const EN: bool;
        const _RESERVED0 = 3;
        /// I/O Command Set Selected
        const CSS = 3;
        /// Memory Page Size
        const MPS = 4;
        /// Arbitration Mechanism Selected
        const AMS = 3;
        /// Shutdown Notification
        const SHN = 2;
        /// I/O Submission Queue Entry Size
        const IOSQES = 4;
        /// I/O Completion Queue Entry Size
        const IOCQES = 4;
        /// Controller Ready Independent of Media Enable
        const CRIME: bool;
    }
}

impl Register for ControllerConfiguration {
    const REG: u32 = 0x14;
}

mycelium_bitfield::bitfield! {
    struct ControllerStatus<u32> {
        /// Ready
        const RDY: bool;
        /// Controller Fatal Status
        const CFS: bool;
        /// Shutdown Status
        const SHST = 2;
        /// NVME Subsystem Reset Occured
        const NSSRO: bool;
        /// Processing Paused
        const PP: bool;
        /// Shutdown Type
        const ST: bool;
    }
}

impl Register for ControllerStatus {
    const REG: u32 = 0x1C;
}

mycelium_bitfield::bitfield! {
    struct AdminQueueAttributes<u32> {
        /// Admin Submission Queue Size
        const ASQS = 12;
        const _RESERVED0 = 4;
        /// Admin COmpletion Queue Size
        const ACQS = 12;
        const _RESERVED1 = 4;
    }
}

impl Register for AdminQueueAttributes {
    const REG: u32 = 0x24;
}

#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
struct AdminSubmissionQueue(u64);

impl Register for AdminSubmissionQueue {
    const REG: u32 = 0x28;
}

#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
struct AdminCompletionQueue(u64);

impl Register for AdminCompletionQueue {
    const REG: u32 = 0x30;
}

enum ControllerCommandSet {
    Nvm = 0b000,
    Io = 0b110,
    Admin = 0b111,
}

enum ArbitrationMechanism {
    RoundRobin = 0b000,
    WeightedRoundRobinWithUrgentPriorityClass = 0b001,
    VendorSpecific = 0b111,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct CompletionQueueEntry {
    pub result: u32, // Used by admin commands to return data.
    pub reserved: u32,
    pub sq_head: u16,    // Portion of the queue that may be reclaimed
    pub sq_id: u16,      // Submission Queue that generated this entry.
    pub command_id: u16, // Command ID of the command that was completed.
    pub status: u16,     // Reason why the command failed, if it did.
}

static QUEUE_ID: AtomicUsize = AtomicUsize::new(0);

struct QueuePair {
    registers: Registers,
    queue_id: usize,
    command_id: u16,
    submission: Dma<[Command]>,
    submission_index: usize,
    completion: Dma<[CompletionQueueEntry]>,
    completion_index: usize,
    phase: bool,
}

impl QueuePair {
    fn new(registers: Registers, size: usize) -> Self {
        let queue_id = QUEUE_ID.fetch_add(1, Ordering::SeqCst);
        Self {
            registers,
            queue_id,
            command_id: 0,
            submission: Dma::new_zeroed_slice(size, 4096),
            submission_index: 0,
            completion: Dma::new_zeroed_slice(size, 4096),
            completion_index: 0,
            phase: true,
        }
    }

    fn submit(&mut self, mut command: Command) -> Result<CompletionQueueEntry, Error> {
        unsafe {
            *(&mut command as *mut Command as *mut u16).offset(1) = self.command_id;
        }
        self.command_id += 1;
        self.submission[self.submission_index] = command;
        self.submission_index = (self.submission_index + 1) % self.submission.len();
        self.set_doorbell(0, self.submission_index as _);

        let cmd = &self.completion[self.completion_index];
        while (cmd.status & 0x1) != self.phase as u16 {
            core::hint::spin_loop();
        }
        let cmd = self.completion[self.completion_index];
        let status = cmd.status >> 1;
        if status != 0 {
            return Err(Error::CommandError(status));
        }
        self.completion_index = (self.completion_index + 1) % self.completion.len();
        if self.completion_index == 0 {
            self.phase = !self.phase;
        }
        self.set_doorbell(1, self.completion_index as _);

        Ok(cmd)
    }

    fn set_doorbell(&mut self, doorbell: usize, index: u32) {
        let cap = self.registers.read::<Capabilities>();
        let doorbell_stride = cap.get(Capabilities::DSTRD);
        let offset = 0x1000 + (((self.queue_id * 2) + doorbell) * (4 << doorbell_stride));
        let doorbell_addr = self.registers.addr + offset;
        unsafe {
            core::ptr::write_volatile(doorbell_addr.as_u64() as *mut u32, index);
        }
    }
}

#[derive(Clone, Copy)]
struct Registers {
    addr: VirtAddr,
}

impl Registers {
    fn set_enable(&self, enabled: bool) {
        let mut c = self.read::<ControllerConfiguration>();
        c.set(ControllerConfiguration::EN, enabled);
        self.write(c);

        loop {
            let s = self.read::<ControllerStatus>();
            if s.get(ControllerStatus::RDY) == enabled {
                break;
            }
            if s.get(ControllerStatus::CFS) {
                break;
            }
            core::hint::spin_loop();
        }
    }

    fn set_css(&self, css: ControllerCommandSet) {
        let mut c = self.read::<ControllerConfiguration>();
        c.set(ControllerConfiguration::CSS, css as _);
        self.write(c);
    }

    fn set_ams(&self, ams: ArbitrationMechanism) {
        let mut c = self.read::<ControllerConfiguration>();
        c.set(ControllerConfiguration::AMS, ams as _);
        self.write(c);
    }

    fn set_queue_entry_sizes(&self, iosqes: u8, iocqes: u8) {
        let mut c = self.read::<ControllerConfiguration>();
        c.set(ControllerConfiguration::IOSQES, iosqes as _);
        c.set(ControllerConfiguration::IOCQES, iocqes as _);
        self.write(c);
    }

    fn set_admin_queue_attributes(&self, asqs: u16, acqs: u16) {
        let mut aqa = AdminQueueAttributes::new();
        aqa.set(AdminQueueAttributes::ASQS, asqs as _);
        aqa.set(AdminQueueAttributes::ACQS, acqs as _);
        self.write(aqa);
    }

    fn read<T: Register>(&self) -> T {
        unsafe { ((self.addr.as_u64() + (T::REG as u64)) as *const T).read_volatile() }
    }

    fn write<T: Register>(&self, value: T) {
        unsafe { ((self.addr.as_u64() + (T::REG as u64)) as *mut T).write_volatile(value) }
    }
}

struct Controller {
    registers: Registers,
}

impl Controller {
    fn new(device: &PciDevice) -> Result<Self, Error> {
        device.enable_bus_mastering();
        device.enable_mmio();

        let (registers_addr, registers_size) = match device.bars[0] {
            Some(Bar::Memory64 { address, size, .. }) => (PhysAddr::new(address), size as usize),
            Some(Bar::Memory32 { address, size, .. }) => {
                (PhysAddr::new(address as u64), size as usize)
            }
            _ => return Err(Error::UnknownBar),
        };

        let registers = Registers {
            addr: map_address(registers_addr, registers_size),
        };

        let cap = registers.read::<Capabilities>();
        if !cap.get(Capabilities::CSS_NVM) {
            return Err(Error::NotSupported);
        }

        registers.set_enable(false);

        // TODO: msix

        let queue_size = cap.get(Capabilities::MQES);

        let mut admin_queue = QueuePair::new(registers, queue_size as _);

        registers.set_admin_queue_attributes(queue_size, queue_size);

        registers.write(AdminSubmissionQueue(
            translate_virt_addr(VirtAddr::new(admin_queue.submission.as_mut_ptr() as _))
                .unwrap()
                .as_u64(),
        ));
        registers.write(AdminCompletionQueue(
            translate_virt_addr(VirtAddr::new(admin_queue.completion.as_mut_ptr() as _))
                .unwrap()
                .as_u64(),
        ));

        registers.set_css(ControllerCommandSet::Nvm);
        registers.set_ams(ArbitrationMechanism::RoundRobin);
        registers.set_queue_entry_sizes(6, 4);

        registers.set_enable(true);

        if registers
            .read::<ControllerStatus>()
            .get(ControllerStatus::CFS)
        {
            return Err(Error::Fatal);
        }

        let mut identity = Dma::<IdentifyController>::new_zeroed(4096);
        let identify = IdentifyCommand {
            controller_id: 0,
            command_id: 0,
            flags: 0,
            nsid: 0,
            opcode: AdminOpcode::Identify as u8,
            cns: IdentifyCns::Controller as u8,
            data_ptr: DataPointer {
                prp1: identity.phys_addr() as _,
                prp2: 0,
            },
            reserved2: [0; 2],
            reserved3: 0,
            reserved11: [0; 5],
        };
        admin_queue.submit(Command { identify })?;

        let mut io_queue = QueuePair::new(registers, queue_size as _);

        let create_cq = CreateCQCommand {
            opcode: AdminOpcode::CreateCq as _,
            flags: 0,
            command_id: 1,
            reserved1: [0; 5],
            prp1: io_queue.completion.phys_addr() as _,
            prp2: 0,
            cqid: io_queue.queue_id as _,
            q_size: (io_queue.completion.len() - 1) as _,
            cq_flags: 1,
            irq_vector: 0,
            reserved2: [0; 4],
        };
        admin_queue.submit(Command { create_cq })?;

        let create_sq = CreateSQCommand {
            opcode: AdminOpcode::CreateSq as _,
            flags: 0,
            command_id: 3,
            reserved1: [0; 5],
            prp1: io_queue.submission.phys_addr() as _,
            prp2: 0,
            sqid: io_queue.queue_id as _,
            q_size: (io_queue.submission.len() - 1) as _,
            sq_flags: 1,
            cqid: io_queue.queue_id as _,
            reserved2: [0; 4],
        };
        admin_queue.submit(Command { create_sq })?;

        let shift = 12 + registers.read::<Capabilities>().get(Capabilities::MPSMIN) as usize;
        let max_transfer_shift = if identity.mdts != 0 {
            shift + identity.mdts as usize
        } else {
            20
        };

        {
            let nsids = Dma::<[u32]>::new_zeroed_slice(identity.nn as usize, 4096);
            let identify = IdentifyCommand {
                controller_id: 0,
                command_id: 4,
                flags: 0,
                nsid: 0,
                opcode: AdminOpcode::Identify as u8,
                cns: IdentifyCns::ActivateList as u8,
                data_ptr: DataPointer {
                    prp1: nsids.phys_addr() as _,
                    prp2: 0,
                },
                reserved2: [0; 2],
                reserved3: 0,
                reserved11: [0; 5],
            };
            admin_queue.submit(Command { identify })?;

            for nsid in nsids.into_iter() {
                if *nsid == 0 {
                    continue;
                }

                println!("{nsid}");

                let identity = Dma::<IdentifyNamespace>::new_zeroed(4096);
                let identify = IdentifyCommand {
                    controller_id: 0,
                    command_id: 5,
                    flags: 0,
                    nsid: *nsid,
                    opcode: AdminOpcode::Identify as _,
                    cns: IdentifyCns::Namespace as _,
                    data_ptr: DataPointer {
                        prp1: identity.phys_addr() as _,
                        prp2: 0,
                    },
                    reserved2: [0; 2],
                    reserved3: 0,
                    reserved11: [0; 5],
                };
                admin_queue.submit(Command { identify })?;

                let blocks = identity.nsze as usize;
                let block_size = 1 << identity.lbaf[(identity.flbas & 0b11111) as usize].ds;

                // The maximum transfer size is in units of 2^(min page size)
                let lba_shift = identity.lbaf[(identity.flbas & 0xf) as usize].ds;
                let max_lbas = 1 << (max_transfer_shift - lba_shift as usize);
                let max_prps = (max_lbas * (1 << lba_shift)) / 4096;

                println!(
                    "nvme: identified namespace (blocks={}, block_size={}, size={})",
                    blocks,
                    block_size,
                    blocks * block_size,
                )
            }
        }

        Ok(Self { registers })
    }
}

pub fn init(device: &PciDevice) -> bool {
    if device.class != 0x1 || device.sub_class != 0x8 {
        return false;
    }

    Controller::new(device).unwrap();

    true
}

#[repr(u8)]
#[derive(Default, Copy, Clone)]
pub enum AdminOpcode {
    CreateSq = 0x1,
    CreateCq = 0x5,
    Identify = 0x6,

    #[default]
    Unknown = u8::MAX,
}

#[repr(u8)]
#[derive(Default, Copy, Clone)]
pub enum IdentifyCns {
    Namespace = 0x00,
    Controller = 0x01,
    ActivateList = 0x2,

    #[default]
    Unknown = u8::MAX,
}

#[derive(Debug, Default, Copy, Clone)]
#[repr(C)]
pub struct DataPointer {
    pub prp1: u64,
    pub prp2: u64,
}

#[derive(Default, Copy, Clone)]
#[repr(C)]
pub struct CommonCommand {
    pub opcode: u8,
    pub flags: u8,
    pub command_id: u16,
    pub namespace_id: u32,
    pub cdw2: [u32; 2],
    pub metadata: u64,
    pub data_ptr: DataPointer,
    pub cdw10: u32,
    pub cdw11: u32,
    pub cdw12: u32,
    pub cdw13: u32,
    pub cdw14: u32,
    pub cdw15: u32,
}

#[derive(Default, Copy, Clone)]
#[repr(C)]
pub struct IdentifyCommand {
    pub opcode: u8,
    pub flags: u8,
    pub command_id: u16,
    pub nsid: u32,
    pub reserved2: [u64; 2],
    pub data_ptr: DataPointer,
    pub cns: u8,
    pub reserved3: u8,
    pub controller_id: u16,
    pub reserved11: [u32; 5],
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct ReadWriteCommand {
    pub opcode: u8,
    pub flags: u8,
    pub command_id: u16,
    pub nsid: u32,
    pub reserved2: u64,
    pub metadata: u64,
    pub data_ptr: DataPointer,
    pub start_lba: u64,
    pub length: u16,
    pub control: u16,
    pub ds_mgmt: u32,
    pub ref_tag: u32,
    pub app_tag: u16,
    pub app_mask: u16,
}

#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct CreateSQCommand {
    pub opcode: u8,
    pub flags: u8,
    pub command_id: u16,
    pub reserved1: [u32; 5],
    pub prp1: u64,
    pub prp2: u64,
    pub sqid: u16,
    pub q_size: u16,
    pub sq_flags: u16,
    pub cqid: u16,
    pub reserved2: [u32; 4],
}

#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct CreateCQCommand {
    pub opcode: u8,
    pub flags: u8,
    pub command_id: u16,
    pub reserved1: [u32; 5],
    pub prp1: u64,
    pub prp2: u64,
    pub cqid: u16,
    pub q_size: u16,
    pub cq_flags: u16,
    pub irq_vector: u16,
    pub reserved2: [u32; 4],
}

#[repr(C)]
pub union Command {
    common: CommonCommand,
    identify: IdentifyCommand,
    rw: ReadWriteCommand,
    create_sq: CreateSQCommand,
    create_cq: CreateCQCommand,
}

#[derive(Debug)]
#[repr(C)]
pub struct PowerState {
    pub max_power: u16,
    pub rsvd1: u8,
    pub flags: u8,
    pub entry_lat: u32,
    pub exit_lat: u32,
    pub read_tput: u8,
    pub read_lat: u8,
    pub write_tput: u8,
    pub write_lat: u8,
    pub idle_power: u16,
    pub idle_scale: u8,
    pub rsvd2: u8,
    pub active_power: u16,
    pub active_work_scale: u8,
    pub rsvd3: [u8; 9],
}

#[derive(Debug)]
#[repr(C)]
pub struct IdentifyController {
    pub vid: u16,
    pub ssvid: u16,
    pub sn: [u8; 20],
    pub mn: [u8; 40],
    pub fr: [u8; 8],
    pub rab: u8,
    pub ieee: [u8; 3],
    pub cmic: u8,
    pub mdts: u8,
    pub cntlid: u16,
    pub ver: u32,
    pub rtd3r: u32,
    pub rtd3e: u32,
    pub oaes: u32,
    pub ctratt: u32,
    pub reserved100: [u8; 28],
    pub crdt1: u16,
    pub crdt2: u16,
    pub crdt3: u16,
    pub reserved134: [u8; 122],
    pub oacs: u16,
    pub acl: u8,
    pub aerl: u8,
    pub frmw: u8,
    pub lpa: u8,
    pub elpe: u8,
    pub npss: u8,
    pub avscc: u8,
    pub apsta: u8,
    pub wctemp: u16,
    pub cctemp: u16,
    pub mtfa: u16,
    pub hmpre: u32,
    pub hmmin: u32,
    pub tnvmcap: [u8; 16],
    pub unvmcap: [u8; 16],
    pub rpmbs: u32,
    pub edstt: u16,
    pub dsto: u8,
    pub fwug: u8,
    pub kas: u16,
    pub hctma: u16,
    pub mntmt: u16,
    pub mxtmt: u16,
    pub sanicap: u32,
    pub hmminds: u32,
    pub hmmaxd: u16,
    pub reserved338: [u8; 4],
    pub anatt: u8,
    pub anacap: u8,
    pub anagrpmax: u32,
    pub nanagrpid: u32,
    pub reserved352: [u8; 160],
    pub sqes: u8,
    pub cqes: u8,
    pub maxcmd: u16,
    pub nn: u32,
    pub oncs: u16,
    pub fuses: u16,
    pub fna: u8,
    pub vwc: u8,
    pub awun: u16,
    pub awupf: u16,
    pub nvscc: u8,
    pub nwpc: u8,
    pub acwu: u16,
    pub reserved534: [u8; 2],
    pub sgls: u32,
    pub mnan: u32,
    pub reserved544: [u8; 224],
    pub subnqn: [u8; 256],
    pub reserved1024: [u8; 768],
    pub ioccsz: u32,
    pub iorcsz: u32,
    pub icdoff: u16,
    pub ctrattr: u8,
    pub msdbd: u8,
    pub reserved1804: [u8; 244],
    pub psd: [PowerState; 32],
    pub vs: [u8; 1024],
}

#[repr(C)]
#[derive(Debug)]
pub struct LbaFormat {
    pub ms: u16,
    pub ds: u8,
    pub rp: u8,
}

#[derive(Debug)]
#[repr(C)]
pub struct IdentifyNamespace {
    pub nsze: u64,
    pub ncap: u64,
    pub nuse: u64,
    pub nsfeat: u8,
    pub nlbaf: u8,
    pub flbas: u8,
    pub mc: u8,
    pub dpc: u8,
    pub dps: u8,
    pub nmic: u8,
    pub rescap: u8,
    pub fpi: u8,
    pub dlfeat: u8,
    pub nawun: u16,
    pub nawupf: u16,
    pub nacwu: u16,
    pub nabsn: u16,
    pub nabo: u16,
    pub nabspf: u16,
    pub noiob: u16,
    pub nvmcap: [u8; 16],
    pub npwg: u16,
    pub npwa: u16,
    pub npdg: u16,
    pub npda: u16,
    pub nows: u16,
    pub reserved74: [u8; 18],
    pub anagrpid: u32,
    pub reserved96: [u8; 3],
    pub nsattr: u8,
    pub nvmsetid: u16,
    pub endgid: u16,
    pub nguid: [u8; 16],
    pub eui64: [u8; 8],
    pub lbaf: [LbaFormat; 16],
    pub reserved192: [u8; 192],
    pub vs: [u8; 3712],
}
