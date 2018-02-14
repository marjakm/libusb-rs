extern crate libusb;
extern crate env_logger;

use libusb::io::sync::*;
// use libusb::LogLevel;

include!("_read_device.rs");

fn main() {
    env_logger::init();
    let context = Context::new().unwrap_or_else(|e| panic!("could not initialize libusb: {}", e));
    // context.set_log_level(LogLevel::Debug);
    inner_main(&context);
}
