use std::fmt;
use libc::{c_uchar, c_void};
use libusb::{self, libusb_transfer, libusb_device_handle, libusb_transfer_cb_fn, libusb_context};


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

// I want zero sized references and handle probably contains a ref
// https://github.com/rust-lang/rfcs/pull/2040
pub trait IoType<'ctx>: 'static {
    type Handle: Clone;
    fn new(ctx: *mut libusb_context) -> Self;
    fn handle(&'ctx self) -> Self::Handle;
}

pub trait AsyncIoType<'ctx, 'dh>: Sized {
    type TransferBuilder: TransferBuilderType<TransferHandle=Self::TransferHandle>;
    type TransferHandle:  TransferHandleType;
    type TransferCbData;
    type TransferCbRes;

    fn allocate(&self, dh: &'dh *mut libusb_device_handle, cb: Option<Box<FnMut(Self::TransferCbData) -> Self::TransferCbRes>>, buf: Vec<u8>) -> AsyncAllocationResult<Self::TransferBuilder>;
}

pub struct AsyncAllocationResult<TransferBuilder> {
    pub builder:       TransferBuilder,
    pub callback:      libusb_transfer_cb_fn,  // c abi callback
    pub user_data_ptr: *mut c_void,
    pub buf_ptr:       *mut c_uchar,
    pub len:           i32,
}

impl<TransferBuilder> fmt::Debug for AsyncAllocationResult<TransferBuilder> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AsyncAllocationResult {{ callback: {:?}, user_data_ptr: {:?}, buf_ptr: {:?}, len: {:?} }}",
               self.callback,
               self.user_data_ptr,
               self.buf_ptr,
               self.len,
        )
    }
}

pub trait TransferBuilderType {
    type TransferHandle;
    fn submit(self, transfer: *mut libusb_transfer) -> ::Result<Self::TransferHandle>;
}

pub trait TransferHandleType {
    fn cancel(&self) -> ::Result<()>;
}

pub mod generic {
    pub use ::context::Context;
    pub use ::device_list::{DeviceList, Devices};
    pub use ::device::Device;
    pub use ::device_handle::DeviceHandle;
}

pub mod sync {
    pub type Context            = ::context::Context<SyncIo>;
    pub type DeviceList<'ctx>   = ::device_list::DeviceList<'ctx, SyncIo>;
    pub type Devices<'ctx, 'dl> = ::device_list::Devices<'ctx, 'dl, SyncIo>;
    pub type Device<'ctx>       = ::device::Device<'ctx, SyncIo>;
    pub type DeviceHandle<'ctx> = ::device_handle::DeviceHandle<'ctx, SyncIo>;

    use super::IoType;
    use libusb::libusb_context;

    pub struct SyncIo;

    impl<'ctx> IoType<'ctx> for SyncIo {
        type Handle = ();
        fn new(_ctx: *mut libusb_context) -> Self { SyncIo }
        fn handle(&'ctx self) -> Self::Handle { }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod async {
    pub type Context            = ::context::Context<AsyncIo>;
    pub type DeviceList<'ctx>   = ::device_list::DeviceList<'ctx, AsyncIo>;
    pub type Devices<'ctx, 'dl> = ::device_list::Devices<'ctx, 'dl, AsyncIo>;
    pub type Device<'ctx>       = ::device::Device<'ctx, AsyncIo>;
    pub type DeviceHandle<'ctx> = ::device_handle::DeviceHandle<'ctx, AsyncIo>;

    use std::ptr;
    use std::sync::Mutex;
    use std::process::abort;
    use std::os::unix::io::RawFd;
    use std::collections::HashMap;
    use std::panic::catch_unwind;
    use std::marker::PhantomData;
    use mio::{Ready, Token};
    use libusb::*;
    use super::*;

    pub struct AsyncIo {
        pub reg: Mutex<Option<(Token, Vec<(RawFd, Ready)>)>>,
        pub transfers: Mutex<AsyncIoTransfers>,
    }

    pub struct AsyncIoTransfers {
        next_id: usize,
        running: HashMap<usize, Box<AsyncIoTransfer>>,
        pub complete: Vec<(usize, AsyncIoTrRes)>,
    }

    impl<'ctx> IoType<'ctx> for AsyncIo {
        type Handle = &'ctx AsyncIo;
        fn new(ctx: *mut libusb_context) -> Self {
            if unsafe { libusb_pollfds_handle_timeouts(ctx) } == 0 {
                panic!("This system requires time-based event handling, which is not \
                       supported, see http://libusb.sourceforge.net/api-1.0/group__poll.html \
                       for details")
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
        fn handle(&'ctx self) -> Self::Handle { self }
    }

    impl<'ctx, 'dh> AsyncIoType<'ctx, 'dh> for &'ctx AsyncIo {
        type TransferBuilder = AsyncIoTransferBuilder<'ctx, 'dh>;
        type TransferHandle = AsyncIoTransferHandle<'ctx, 'dh>;
        type TransferCbData = AsyncIoCbData;
        type TransferCbRes = AsyncIoCbRes;

        fn allocate(&self, _dh: &'dh *mut libusb_device_handle, cb: Option<Box<FnMut(Self::TransferCbData) -> Self::TransferCbRes>>, buf: Vec<u8>) -> AsyncAllocationResult<AsyncIoTransferBuilder<'ctx, 'dh>> {
            let mut tr = self.transfers.lock().expect("Could not unlock AsyncIo transfers mutex");
            while tr.running.contains_key(&tr.next_id) {
                tr.next_id += 1;
            }
            let id = tr.next_id;
            tr.next_id += 1;
            let mut transfer = Box::new(AsyncIoTransfer {
                id: id,
                io: *self as _,
                buf: Some(buf),
                callback: cb,
                transfer: ptr::null_mut(),
            });
            let res = AsyncAllocationResult {
                builder:       AsyncIoTransferBuilder { io: self, id: id, _dh: PhantomData },
                callback:      async_io_callback_function,
                user_data_ptr: ((&mut *transfer as &mut AsyncIoTransfer) as *mut AsyncIoTransfer) as *mut c_void,
                buf_ptr:       transfer.buf.as_mut().unwrap().as_mut_ptr(),
                len:           transfer.buf.as_ref().unwrap().len() as i32,
            };
            tr.running.insert(id, transfer);
            res
        }
    }

    pub struct AsyncIoTransferBuilder<'ctx, 'dh> {
        io: &'ctx AsyncIo,
        id: usize,
        _dh: PhantomData<&'dh *mut libusb_device_handle>,
    }

    impl<'ctx, 'dh> TransferBuilderType for AsyncIoTransferBuilder<'ctx, 'dh> {
        type TransferHandle = AsyncIoTransferHandle<'ctx, 'dh>;

        fn submit(self, transfer: *mut libusb_transfer) -> ::Result<AsyncIoTransferHandle<'ctx, 'dh>> {
            let mut transfers = self.io.transfers.lock().expect("Could not unlock AsyncIo transfers mutex");
            match transfers.running.get_mut(&self.id) {
                Some(tr) => { tr.transfer = transfer; },
                None => return Err("Should not happen: TransferBuilder id has no match in running transfers".into())
            }
            try_unsafe!(libusb_submit_transfer(transfer));
            Ok(AsyncIoTransferHandle { io: self.io, id: self.id, _dh: PhantomData })
        }
    }

    pub struct AsyncIoTransferHandle<'ctx, 'dh> {
        io: &'ctx AsyncIo,
        id: usize,
        _dh: PhantomData<&'dh *mut libusb_device_handle>,
    }

    impl<'ctx, 'dh> TransferHandleType for AsyncIoTransferHandle<'ctx, 'dh> {
        fn cancel(&self) -> ::Result<()> {
            let mut transfers = self.io.transfers.lock().expect("Could not unlock AsyncIo transfers mutex");
            match transfers.running.get_mut(&self.id) {
                Some(tr) => {
                    try_unsafe!(libusb_cancel_transfer(tr.transfer));
                    Ok(())
                },
                None => Err(format!("Transfer with id {} not running", self.id).into())
            }
        }
    }

    pub struct AsyncIoCbData {
        pub buf: Vec<u8>,
        pub actual_length: usize,
        pub status: TransferStatus,
    }

    pub enum AsyncIoCbRes {
        Done,                   // Consumed buffer
        ToMio(AsyncIoCbData),   // Handle this in mio
        ReSubmit(Vec<u8>),      // Resubmit with buf (may be a new one)
    }

    pub enum AsyncIoTrRes {
        Handled,
        Unhandled(AsyncIoCbData),
        Err(::Error),
    }

    pub struct AsyncIoTransfer {
        id: usize,
        io: *const AsyncIo,
        buf: Option<Vec<u8>>,
        callback: Option<Box<FnMut(AsyncIoCbData) -> AsyncIoCbRes>>,
        transfer: *mut libusb_transfer,
    }

    extern "C" fn async_io_callback_function(transfer_ptr: *mut libusb_transfer) {
        // It is currently undefined behavior to unwind from Rust code into foreign code
        let res = catch_unwind(|| {
            if transfer_ptr.is_null() { panic!("async_io_callback_function got null ptr for transfer") }
            let tr = unsafe { &mut *transfer_ptr };
            if tr.user_data.is_null() { panic!("async_io_callback_function got null ptr for user_data") }
            let aiotr = unsafe { &mut *(tr.user_data as *mut AsyncIoTransfer) };
            if aiotr.io.is_null() { panic!("async_io_callback_function got null ptr for io") }
            let io = unsafe { &*aiotr.io };
            let mut transfers = io.transfers.lock().expect("async_io_callback_function could not unlock AsyncIo transfers mutex");
            let cb_data = AsyncIoCbData{
                buf: match aiotr.buf.take() {
                    Some(b) => b,
                    None => panic!("async_io_callback_function: buf is None, but it can't be at this point"),
                },
                actual_length: tr.actual_length as usize,
                status: TransferStatus::from(tr.status),
            };
            let atrr = match aiotr.callback {
                Some(ref mut cb) => {
                    let res = cb(cb_data);
                    match res {
                        AsyncIoCbRes::Done => AsyncIoTrRes::Handled,
                        AsyncIoCbRes::ToMio(x) => AsyncIoTrRes::Unhandled(x),
                        AsyncIoCbRes::ReSubmit(b) => {
                            aiotr.buf = Some(b);
                            tr.buffer = aiotr.buf.as_mut().unwrap().as_mut_ptr();
                            tr.length = aiotr.buf.as_ref().unwrap().len() as i32;
                            match unsafe{ libusb_submit_transfer(transfer_ptr) } {
                                0 => return,
                                e => AsyncIoTrRes::Err(::error::from_libusb(e))
                            }
                        },
                    }
                },
                None => AsyncIoTrRes::Unhandled(cb_data),
            };
            // Transfer is done if this point is reached
            transfers.running.remove(&aiotr.id);
            transfers.complete.push((aiotr.id, atrr));
            unsafe{ libusb_free_transfer(transfer_ptr) };
        });
        if let Err(e) = res {
            error!("Panic in async_io_callback_function: {:?}", e);
            println!("Panic in async_io_callback_function: {:?}", e);
            error!("Aborting");
            println!("Aborting");
            abort()
        };
    }
}
