use libusb::libusb_context;

pub trait IoType: 'static {
    fn new(ctx: *mut libusb_context) -> Self;
}

pub mod generic {
    pub use ::context::Context;
    pub use ::device_list::{DeviceList, Devices};
    pub use ::device::Device;
    pub use ::device_handle::DeviceHandle;
}

pub mod sync {
    type Io = SyncIo;
    pub type Context = ::context::Context<Io>;
    pub type DeviceList<'ctx> = ::device_list::DeviceList<'ctx, Io>;
    pub type Devices<'ctx, 'dl> = ::device_list::Devices<'ctx, 'dl, Io>;
    pub type Device<'ctx> = ::device::Device<'ctx, Io>;
    pub type DeviceHandle<'ctx> = ::device_handle::DeviceHandle<'ctx, Io>;

    use ::IoType;
    use libusb::libusb_context;

    pub struct SyncIo;
    impl IoType for  SyncIo {
        fn new(_ctx: *mut libusb_context) -> Self { SyncIo }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod async {
    type Io = AsyncIo;
    pub type Context = ::context::Context<Io>;
    pub type DeviceList<'ctx> = ::device_list::DeviceList<'ctx, Io>;
    pub type Devices<'ctx, 'dl> = ::device_list::Devices<'ctx, 'dl, Io>;
    pub type Device<'ctx> = ::device::Device<'ctx, Io>;
    pub type DeviceHandle<'ctx> = ::device_handle::DeviceHandle<'ctx, Io>;

    use std::sync::Mutex;
    use std::os::unix::io::RawFd;
    use mio::{Ready, Token};
    use libusb::*;
    use ::IoType;

    pub struct AsyncIo {
        pub reg: Mutex<Option<(Token, Vec<(RawFd, Ready)>)>>,
    }

    impl IoType for AsyncIo {
        fn new(ctx: *mut libusb_context) -> Self {
            if unsafe { libusb_pollfds_handle_timeouts(ctx) } == 0 {
                panic!("This system requires time-based event handling, which is not \
                       supported, see http://libusb.sourceforge.net/api-1.0/group__poll.html \
                       for details")
            }
            AsyncIo {
                reg: Mutex::new(None)
            }
        }
    }
}
