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

pub trait IoType
    where Self: 'static,
          for<'io> &'io Self: IoRefType
{
    fn new(ctx: *mut libusb_context) -> Self;
}

// I want zero sized references and handle probably contains a ref
// https://github.com/rust-lang/rfcs/pull/2040
pub trait IoRefType: Clone {
    type Handle;
    fn handle(&self) -> Self::Handle;
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
    pub type Context = ::context::Context<SyncIo>;
    pub type DeviceList<'ctx> = ::device_list::DeviceList<'ctx, SyncIo, ()>;
    pub type Devices<'ctx, 'dl> = ::device_list::Devices<'ctx, 'dl, SyncIo, ()>;
    pub type Device<'ctx> = ::device::Device<'ctx, SyncIo, ()>;
    pub type DeviceHandle<'ctx> = ::device_handle::DeviceHandle<'ctx, SyncIo, ()>;

    use super::{IoType, IoRefType};
    use libusb::libusb_context;

    pub struct SyncIo;

    impl IoType for SyncIo {
        fn new(_ctx: *mut libusb_context) -> Self { SyncIo }
    }

    impl<'ctx> IoRefType for &'ctx SyncIo {
        type Handle = ();
        fn handle(&self) -> Self::Handle { }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod async {
    pub type Context = ::context::Context<AsyncIo>;
    pub type DeviceList<'ctx> = ::device_list::DeviceList<'ctx, AsyncIo, &'ctx AsyncIo>;
    pub type Devices<'ctx, 'dl> = ::device_list::Devices<'ctx, 'dl, AsyncIo, &'ctx AsyncIo>;
    pub type Device<'ctx> = ::device::Device<'ctx, AsyncIo, &'ctx AsyncIo>;
    pub type DeviceHandle<'ctx> = ::device_handle::DeviceHandle<'ctx, AsyncIo, &'ctx AsyncIo>;

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

    impl<'ctx> IoRefType for &'ctx AsyncIo {
        type Handle = &'ctx AsyncIo;
        fn handle(&self) -> Self::Handle { self }
    }

    impl<'ctx> AsyncIoType<'ctx> for &'ctx AsyncIo {
        type TransferBuilder = AsyncIoTransferBuilder<'ctx>;
        type Transfer = AsyncIoTransfer<'ctx>;
        type TransferCbData = ();
        type TransferCbRes = ();

        fn allocate<TBF, F>(&self, _callback: Option<TBF>) -> (Self::TransferBuilder, libusb_transfer_cb_fn, *mut c_void)
            where TBF: Into<Box<F>>,
                    F: Fn(Self::TransferCbData) -> Self::TransferCbRes
        {
            ( AsyncIoTransferBuilder { _io: self }, c_callback_function, ptr::null_mut() )
        }
    }

    pub struct AsyncIoTransferBuilder<'ctx> {
        _io: &'ctx AsyncIo
    }

    impl<'ctx> TransferBuilderType for AsyncIoTransferBuilder<'ctx> {
        type Transfer = AsyncIoTransfer<'ctx>;
        fn submit(self) -> Self::Transfer {
            let _t = AsyncIoTransfer { _io: self._io };
            unimplemented!()
        }
    }

    pub struct AsyncIoTransfer<'ctx> {
        _io: &'ctx AsyncIo
    }

    impl<'ctx> TransferType for AsyncIoTransfer<'ctx> {
        fn cancel(&self) -> ::Result<()> { unimplemented!() }
        fn status(&self) -> TransferStatus { unimplemented!() }
    }

    extern "C" fn c_callback_function(_tr: *mut libusb_transfer) {
        unimplemented!()
    }

}
