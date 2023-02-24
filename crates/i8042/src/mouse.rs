pub(crate) enum MouseKind {
    Standard,
    Scroll,
    FiveButton,
}

#[derive(Debug)]
enum State {
    Ack,
    Idle,
    WaitByte2(u8),
    WaitByte3(u8, u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseState {
    pub x: i16,
    pub y: i16,
    pub left: bool,
    pub right: bool,
    pub middle: bool,
}

#[derive(Debug)]
pub(crate) struct Mouse {
    state: State,
    mouse_state: MouseState,
}

impl Mouse {
    pub(crate) fn new(_kind: MouseKind) -> (Option<u8>, Self) {
        (
            Some(0xF4),
            Self {
                state: State::Ack,
                mouse_state: MouseState {
                    x: i16::MAX / 2,
                    y: i16::MAX / 2,
                    middle: false,
                    right: false,
                    left: false,
                },
            },
        )
    }

    pub(crate) fn handle_data(&mut self, data: u8) -> Option<MouseState> {
        match self.state {
            State::Ack => {
                self.state = State::Idle;
                None
            }
            State::Idle => {
                if data & (1 << 3) != 0 {
                    self.state = State::WaitByte2(data)
                }
                None
            }
            State::WaitByte2(b1) => {
                self.state = State::WaitByte3(b1, data);
                None
            }
            State::WaitByte3(b1, b2) => {
                let dx = get_signed_9(((b1 >> 6) & 1) != 0, ((b1 >> 4) & 1) != 0, b2);
                let dy = get_signed_9(((b1 >> 7) & 1) != 0, ((b1 >> 5) & 1) != 0, data);

                let new_state = MouseState {
                    x: self.mouse_state.x.saturating_add(dx),
                    y: self.mouse_state.y.saturating_add(dy),
                    left: b1 & (1 << 0) != 0,
                    right: b1 & (1 << 1) != 0,
                    middle: b1 & (1 << 2) != 0,
                };

                self.state = State::Idle;

                if new_state != self.mouse_state {
                    self.mouse_state = new_state.clone();
                    Some(new_state)
                } else {
                    None
                }
            }
        }
    }
}

fn get_signed_9(overflow: bool, sign: bool, val: u8) -> i16 {
    if sign {
        if overflow {
            -256
        } else {
            val as i16 - 0x100
        }
    } else {
        if overflow {
            256
        } else {
            val as i16
        }
    }
}
