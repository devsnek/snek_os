use conquer_once::spin::OnceCell;
use core::{future::poll_fn, task::Poll};
use crossbeam_queue::ArrayQueue;
use futures::task::AtomicWaker;
use i8042::{Change, DecodedKey, Driver8042, Irq, MouseState};
use spin::Mutex;
use x86_64::instructions::port::Port;

#[derive(Debug)]
struct DriverImpl;

impl i8042::Impl for DriverImpl {
    fn read_data(&mut self) -> u8 {
        unsafe { Port::new(0x60).read() }
    }

    fn write_data(&mut self, data: u8) {
        unsafe { Port::new(0x60).write(data) }
    }

    fn write_cmd(&mut self, cmd: u8) {
        unsafe { Port::new(0x64).write(cmd) }
    }

    fn read_status(&mut self) -> u8 {
        unsafe { Port::new(0x64).read() }
    }
}

static DRIVER: Mutex<Driver8042<DriverImpl>> = Mutex::new(Driver8042::new(DriverImpl));

static KEY_QUEUE: OnceCell<ArrayQueue<DecodedKey>> = OnceCell::uninit();
static KEY_WAKER: AtomicWaker = AtomicWaker::new();

static MOUSE_QUEUE: OnceCell<ArrayQueue<MouseState>> = OnceCell::uninit();
static MOUSE_WAKER: AtomicWaker = AtomicWaker::new();

pub fn interrupt(irq: Irq) {
    if let Some(change) = DRIVER.lock().interrupt(irq) {
        match change {
            Change::Keyboard(key) => {
                if let Ok(queue) = KEY_QUEUE.try_get() {
                    if queue.push(key).is_ok() {
                        KEY_WAKER.wake();
                    }
                }
            }
            Change::Mouse(state) => {
                if let Ok(queue) = MOUSE_QUEUE.try_get() {
                    if queue.push(state).is_ok() {
                        MOUSE_WAKER.wake();
                    }
                }
            }
        }
    }
}

pub fn init() {
    unsafe {
        DRIVER.lock().init().unwrap();
    }

    KEY_QUEUE.try_init_once(|| ArrayQueue::new(32)).unwrap();
    MOUSE_QUEUE.try_init_once(|| ArrayQueue::new(32)).unwrap();

    core::mem::forget(crate::arch::set_interrupt_static(
        1,
        crate::arch::InterruptType::LevelLow,
        || {
            interrupt(i8042::Irq::Irq1);
        },
    ));
    core::mem::forget(crate::arch::set_interrupt_static(
        12,
        crate::arch::InterruptType::LevelLow,
        || {
            interrupt(i8042::Irq::Irq12);
        },
    ));
}

pub async fn next_key() -> Option<DecodedKey> {
    poll_fn(|cx| {
        let Ok(queue) = KEY_QUEUE.try_get() else {
            return Poll::Ready(None);
        };

        // fast path
        if let Some(key) = queue.pop() {
            return Poll::Ready(Some(key));
        }

        KEY_WAKER.register(cx.waker());
        match queue.pop() {
            Some(key) => {
                KEY_WAKER.take();
                Poll::Ready(Some(key))
            }
            None => Poll::Pending,
        }
    })
    .await
}

#[allow(unused)]
pub async fn next_mouse_state() -> Option<MouseState> {
    poll_fn(|cx| {
        let Ok(queue) = MOUSE_QUEUE.try_get() else {
            return Poll::Ready(None);
        };

        // fast path
        if let Some(state) = queue.pop() {
            return Poll::Ready(Some(state));
        }

        MOUSE_WAKER.register(cx.waker());
        match queue.pop() {
            Some(state) => {
                MOUSE_WAKER.take();
                Poll::Ready(Some(state))
            }
            None => Poll::Pending,
        }
    })
    .await
}
