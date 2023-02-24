use conquer_once::spin::OnceCell;
use core::{
    pin::Pin,
    task::{Context, Poll},
};
use crossbeam_queue::ArrayQueue;
use futures_util::{
    stream::{Stream, StreamExt},
    task::AtomicWaker,
};
use i8042::{Change, DecodedKey, Driver8042};

#[cfg(target_arch = "x86_64")]
use x86_64::instructions::port::Port;

#[derive(Debug)]
struct DriverImpl;

#[cfg(target_arch = "x86_64")]
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

static mut DRIVER: Driver8042<DriverImpl> = Driver8042::new(DriverImpl);

pub fn init() {
    unsafe {
        DRIVER.init().unwrap();
    }
}

static KEY_QUEUE: OnceCell<ArrayQueue<DecodedKey>> = OnceCell::uninit();
static WAKER: AtomicWaker = AtomicWaker::new();

pub fn interrupt(port: u8) {
    if let Some(change) = unsafe { DRIVER.interrupt(port) } {
        match change {
            Change::Keyboard(key) => {
                if let Ok(queue) = KEY_QUEUE.try_get() {
                    if queue.push(key).is_ok() {
                        WAKER.wake();
                    }
                }
            }
            Change::Mouse(_state) => {}
        }
    }
}

pub struct KeyStream {}

impl KeyStream {
    pub fn new() -> Self {
        KEY_QUEUE
            .try_init_once(|| ArrayQueue::new(32))
            .expect("KeyStream::new should only be called once");
        KeyStream {}
    }
}

impl Stream for KeyStream {
    type Item = DecodedKey;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<DecodedKey>> {
        let queue = KEY_QUEUE.try_get().expect("KEY_QUEUE not initialized");

        // fast path
        if let Some(key) = queue.pop() {
            return Poll::Ready(Some(key));
        }

        WAKER.register(cx.waker());
        match queue.pop() {
            Some(key) => {
                WAKER.take();
                Poll::Ready(Some(key))
            }
            None => Poll::Pending,
        }
    }
}

pub async fn dispatch_keypresses() {
    let mut keys = KeyStream::new();

    while let Some(key) = keys.next().await {
        match key {
            DecodedKey::Unicode(c) => {
                print!("{c}");
            }
            DecodedKey::RawKey(_key) => {}
        }
    }
}
