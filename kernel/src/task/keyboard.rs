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
use pc_keyboard::{layouts, DecodedKey, HandleControl, KeyCode, KeyState, Keyboard, ScancodeSet1};

static SCANCODE_QUEUE: OnceCell<ArrayQueue<u8>> = OnceCell::uninit();
static WAKER: AtomicWaker = AtomicWaker::new();

// INTERRUPT HANDLER!!!
// DO NOT BLOCK OR ALLOCATE HERE!!!
pub(crate) fn add_scancode(scancode: u8) {
    if let Ok(queue) = SCANCODE_QUEUE.try_get() {
        if queue.push(scancode).is_err() {
            println!("WARNING: scancode queue full; dropping keyboard input");
        } else {
            WAKER.wake();
        }
    } else {
        println!("WARNING: scancode queue uninitialized");
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

        WAKER.register(&cx.waker());
        match queue.pop() {
            Some(scancode) => {
                WAKER.take();
                Poll::Ready(Some(scancode))
            }
            None => Poll::Pending,
        }
    }
}

#[derive(Debug)]
struct KeyboardEvent {
    code: String,
    pressed: bool,
    alt_key: bool,
    ctrl_key: bool,
    shift_key: bool,
    meta_key: bool,
}

pub async fn dispatch_keypresses() {
    let mut scancodes = ScancodeStream::new();
    let mut keyboard = Keyboard::new(layouts::Us104Key, ScancodeSet1, HandleControl::Ignore);

    let mut alt_key = false;
    let mut ctrl_key = false;
    let mut shift_key = false;
    let mut meta_key = false;

    while let Some(scancode) = scancodes.next().await {
        let r = if let Ok(Some(event)) = keyboard.add_byte(scancode) {
            let pressed = event.state == KeyState::Down;
            match event.code {
                KeyCode::AltLeft | KeyCode::AltRight => {
                    alt_key = pressed;
                    None
                }
                KeyCode::ControlLeft | KeyCode::ControlRight => {
                    ctrl_key = pressed;
                    None
                }
                KeyCode::ShiftLeft | KeyCode::ShiftRight => {
                    shift_key = pressed;
                    None
                }
                KeyCode::WindowsLeft | KeyCode::WindowsRight => {
                    meta_key = pressed;
                    None
                }
                _ => {
                    if let Some(key) = keyboard.process_keyevent(event) {
                        Some(match key {
                            DecodedKey::Unicode(character) => (character.to_string(), pressed),
                            DecodedKey::RawKey(key) => (alloc::format!("{:?}", key), pressed),
                        })
                    } else {
                        None
                    }
                }
            }
        } else {
            None
        };

        if let Some((code, state)) = r {
            let event = KeyboardEvent {
                code,
                pressed: state,
                alt_key,
                ctrl_key,
                shift_key,
                meta_key,
            };

            println!("{:?}", event);
        }
    }
}
