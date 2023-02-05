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
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};

static SCANCODE_QUEUE: OnceCell<ArrayQueue<u8>> = OnceCell::uninit();
static WAKER: AtomicWaker = AtomicWaker::new();

// INTERRUPT HANDLER!!!
// DO NOT BLOCK OR ALLOCATE HERE!!!
pub(crate) fn add_scancode(scancode: u8) {
    if let Ok(queue) = SCANCODE_QUEUE.try_get() {
        if queue.push(scancode).is_ok() {
            WAKER.wake();
        }
    }
}

pub struct ScancodeStream {}

impl ScancodeStream {
    pub fn new() -> Self {
        SCANCODE_QUEUE
            .try_init_once(|| ArrayQueue::new(32))
            .expect("ScancodeStream::new should only be called once");
        ScancodeStream {}
    }
}

impl Stream for ScancodeStream {
    type Item = u8;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<u8>> {
        let queue = SCANCODE_QUEUE
            .try_get()
            .expect("scancode queue not initialized");

        // fast path
        if let Some(scancode) = queue.pop() {
            return Poll::Ready(Some(scancode));
        }

        WAKER.register(cx.waker());
        match queue.pop() {
            Some(scancode) => {
                WAKER.take();
                Poll::Ready(Some(scancode))
            }
            None => Poll::Pending,
        }
    }
}

pub async fn dispatch_keypresses() {
    let mut scancodes = ScancodeStream::new();
    let mut keyboard = Keyboard::<layouts::Us104Key, ScancodeSet1>::new(HandleControl::Ignore);

    while let Some(scancode) = scancodes.next().await {
        let r = if let Ok(Some(event)) = keyboard.add_byte(scancode) {
            keyboard.process_keyevent(event)
        } else {
            None
        };

        if let Some(key) = r {
            match key {
                DecodedKey::Unicode(c) => {
                    print!("{c}");
                }
                DecodedKey::RawKey(_key) => {}
            }
        }
    }
}
