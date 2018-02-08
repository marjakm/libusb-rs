use libc::c_void;
use libusb::{self, libusb_transfer_cb_fn, libusb_context};


/// The status of a Transfer returned by wait_any.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TransferStatus {
    /// Completed without error
    Success = libusb::LIBUSB_TRANSFER_COMPLETED as isize,
    /// Failed (IO error)
    Error = libusb::LIBUSB_TRANSFER_ERROR as isize,
    /// Timed out
    Timeout = libusb::LIBUSB_TRANSFER_TIMED_OUT as isize,
    /// Cancelled
    Cancelled = libusb::LIBUSB_TRANSFER_CANCELLED as isize,
    /// Endpoint stalled or control request not supported
    Stall = libusb::LIBUSB_TRANSFER_STALL as isize,
    /// Device was disconnected
    NoDevice = libusb::LIBUSB_TRANSFER_NO_DEVICE as isize,
    /// Device sent more data than requested
    Overflow = libusb::LIBUSB_TRANSFER_OVERFLOW as isize,
    /// No status, not yet submitted
    Unknown = -1 as isize,
}

impl From<i32> for TransferStatus {
    fn from(nr: i32) -> Self {
        match nr {
            libusb::LIBUSB_TRANSFER_COMPLETED => TransferStatus::Success,
            libusb::LIBUSB_TRANSFER_ERROR => TransferStatus::Error,
            libusb::LIBUSB_TRANSFER_TIMED_OUT => TransferStatus::Timeout,
            libusb::LIBUSB_TRANSFER_CANCELLED => TransferStatus::Cancelled,
            libusb::LIBUSB_TRANSFER_STALL => TransferStatus::Stall,
            libusb::LIBUSB_TRANSFER_NO_DEVICE => TransferStatus::NoDevice,
            _ => TransferStatus::Unknown,
        }
    }
}

pub trait IoType: 'static {
    fn new(ctx: *mut libusb_context) -> Self;
}

pub trait TransferBuilderType {
    type Transfer;
    fn submit(self) -> Self::Transfer;
}

pub trait TransferType {
    fn cancel(&self) -> ::Result<()>;
    fn status(&self) -> TransferStatus;
}

pub trait AsyncIoType<'ctx> : Sized {
    type TransferBuilder: TransferBuilderType<Transfer=Self::Transfer>;
    type Transfer: TransferType;
    type TransferCbData;
    type TransferCbRes;

    fn allocate<TBF, F>(&self, callback: Option<TBF>) -> (Self::TransferBuilder, libusb_transfer_cb_fn, *mut c_void)
        where TBF: Into<Box<F>>,
                F: Fn(Self::TransferCbData) -> Self::TransferCbRes;
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

    use std::ptr;
    use std::sync::Mutex;
    use std::os::unix::io::RawFd;
    use mio::{Ready, Token};
    use libusb::*;
    use libc::c_void;
    use super::*;


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

    impl<'ctx, 'io> AsyncIoType<'ctx> for &'io AsyncIo {
        type TransferBuilder = AsyncIoTransferBuilder<'io>;
        type Transfer = AsyncIoTransfer<'io>;
        type TransferCbData = ();
        type TransferCbRes = ();

        fn allocate<TBF, F>(&self, _callback: Option<TBF>) -> (Self::TransferBuilder, libusb_transfer_cb_fn, *mut c_void)
            where TBF: Into<Box<F>>,
                    F: Fn(Self::TransferCbData) -> Self::TransferCbRes
        {
            ( AsyncIoTransferBuilder { _io: self }, c_callback_function, ptr::null_mut() )
        }
    }

    pub struct AsyncIoTransferBuilder<'io> {
        _io: &'io AsyncIo
    }
    impl<'io> TransferBuilderType for AsyncIoTransferBuilder<'io> {
        type Transfer = AsyncIoTransfer<'io>;
        fn submit(self) -> Self::Transfer {
            let _t = AsyncIoTransfer { _io: self._io };
            unimplemented!()
        }
    }

    pub struct AsyncIoTransfer<'io> {
        _io: &'io AsyncIo
    }
    impl<'io> TransferType for AsyncIoTransfer<'io> {
        fn cancel(&self) -> ::Result<()> { unimplemented!() }
        fn status(&self) -> TransferStatus { unimplemented!() }
    }

    extern "C" fn c_callback_function(_tr: *mut libusb_transfer) {
        unimplemented!()
    }

}
