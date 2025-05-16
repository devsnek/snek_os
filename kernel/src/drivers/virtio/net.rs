use super::{create_transport, Hal};
use crate::arch::InterruptGuard;
use crate::arch::PciDevice;
use alloc::sync::Arc;
use futures::task::AtomicWaker;
use spin::Mutex;
use virtio_drivers::{
    device::net::{RxBuffer, VirtIONet},
    transport::pci::PciTransport,
    Error,
};

type Device = VirtIONet<Hal, PciTransport, 16>;

struct WrapperInner {
    device: Device,
    waker: AtomicWaker,
}

impl WrapperInner {
    fn handle_irq(&self) {
        self.waker.wake();
    }
}

struct Wrapper {
    inner: Arc<Mutex<WrapperInner>>,
    _guard: InterruptGuard,
}

impl Wrapper {
    fn new(header: &PciDevice, device: Device) -> Result<Self, anyhow::Error> {
        let inner = Arc::new(Mutex::new(WrapperInner {
            device,
            waker: AtomicWaker::new(),
        }));

        let weak = Arc::downgrade(&inner);

        let interrupt_guard = crate::arch::set_interrupt_msi(
            header.clone(),
            Box::new(move || {
                if let Some(w) = weak.upgrade() {
                    w.lock().handle_irq();
                }
            }),
        )
        .unwrap();

        crate::arch::without_interrupts(|| {
            let mut w = inner.lock();
            w.device.transport_mut().set_queue_msix_vector(0x00);
            w.device.enable_interrupts();
            assert_eq!(w.device.transport().get_queue_msix_vector(), 0x00);
        });

        Ok(Self {
            inner,
            _guard: interrupt_guard,
        })
    }
}

impl smoltcp::phy::Device for Wrapper {
    type RxToken<'a>
        = RxToken
    where
        Self: 'a;
    type TxToken<'a>
        = TxToken
    where
        Self: 'a;

    fn receive(
        &mut self,
        _timestamp: smoltcp::time::Instant,
    ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        match crate::arch::without_interrupts(|| self.inner.lock().device.receive()) {
            Ok(buf) => Some((
                RxToken(self.inner.clone(), buf),
                TxToken(self.inner.clone()),
            )),
            Err(Error::NotReady) => None,
            Err(e) => {
                error!("receive failed {e}");
                None
            }
        }
    }

    fn transmit(&mut self, _timestamp: smoltcp::time::Instant) -> Option<Self::TxToken<'_>> {
        Some(TxToken(self.inner.clone()))
    }

    fn capabilities(&self) -> smoltcp::phy::DeviceCapabilities {
        let mut caps = smoltcp::phy::DeviceCapabilities::default();
        caps.max_transmission_unit = 1536;
        caps.max_burst_size = Some(1);
        caps.medium = smoltcp::phy::Medium::Ethernet;
        caps
    }
}

struct RxToken(Arc<Mutex<WrapperInner>>, RxBuffer);

impl smoltcp::phy::RxToken for RxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        let result = f(self.1.packet());
        crate::arch::without_interrupts(|| {
            self.0.lock().device.recycle_rx_buffer(self.1).unwrap();
        });
        result
    }
}

struct TxToken(Arc<Mutex<WrapperInner>>);

impl smoltcp::phy::TxToken for TxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        crate::arch::without_interrupts(|| {
            let mut w = self.0.lock();
            let mut tx_buf = w.device.new_tx_buffer(len);
            let result = f(tx_buf.packet_mut());
            w.device.send(tx_buf).unwrap();
            result
        })
    }
}

impl crate::net::Driver for Wrapper {
    fn address(&self) -> smoltcp::wire::HardwareAddress {
        smoltcp::wire::HardwareAddress::Ethernet(smoltcp::wire::EthernetAddress(
            crate::arch::without_interrupts(|| self.inner.lock().device.mac_address()),
        ))
    }

    fn poll(&self, cx: &mut core::task::Context) -> core::task::Poll<()> {
        crate::arch::without_interrupts(|| {
            let w = self.inner.lock();
            w.waker.register(cx.waker());
            if w.device.can_recv() {
                core::task::Poll::Ready(())
            } else {
                core::task::Poll::Pending
            }
        })
    }
}

pub fn init(header: &PciDevice) -> Result<bool, anyhow::Error> {
    if header.vendor_id != 0x1af4 || (header.device_id != 0x1000 && header.device_id != 0x1041) {
        return Ok(false);
    }

    let transport = create_transport(header)?;

    let device = Device::new(transport, 1600)?;

    let wrapper = Wrapper::new(header, device)?;

    crate::net::register(wrapper);

    Ok(true)
}
