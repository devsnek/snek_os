use pc_keyboard::{
    layouts::Us104Key, DecodedKey, HandleControl, Keyboard as PcKeyboard, ScancodeSet1,
    ScancodeSet2,
};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum KeyboardKind {
    MF2,
    Short,
    N97,
    K122,
    MF2Emul,
    JapG,
    JapP,
    JapA,
    Sun,
}

enum SomeKeyboard {
    Set1(PcKeyboard<Us104Key, ScancodeSet1>),
    Set2(PcKeyboard<Us104Key, ScancodeSet2>),
}

impl SomeKeyboard {
    fn handle_data(&mut self, data: u8) -> Option<DecodedKey> {
        macro_rules! get_kbd {
            ($k:ident, $e:expr) => {{
                match self {
                    SomeKeyboard::Set1(k) => {
                        let $k = k;
                        $e
                    }
                    SomeKeyboard::Set2(k) => {
                        let $k = k;
                        $e
                    }
                }
            }};
        }

        if let Ok(Some(event)) = get_kbd!(k, k.add_byte(data)) {
            get_kbd!(k, k.process_keyevent(event))
        } else {
            None
        }
    }
}

impl core::fmt::Debug for SomeKeyboard {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "SomeKeyboard")?;
        Ok(())
    }
}

#[derive(Debug)]
enum State {
    Disabled,
    Init(Init),
    Normal(SomeKeyboard),
}

#[derive(Debug)]
enum Init {
    ReqScancodeSetAck,
    ReqScancodeSetRsp,
    EnableAck(u8),
}

#[derive(Debug)]
pub(crate) struct Keyboard {
    state: State,
}

impl Keyboard {
    pub(crate) fn new(kind: KeyboardKind) -> (Option<u8>, Self) {
        if kind == KeyboardKind::MF2 {
            (
                Some(0xF0),
                Self {
                    state: State::Init(Init::ReqScancodeSetAck),
                },
            )
        } else {
            (
                None,
                Self {
                    state: State::Disabled,
                },
            )
        }
    }

    pub(crate) fn handle_data(&mut self, data: u8) -> (Option<u8>, Option<DecodedKey>) {
        match self.state {
            State::Disabled => (None, None),
            State::Init(ref mut init) => match init {
                Init::ReqScancodeSetAck => {
                    self.state = State::Init(Init::ReqScancodeSetRsp);
                    (Some(0x00), None)
                }
                Init::ReqScancodeSetRsp => match data {
                    1 | 2 | 3 => {
                        self.state = State::Init(Init::EnableAck(data));
                        (Some(0xF4), None)
                    }
                    0xFA => (None, None),
                    _ => {
                        self.state = State::Disabled;
                        (None, None)
                    }
                },
                Init::EnableAck(set) => match data {
                    0xFA => {
                        let keyboard = match set {
                            1 => SomeKeyboard::Set1(PcKeyboard::new(
                                ScancodeSet1::new(),
                                Us104Key,
                                HandleControl::Ignore,
                            )),
                            2 => SomeKeyboard::Set2(PcKeyboard::new(
                                ScancodeSet2::new(),
                                Us104Key,
                                HandleControl::Ignore,
                            )),
                            3 => {
                                self.state = State::Disabled;
                                return (None, None);
                            }
                            _ => {
                                self.state = State::Disabled;
                                return (None, None);
                            }
                        };

                        self.state = State::Normal(keyboard);
                        (None, None)
                    }
                    _ => {
                        self.state = State::Disabled;
                        (None, None)
                    }
                },
            },
            State::Normal(ref mut keyboard) => match data {
                0x00 | 0xFF | 0xAA | 0xFC | 0xFD | 0xEE | 0xFA | 0xFE => (None, None),
                _ => {
                    let key = keyboard.handle_data(data);
                    (None, key)
                }
            },
        }
    }
}
