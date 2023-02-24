use crate::task::spawn_blocking;
use core::time::Duration;

pub fn sleep(duration: Duration) {
    spawn_blocking(maitake::time::sleep(duration));
}
