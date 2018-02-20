extern crate libusb;
extern crate env_logger;

pub use libusb::io::sync::*;
pub use libusb::ContextApi;
// use libusb::LogLevel;

mod inner;

fn main() {
    env_logger::init();
    let context = Context::new().unwrap_or_else(|e| panic!("could not initialize libusb: {}", e));
    // context.set_log_level(LogLevel::Debug);
    inner::main(&context);
}
