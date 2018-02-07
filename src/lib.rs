//! This crate provides a safe wrapper around the native `libusb` library.

#[macro_use] extern crate log;
extern crate bit_set;
extern crate libusb_sys as libusb;
extern crate libc;
extern crate mio;

pub use version::{LibraryVersion, version};
pub use error::{Result, Error};

pub use fields::{Speed, TransferType, SyncType, UsageType, Direction, RequestType, Recipient, Version, request_type};
pub use device_descriptor::DeviceDescriptor;
pub use config_descriptor::{ConfigDescriptor, Interfaces};
pub use interface_descriptor::{Interface, InterfaceDescriptors, InterfaceDescriptor, EndpointDescriptors};
pub use endpoint_descriptor::EndpointDescriptor;
pub use language::{Language, PrimaryLanguage, SubLanguage};

pub use context::LogLevel;
pub use sync_io::*;


#[cfg(test)]
#[macro_use]
mod test_helpers;

#[macro_use]
mod error;
mod version;

mod context;
mod device_list;
mod device;
mod device_handle;

mod fields;
mod device_descriptor;
mod config_descriptor;
mod interface_descriptor;
mod endpoint_descriptor;
mod language;


pub trait IoType: 'static {
    fn new() -> Self;
}

pub mod generic_io {
    pub use ::context::Context;
    pub use ::device_list::{DeviceList, Devices};
    pub use ::device::Device;
    pub use ::device_handle::DeviceHandle;
}

pub mod sync_io {
    type Io = SyncIo;
    pub type Context = ::context::Context<Io>;
    pub type DeviceList<'ctx> = ::device_list::DeviceList<'ctx, Io>;
    pub type Devices<'ctx, 'dl> = ::device_list::Devices<'ctx, 'dl, Io>;
    pub type Device<'ctx> = ::device::Device<'ctx, Io>;
    pub type DeviceHandle<'ctx> = ::device_handle::DeviceHandle<'ctx, Io>;

    use ::IoType;
    pub struct SyncIo;
    impl IoType for  SyncIo {
        fn new() -> Self { SyncIo }
    }
}

pub mod async_io {
    type Io = AsyncIo;
    pub type Context = ::context::Context<Io>;
    pub type DeviceList<'ctx> = ::device_list::DeviceList<'ctx, Io>;
    pub type Devices<'ctx, 'dl> = ::device_list::Devices<'ctx, 'dl, Io>;
    pub type Device<'ctx> = ::device::Device<'ctx, Io>;
    pub type DeviceHandle<'ctx> = ::device_handle::DeviceHandle<'ctx, Io>;

    use std::sync::Mutex;
    use std::os::unix::io::RawFd;
    use mio::{Ready, Token};
    use ::IoType;

    pub struct AsyncIo {
        pub reg: Mutex<Option<(Token, Vec<(RawFd, Ready)>)>>,
    }

    impl IoType for AsyncIo {
        fn new() -> Self {
            AsyncIo {
                reg: Mutex::new(None)
            }
        }
    }
}
