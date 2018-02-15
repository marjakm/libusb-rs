use std::fmt;
use libc::{c_uchar, c_void};
use libusb::{self, libusb_transfer, libusb_device_handle, libusb_transfer_cb_fn, libusb_context};


// I want zero sized references and handle probably contains a ref
// https://github.com/rust-lang/rfcs/pull/2040
pub trait IoType<'ctx>: 'static+fmt::Debug {
    type Handle: Clone+fmt::Debug;
    fn new(ctx: *mut libusb_context) -> Self;
    fn handle(&'ctx self) -> Self::Handle;
}

pub trait AsyncIoType<'ctx, 'dh>: Sized+fmt::Debug {
    type TransferBuilder: AsyncIoTransferBuilderType<TransferHandle=Self::TransferHandle>+fmt::Debug;
    type TransferHandle:  AsyncIoTransferHandleType+fmt::Debug;
    type TransferCallbackData: fmt::Debug;
    type TransferCallbackResult: fmt::Debug;
    fn allocate(&self, dh: &'dh *mut libusb_device_handle, cb: Option<Box<FnMut(Self::TransferCallbackData) -> Self::TransferCallbackResult>>, buf: Vec<u8>) -> AsyncIoTransferAllocationResult<Self::TransferBuilder>;
}

pub trait AsyncIoTransferBuilderType: fmt::Debug {
    type TransferHandle: fmt::Debug;
    fn submit(self, transfer: *mut libusb_transfer) -> ::Result<Self::TransferHandle>;
}

pub trait AsyncIoTransferHandleType: fmt::Debug {
    fn cancel(&self) -> ::Result<()>;
}

#[derive(Debug)]
pub struct AsyncIoTransferAllocationResult<TransferBuilder>
    where TransferBuilder: fmt::Debug
{
    pub builder:       TransferBuilder,
    pub callback:      libusb_transfer_cb_fn,
    pub user_data_ptr: *mut c_void,
    pub buf_ptr:       *mut c_uchar,
    pub len:           i32,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AsyncIoTransferStatus {
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

impl From<i32> for AsyncIoTransferStatus {
    fn from(nr: i32) -> Self {
        match nr {
            libusb::LIBUSB_TRANSFER_COMPLETED => AsyncIoTransferStatus::Success,
            libusb::LIBUSB_TRANSFER_ERROR => AsyncIoTransferStatus::Error,
            libusb::LIBUSB_TRANSFER_TIMED_OUT => AsyncIoTransferStatus::Timeout,
            libusb::LIBUSB_TRANSFER_CANCELLED => AsyncIoTransferStatus::Cancelled,
            libusb::LIBUSB_TRANSFER_STALL => AsyncIoTransferStatus::Stall,
            libusb::LIBUSB_TRANSFER_NO_DEVICE => AsyncIoTransferStatus::NoDevice,
            _ => AsyncIoTransferStatus::Unknown,
        }
    }
}

// Implementations ////////////////////////////////////////////////////////////////////

pub mod sync {
    pub type Context            = ::context::Context<SyncIo>;
    pub type DeviceList<'ctx>   = ::device_list::DeviceList<'ctx, SyncIo>;
    pub type Devices<'ctx, 'dl> = ::device_list::Devices<'ctx, 'dl, SyncIo>;
    pub type Device<'ctx>       = ::device::Device<'ctx, SyncIo>;
    pub type DeviceHandle<'ctx> = ::device_handle::DeviceHandle<'ctx, SyncIo>;

    use super::IoType;
    use libusb::libusb_context;

    #[derive(Debug)]
    pub struct SyncIo;

    impl<'ctx> IoType<'ctx> for SyncIo {
        type Handle = ();
        fn new(_ctx: *mut libusb_context) -> Self { SyncIo }
        fn handle(&'ctx self) -> Self::Handle { }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod unix_async {
    pub type Context            = ::context::Context<UnixAsyncIo>;
    pub type DeviceList<'ctx>   = ::device_list::DeviceList<'ctx, UnixAsyncIo>;
    pub type Devices<'ctx, 'dl> = ::device_list::Devices<'ctx, 'dl, UnixAsyncIo>;
    pub type Device<'ctx>       = ::device::Device<'ctx, UnixAsyncIo>;
    pub type DeviceHandle<'ctx> = ::device_handle::DeviceHandle<'ctx, UnixAsyncIo>;

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

    #[derive(Debug)]
    pub struct UnixAsyncIo {
        pub reg: Mutex<Option<(Token, Vec<(RawFd, Ready)>)>>,
        pub state: Mutex<UnixAsyncIoState>,
    }

    #[derive(Debug)]
    pub struct UnixAsyncIoState {
        next_id: usize,
        running: HashMap<usize, Box<UnixAsyncIoTransfer>>,
        pub complete: Vec<(usize, UnixAsyncIoTransferResult)>,
    }

    impl<'ctx> IoType<'ctx> for UnixAsyncIo {
        type Handle = &'ctx UnixAsyncIo;
        fn new(ctx: *mut libusb_context) -> Self {
            if unsafe { libusb_pollfds_handle_timeouts(ctx) } == 0 {
                panic!("This system requires time-based event handling, which is not \
                       supported, see http://libusb.sourceforge.net/api-1.0/group__poll.html \
                       for details")
            }
            UnixAsyncIo {
                reg: Mutex::new(None),
                state: Mutex::new( UnixAsyncIoState {
                    next_id: 0,
                    running: HashMap::new(),
                    complete: Vec::new()
                }),
            }
        }
        fn handle(&'ctx self) -> Self::Handle { self }
    }

    impl<'ctx, 'dh> AsyncIoType<'ctx, 'dh> for &'ctx UnixAsyncIo {
        type TransferBuilder = UnixAsyncIoTransferBuilder<'ctx, 'dh>;
        type TransferHandle = UnixAsyncIoTransferHandle<'ctx, 'dh>;
        type TransferCallbackData = UnixAsyncIoCallbackData;
        type TransferCallbackResult = UnixAsyncIoCallbackResult;

        fn allocate(&self, _dh: &'dh *mut libusb_device_handle, cb: Option<Box<FnMut(Self::TransferCallbackData) -> Self::TransferCallbackResult>>, buf: Vec<u8>) -> AsyncIoTransferAllocationResult<UnixAsyncIoTransferBuilder<'ctx, 'dh>> {
            let mut tr = self.state.lock().expect("Could not unlock UnixAsyncIo state mutex");
            while tr.running.contains_key(&tr.next_id) {
                tr.next_id += 1;
            }
            let id = tr.next_id;
            tr.next_id += 1;
            let mut transfer = Box::new( UnixAsyncIoTransfer {
                id: id,
                io: *self as _,
                buf: Some(buf),
                callback: cb,
                transfer: ptr::null_mut(),
            });
            let res = AsyncIoTransferAllocationResult {
                builder:       UnixAsyncIoTransferBuilder { io: self, id: id, _dh: PhantomData },
                callback:      async_io_callback_function,
                user_data_ptr: ((&mut *transfer as &mut UnixAsyncIoTransfer) as *mut UnixAsyncIoTransfer) as *mut c_void,
                buf_ptr:       transfer.buf.as_mut().unwrap().as_mut_ptr(),
                len:           transfer.buf.as_ref().unwrap().len() as i32,
            };
            tr.running.insert(id, transfer);
            res
        }
    }

    #[derive(Debug)]
    pub struct UnixAsyncIoTransferBuilder<'ctx, 'dh> {
        io: &'ctx UnixAsyncIo,
        id: usize,
        _dh: PhantomData<&'dh *mut libusb_device_handle>,
    }

    impl<'ctx, 'dh> AsyncIoTransferBuilderType for UnixAsyncIoTransferBuilder<'ctx, 'dh> {
        type TransferHandle = UnixAsyncIoTransferHandle<'ctx, 'dh>;

        fn submit(self, transfer: *mut libusb_transfer) -> ::Result<UnixAsyncIoTransferHandle<'ctx, 'dh>> {
            let mut state = self.io.state.lock().expect("Could not unlock UnixAsyncIo state mutex");
            match state.running.get_mut(&self.id) {
                Some(tr) => { tr.transfer = transfer; },
                None => return Err("Should not happen: TransferBuilder id has no match in running state".into())
            }
            try_unsafe!(libusb_submit_transfer(transfer));
            Ok(UnixAsyncIoTransferHandle { io: self.io, id: self.id, _dh: PhantomData })
        }
    }

    #[derive(Debug)]
    pub struct UnixAsyncIoTransferHandle<'ctx, 'dh> {
        io: &'ctx UnixAsyncIo,
        id: usize,
        _dh: PhantomData<&'dh *mut libusb_device_handle>,
    }

    impl<'ctx, 'dh> AsyncIoTransferHandleType for UnixAsyncIoTransferHandle<'ctx, 'dh> {
        fn cancel(&self) -> ::Result<()> {
            let mut state = self.io.state.lock().expect("Could not unlock UnixAsyncIo state mutex");
            match state.running.get_mut(&self.id) {
                Some(tr) => {
                    try_unsafe!(libusb_cancel_transfer(tr.transfer));
                    Ok(())
                },
                None => Err(format!("Transfer with id {} not running", self.id).into())
            }
        }
    }

    #[derive(Debug)]
    pub struct UnixAsyncIoCallbackData {
        pub buf: Vec<u8>,
        pub actual_length: usize,
        pub status: AsyncIoTransferStatus,
    }

    #[derive(Debug)]
    pub enum UnixAsyncIoCallbackResult {
        Handled,                            // Consumed buffer
        Unhandled(UnixAsyncIoCallbackData), // Handle this in mio
        ReSubmit(Vec<u8>),                  // Resubmit with buf (may be a new one)
    }

    #[derive(Debug)]
    pub enum UnixAsyncIoTransferResult {
        Handled,
        Unhandled(UnixAsyncIoCallbackData),
        Err(::Error),
    }

    pub struct UnixAsyncIoTransfer {
        id: usize,
        io: *const UnixAsyncIo,
        buf: Option<Vec<u8>>,
        callback: Option<Box<FnMut(UnixAsyncIoCallbackData) -> UnixAsyncIoCallbackResult>>,
        transfer: *mut libusb_transfer,
    }

    impl fmt::Debug for UnixAsyncIoTransfer {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "UnixAsyncIoTransfer {{ id: {}, io: {:?}, buf: {:?}, callback: {}, transfer: {:?} }}",
                   self.id, self.io, self.buf,
                   if self.callback.is_some() { "Some" } else { "None" },
                   self.transfer
            )
        }
    }

    extern "C" fn async_io_callback_function(transfer_ptr: *mut libusb_transfer) {
        // It is currently undefined behavior to unwind from Rust code into foreign code
        let res = catch_unwind(|| {
            if transfer_ptr.is_null() { panic!("async_io_callback_function got null ptr for transfer") }
            let tr = unsafe { &mut *transfer_ptr };
            if tr.user_data.is_null() { panic!("async_io_callback_function got null ptr for user_data") }
            let aiotr = unsafe { &mut *(tr.user_data as *mut UnixAsyncIoTransfer) };
            if aiotr.io.is_null() { panic!("async_io_callback_function got null ptr for io") }
            let io = unsafe { &*aiotr.io };
            let mut state = io.state.lock().expect("async_io_callback_function could not unlock UnixAsyncIo state mutex");
            let cb_data = UnixAsyncIoCallbackData{
                buf: match aiotr.buf.take() {
                    Some(b) => b,
                    None => panic!("async_io_callback_function: buf is None, but it can't be at this point"),
                },
                actual_length: tr.actual_length as usize,
                status: AsyncIoTransferStatus::from(tr.status),
            };
            let atrr = match aiotr.callback {
                Some(ref mut cb) => {
                    let res = cb(cb_data);
                    match res {
                        UnixAsyncIoCallbackResult::Handled => UnixAsyncIoTransferResult::Handled,
                        UnixAsyncIoCallbackResult::Unhandled(x) => UnixAsyncIoTransferResult::Unhandled(x),
                        UnixAsyncIoCallbackResult::ReSubmit(b) => {
                            aiotr.buf = Some(b);
                            tr.buffer = aiotr.buf.as_mut().unwrap().as_mut_ptr();
                            tr.length = aiotr.buf.as_ref().unwrap().len() as i32;
                            match unsafe{ libusb_submit_transfer(transfer_ptr) } {
                                0 => return,
                                e => UnixAsyncIoTransferResult::Err(::error::from_libusb(e))
                            }
                        },
                    }
                },
                None => UnixAsyncIoTransferResult::Unhandled(cb_data),
            };
            // Transfer is done if this point is reached
            state.running.remove(&aiotr.id);
            state.complete.push((aiotr.id, atrr));
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
