#[cfg(target_arch = "x86_64")]
pub use super::i8042::next_key;
#[cfg(target_arch = "x86_64")]
pub use i8042::{DecodedKey, KeyCode};

#[cfg(target_arch = "aarch64")]
pub async fn next_key() -> Option<DecodedKey> {
    None
}

#[cfg(target_arch = "aarch64")]
pub enum DecodedKey {
    Unicode(char),
    RawKey(KeyCode),
}

#[cfg(target_arch = "aarch64")]
pub enum KeyCode {
    Backspace,
    Delete,
}
