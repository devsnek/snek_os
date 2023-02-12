use x86_64::instructions::port::Port;

#[inline(always)]
pub fn debug(s: &str) {
    let mut port = Port::new(0xe9);
    if unsafe { port.read() } != 0xe9 {
        return;
    }

    for c in s.bytes() {
        unsafe {
            port.write(c);
        }
    }

    unsafe {
        port.write(b'\r');
        port.write(b'\n');
    }
}
