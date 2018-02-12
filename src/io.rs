use libc::{c_uchar, c_void};
use libusb::{self, libusb_transfer, libusb_transfer_cb_fn, libusb_context};


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
pub trait IoRefType {
    type Handle: Clone;
    fn handle(&self) -> Self::Handle;
}

pub trait AsyncIoType<'ctx> : Sized {
    type TransferBuilder: TransferBuilderType<TransferHandle=Self::TransferHandle>;
    type TransferHandle: TransferHandleType;
    type TransferCbData;
    type TransferCbRes;

    fn allocate<TBF>(&self, callback: Option<TBF>, buf: Vec<u8>) -> AsyncAllocationResult<Self::TransferBuilder>
        where TBF: Into<Box<Fn(Self::TransferCbData) -> Self::TransferCbRes>>;
}

pub struct AsyncAllocationResult<Builder> {
    pub builder:       Builder,
    pub callback:      libusb_transfer_cb_fn,  // c abi callback
    pub user_data_ptr: *mut c_void,
    pub buf_ptr:       *mut c_uchar,
    pub len:           i32,
}

pub trait TransferBuilderType {
    type TransferHandle;
    fn submit(self, transfer: *mut libusb_transfer) -> ::Result<Self::TransferHandle>;
}

pub trait TransferHandleType {
    fn cancel(&self) -> ::Result<()>;
    fn status(&self) -> TransferStatus;
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
    use std::mem::{size_of, transmute};
    use std::sync::Mutex;
    use std::os::unix::io::RawFd;
    use std::collections::HashMap;
    use mio::{Ready, Token};
    use libusb::*;
    use libc::c_void;
    use super::*;

    pub struct AsyncIo {
        pub reg: Mutex<Option<(Token, Vec<(RawFd, Ready)>)>>,
        pub transfers: Mutex<AsyncIoTransfers>,
    }

    impl IoType for AsyncIo {
        fn new(ctx: *mut libusb_context) -> Self {
            if unsafe { libusb_pollfds_handle_timeouts(ctx) } == 0 {
                panic!("This system requires time-based event handling, which is not \
                       supported, see http://libusb.sourceforge.net/api-1.0/group__poll.html \
                       for details")
            }
            if size_of::<usize>() != size_of::<*mut c_void>() {
                panic!("Async code is written by assuming *mut c_void is as big as usize, \
                        but its not, *mut c_void is {} and usize is {}",
                        size_of::<*mut c_void>(), size_of::<usize>())

            }
            AsyncIo {
                reg: Mutex::new(None),
                transfers: Mutex::new( AsyncIoTransfers {
                    next_id: 0,
                    running: HashMap::new(),
                    complete: Vec::new()
                }),
            }
        }
    }

    impl<'ctx> IoRefType for &'ctx AsyncIo {
        type Handle = &'ctx AsyncIo;
        fn handle(&self) -> Self::Handle { self }
    }

    impl<'ctx> AsyncIoType<'ctx> for &'ctx AsyncIo {
        type TransferBuilder = AsyncIoTransferBuilder<'ctx>;
        type TransferHandle = AsyncIoTransferHandle<'ctx>;
        type TransferCbData = ();
        type TransferCbRes = ();

        fn allocate<TBF>(&self, cb: Option<TBF>, buf: Vec<u8>) -> AsyncAllocationResult<Self::TransferBuilder>
            where TBF: Into<Box<Fn(Self::TransferCbData) -> Self::TransferCbRes>>,
        {
            let mut tr = self.transfers.lock().expect("Could not unlock AsyncIo transfers mutex");
            while tr.running.contains_key(&tr.next_id) {
                tr.next_id += 1;
            }
            let id = tr.next_id;
            tr.next_id += 1;
            let mut transfer = Box::new(AsyncIoTransfer {
                buf: buf,
                callback: cb.map(|x| x.into()),
                transfer: ptr::null_mut(),
            });
            let res = AsyncAllocationResult {
                builder:       AsyncIoTransferBuilder { io: self, id: id },
                callback:      async_io_callback_function,
                user_data_ptr: unsafe{ transmute(id) },
                buf_ptr:       transfer.buf.as_mut_ptr(),
                len:           transfer.buf.len() as i32,
            };
            tr.running.insert(id, transfer);
            res
        }
    }

    pub struct AsyncIoTransferBuilder<'ctx> {
        io: &'ctx AsyncIo,
        id: usize,
    }

    impl<'ctx> TransferBuilderType for AsyncIoTransferBuilder<'ctx> {
        type TransferHandle = AsyncIoTransferHandle<'ctx>;

        fn submit(self, transfer: *mut libusb_transfer) -> ::Result<Self::TransferHandle> {
            let mut tr = self.io.transfers.lock().expect("Could not unlock AsyncIo transfers mutex");
            let handle = AsyncIoTransferHandle { io: self.io, id: self.id };
            // TODO: fix AsyncIoTransfer transfer ptr, s
            let res = unsafe{ libusb_submit_transfer(transfer) }; // TODO: Conv to result
            unimplemented!()
        }
    }

    pub struct AsyncIoTransferHandle<'ctx> {
        io: &'ctx AsyncIo,
        id: usize,
    }

    impl<'ctx> TransferHandleType for AsyncIoTransferHandle<'ctx> {
        fn cancel(&self) -> ::Result<()> { unimplemented!() }
        fn status(&self) -> TransferStatus { unimplemented!() }
    }

    pub struct AsyncIoTransfer {
        buf: Vec<u8>,
        callback: Option<Box<Fn( () ) -> ()>>,
        transfer: *mut libusb_transfer,
    }

    pub struct AsyncIoTransfers {
        next_id: usize,
        running: HashMap<usize, Box<AsyncIoTransfer>>,
        complete: Vec<(usize, Box<AsyncIoTransfer>)>,
    }

    extern "C" fn async_io_callback_function(_tr: *mut libusb_transfer) {
        unimplemented!()
    }

}
