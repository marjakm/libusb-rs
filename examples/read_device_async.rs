extern crate mio;
extern crate libusb;
extern crate env_logger;

use std::thread::spawn;
use std::sync::Arc;
use mio::*;
use libusb::io::async::*;
// use libusb::LogLevel;

include!("_read_device.rs");

fn main() {
    env_logger::init();
    let context = Arc::new({
        let x = Context::new().unwrap_or_else(|e| panic!("could not initialize libusb: {}", e));
        // x.set_log_level(LogLevel::Debug);
        x
    });

    let context_c = context.clone();
    spawn(|| mio_thread(context_c));

    inner_main(&context);
}

fn mio_thread(context: Arc<Context>) {
    const USB: Token = Token(0);
    let poll = Poll::new().expect("Create poll");
    poll.register(context.as_ref(), USB, Ready::readable(), PollOpt::level()).expect("Register USB");
    let mut events = Events::with_capacity(1024);
    let mut v = Vec::new();
    loop {
        poll.poll(&mut events, None).unwrap();
        for event in events.iter() {
            match event.token() {
                USB => {
                    let _res = context.handle(&poll, &mut v);
                    // println!("USB handled: {:?}", res);
                    v.clear();
                }
                _ => unreachable!(),
            }
        }
    }
}
