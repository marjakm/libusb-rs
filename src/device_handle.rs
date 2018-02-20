use std::mem;
use bit_set::BitSet;
use libc::c_int;
use libusb::*;
use error;
pub use self::async_api::DeviceHandleAsyncApi;


/// A handle to an open USB device.
#[derive(Debug)]
pub struct DeviceHandle<IoHandle, CtxMarker> {
    ctx_marker: CtxMarker,
    io_handle: IoHandle,
    handle: *mut libusb_device_handle,
    interfaces: BitSet,
}

impl<IoHandle, CtxMarker> Drop for DeviceHandle<IoHandle, CtxMarker> {
    /// Closes the device.
    fn drop(&mut self) {
        unsafe {
            for iface in self.interfaces.iter() {
                libusb_release_interface(self.handle, iface as c_int);
            }
            libusb_close(self.handle);
        }
    }
}

unsafe impl<IoHandle, CtxMarker> Send for DeviceHandle<IoHandle, CtxMarker> {}
unsafe impl<IoHandle, CtxMarker> Sync for DeviceHandle<IoHandle, CtxMarker> {}

impl<IoHandle, CtxMarker> DeviceHandle<IoHandle, CtxMarker> {
    /// Returns the active configuration number.
    pub fn active_configuration(&self) -> ::Result<u8> {
        let mut config = unsafe { mem::uninitialized() };

        try_unsafe!(libusb_get_configuration(self.handle, &mut config));
        Ok(config as u8)
    }

    /// Sets the device's active configuration.
    pub fn set_active_configuration(&mut self, config: u8) -> ::Result<()> {
        try_unsafe!(libusb_set_configuration(self.handle, config as c_int));
        Ok(())
    }

    /// Puts the device in an unconfigured state.
    pub fn unconfigure(&mut self) -> ::Result<()> {
        try_unsafe!(libusb_set_configuration(self.handle, -1));
        Ok(())
    }

    /// Resets the device.
    pub fn reset(&mut self) -> ::Result<()> {
        try_unsafe!(libusb_reset_device(self.handle));
        Ok(())
    }

    /// Indicates whether the device has an attached kernel driver.
    ///
    /// This method is not supported on all platforms.
    pub fn kernel_driver_active(&self, iface: u8) -> ::Result<bool> {
        match unsafe { libusb_kernel_driver_active(self.handle, iface as c_int) } {
            0 => Ok(false),
            1 => Ok(true),
            err => Err(error::from_libusb(err)),
        }
    }

    /// Detaches an attached kernel driver from the device.
    ///
    /// This method is not supported on all platforms.
    pub fn detach_kernel_driver(&mut self, iface: u8) -> ::Result<()> {
        try_unsafe!(libusb_detach_kernel_driver(self.handle, iface as c_int));
        Ok(())
    }

    /// Attaches a kernel driver to the device.
    ///
    /// This method is not supported on all platforms.
    pub fn attach_kernel_driver(&mut self, iface: u8) -> ::Result<()> {
        try_unsafe!(libusb_attach_kernel_driver(self.handle, iface as c_int));
        Ok(())
    }

    /// Claims one of the device's interfaces.
    ///
    /// An interface must be claimed before operating on it. All claimed interfaces are released
    /// when the device handle goes out of scope.
    pub fn claim_interface(&mut self, iface: u8) -> ::Result<()> {
        try_unsafe!(libusb_claim_interface(self.handle, iface as c_int));
        self.interfaces.insert(iface as usize);
        Ok(())
    }

    /// Releases a claimed interface.
    pub fn release_interface(&mut self, iface: u8) -> ::Result<()> {
        try_unsafe!(libusb_release_interface(self.handle, iface as c_int));
        self.interfaces.remove((iface as usize));
        Ok(())
    }

    /// Sets an interface's active setting.
    pub fn set_alternate_setting(&mut self, iface: u8, setting: u8) -> ::Result<()> {
        try_unsafe!(libusb_set_interface_alt_setting(self.handle, iface as c_int, setting as c_int));
        Ok(())
    }
}

mod async_api {
    use std::fmt;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::borrow::Borrow;
    use std::time::Duration;
    use libc::c_uint;
    use libusb::*;
    use io::{AsyncIoType, AsyncIoTransferBuilderType};
    use super::DeviceHandle;

    macro_rules! fcsm {
        (       ;$($v:expr),*) => {};
        ($e:expr;$($v:expr),*) => { unsafe{_libusb_fill_control_setup($($v),*)} };
    }
    macro_rules! tb {
        ($( $fn_nam:ident {$($var:ident : $typ:ty),*} $fill:ident  {$($v1:ident),*} {$($len:ident),*} {$($nip:ident),*} {$($znip:expr),*} {$($fcs:expr),*} )*) => {

            pub trait DeviceHandleAsyncApi<'dh, IoHandle, CtxMarker>
                where IoHandle: 'dh+AsyncIoType<CtxMarker, Self::DhMarker>+Clone+fmt::Debug,
                      CtxMarker: 'dh+Clone+fmt::Debug
            {
                type DhMarker: 'dh+Borrow<DeviceHandle<IoHandle, CtxMarker>>+Clone+fmt::Debug;

                fn dh_marker(&'dh self) -> Self::DhMarker;

                $(
                    #[allow(non_snake_case)]
                    fn $fn_nam<F>(&'dh self, buf: Vec<u8>, timeout: Duration, callback: Option<F>, $( $var: $typ ),*) -> ::Result<<IoHandle as AsyncIoType<CtxMarker, Self::DhMarker>>::TransferHandle>
                        where     F: FnMut(<IoHandle as AsyncIoType<CtxMarker, Self::DhMarker>>::TransferCallbackData) -> <IoHandle as AsyncIoType<CtxMarker, Self::DhMarker>>::TransferCallbackResult,
                                  F: 'static,
                    {
                        let dh_marker = self.dh_marker();
                        let dh_ref = Borrow::<DeviceHandle<IoHandle, CtxMarker>>::borrow(&dh_marker);
                        // debug!("BUF: {:?}", buf);
                        let timeout_ms = (timeout.as_secs() * 1000 + timeout.subsec_nanos() as u64 / 1_000_000) as c_uint;
                        let tr = unsafe { libusb_alloc_transfer( $($nip),* $($znip),* ) };
                        let ar = dh_ref.io_handle.allocate(tr, dh_marker.clone(), callback.map(|x| Box::new(x) as Box<_>), buf);
                        // debug!("{:?}", ar);
                        fcsm!($($fcs),* ; ar.buf_ptr, $($var),*);
                        unsafe { $fill(tr, dh_ref.handle, $($v1,)* ar.buf_ptr, $(ar.$len,)* $($nip,)* ar.callback, ar.user_data_ptr, timeout_ms); }
                        ar.builder.submit()
                    }
                )*
            }

            impl<'dh, IoHandle, CtxMarker> DeviceHandleAsyncApi<'dh, IoHandle, CtxMarker> for DeviceHandle<IoHandle, CtxMarker>
                where IoHandle: 'dh+AsyncIoType<CtxMarker, &'dh Self>+Clone+fmt::Debug,
                      CtxMarker: 'dh+Clone+fmt::Debug
            {
                type DhMarker = &'dh Self;
                fn dh_marker(&'dh self) -> Self::DhMarker { self }
            }

            impl<'dh, IoHandle, CtxMarker> DeviceHandleAsyncApi<'dh, IoHandle, CtxMarker> for Rc<DeviceHandle<IoHandle, CtxMarker>>
                where IoHandle: 'dh+AsyncIoType<CtxMarker, Self>+Clone+fmt::Debug,
                      CtxMarker: 'dh+Clone+fmt::Debug
            {
                type DhMarker = Self;
                fn dh_marker(&'dh self) -> Self::DhMarker { self.clone() }
            }

            impl<'dh, IoHandle, CtxMarker> DeviceHandleAsyncApi<'dh, IoHandle, CtxMarker> for Arc<DeviceHandle<IoHandle, CtxMarker>>
                where IoHandle: 'dh+AsyncIoType<CtxMarker, Self>+Clone+fmt::Debug,
                      CtxMarker: 'dh+Clone+fmt::Debug
            {
                type DhMarker = Self;
                fn dh_marker(&'dh self) -> Self::DhMarker { self.clone() }
            }
        }
    }

    tb!(control      {bmRequestType: u8, bRequest: u8, wValue: u16, wIndex: u16 , wLength: u16}   _libusb_fill_control_transfer     {}                     {}     {}                 {0} {0}
        isochronous  {endpoint: u8, num_iso_packets: i32 }                                        _libusb_fill_iso_transfer         {endpoint}             {len}  {num_iso_packets}  {}  {}
        interrupt    {endpoint: u8 }                                                              _libusb_fill_interrupt_transfer   {endpoint}             {len}  {}                 {0} {}
        bulk         {endpoint: u8 }                                                              _libusb_fill_bulk_transfer        {endpoint}             {len}  {}                 {0} {}
        bulk_stream  {endpoint: u8, stream_id: u32 }                                              _libusb_fill_bulk_stream_transfer {endpoint, stream_id}  {len}  {}                 {0} {}
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix_async_io {
    use std::fmt;
    use std::borrow::Borrow;
    use std::time::Duration;
    use std::sync::mpsc::channel;
    use std::mem::size_of;
    use libusb::*;
    use super::{DeviceHandle, DeviceHandleAsyncApi};
    use context::Context;
    use device_handle_sync_api::DeviceHandleSyncApi;
    use io::unix_async::{UnixAsyncIo, UnixAsyncIoHandle, UnixAsyncIoCallbackResult};

    enum BufVar<'a> {
        In(&'a mut [u8]),
        Out(&'a [u8])
    }

    impl<'dh, CtxMarker> DeviceHandle<UnixAsyncIoHandle<CtxMarker>, CtxMarker>
        where CtxMarker: Borrow<Context<UnixAsyncIo>>+Clone+fmt::Debug
    {
        #[inline] fn control_msg<'a>(&'dh self, request_type: u8, request: u8, value: u16, index: u16, buf_var: BufVar<'a>, timeout: Duration) -> ::Result<usize> {
            let (snd, rcv) = channel();
            let callback = Some(move |dat| { snd.send(dat).expect("control message channel send error"); UnixAsyncIoCallbackResult::Handled });
            let csl = size_of::<libusb_control_setup>();
            let (v,s) = match buf_var {
                BufVar::In(ref buf) => {
                    let mut v = Vec::with_capacity(csl+buf.len());
                    v.resize(csl+buf.len(), 0);
                    (v, buf.len())
                },
                BufVar::Out(ref buf) => {
                    let mut v = Vec::with_capacity(csl+buf.len());
                    v.resize(csl, 0);
                    v.extend_from_slice(buf);
                    (v, buf.len())
                }
            };
            let _handle = self.control(v, timeout, callback, request_type, request, value, index, s as u16)?;
            match rcv.recv() {
                Ok(res) => {
                    if let BufVar::In(buf) = buf_var {
                        for i in 0..res.actual_length { buf[i] = res.buf[csl+i]; }
                    }
                    Ok(res.actual_length)
                },
                Err(e) => Err(format!("control message reveiver error: {:?}", e).into())
            }
        }

        #[inline] fn int_blk_msg<'a>(&'dh self, endpoint: u8, buf_var: BufVar<'a>, timeout: Duration, interrupt: bool) -> ::Result<usize> {
            let (snd, rcv) = channel();
            let callback = Some(move |dat| { snd.send(dat).expect("int_blk_msg channel send error"); UnixAsyncIoCallbackResult::Handled });
            let v = match buf_var {
                BufVar::In(ref buf) => {
                    let mut v = Vec::with_capacity(buf.len());
                    v.resize(buf.len(), 0);
                    v
                },
                BufVar::Out(ref buf) => {
                    let mut v = Vec::with_capacity(buf.len());
                    v.extend_from_slice(buf);
                    v
                }
            };
            let _handle = if interrupt {
                self.interrupt(v, timeout, callback, endpoint)?
            } else {
                self.bulk(v, timeout, callback, endpoint)?
            };
            match rcv.recv() {
                Ok(res) => {
                    if let BufVar::In(buf) = buf_var {
                        for i in 0..res.actual_length { buf[i] = res.buf[i]; }
                    }
                    Ok(res.actual_length)
                },
                Err(e) => Err(format!("int_blk_msg message reveiver error: {:?}", e).into())
            }
        }
    }

    impl<'dh, CtxMarker> DeviceHandleSyncApi for DeviceHandle<UnixAsyncIoHandle<CtxMarker>, CtxMarker>
        where CtxMarker: Borrow<Context<UnixAsyncIo>>+Clone+fmt::Debug
    {
        fn read_interrupt(&self, endpoint: u8, buf: &mut [u8], timeout: Duration) -> ::Result<usize> {
            if endpoint & LIBUSB_ENDPOINT_DIR_MASK != LIBUSB_ENDPOINT_IN { return Err(::Error::InvalidParam); }
            self.int_blk_msg(endpoint, BufVar::In(buf), timeout, true)
        }

        fn write_interrupt(&self, endpoint: u8, buf: &[u8], timeout: Duration) -> ::Result<usize> {
            if endpoint & LIBUSB_ENDPOINT_DIR_MASK != LIBUSB_ENDPOINT_OUT { return Err(::Error::InvalidParam); }
            self.int_blk_msg(endpoint, BufVar::Out(buf), timeout, true)
        }

        fn read_bulk(&self, endpoint: u8, buf: &mut [u8], timeout: Duration) -> ::Result<usize> {
            if endpoint & LIBUSB_ENDPOINT_DIR_MASK != LIBUSB_ENDPOINT_IN { return Err(::Error::InvalidParam); }
            self.int_blk_msg(endpoint, BufVar::In(buf), timeout, false)
        }

        fn write_bulk(&self, endpoint: u8, buf: &[u8], timeout: Duration) -> ::Result<usize> {
            if endpoint & LIBUSB_ENDPOINT_DIR_MASK != LIBUSB_ENDPOINT_OUT { return Err(::Error::InvalidParam); }
            self.int_blk_msg(endpoint, BufVar::Out(buf), timeout, false)
        }

        fn read_control(&self, request_type: u8, request: u8, value: u16, index: u16, buf: &mut [u8], timeout: Duration) -> ::Result<usize> {
            if request_type & LIBUSB_ENDPOINT_DIR_MASK != LIBUSB_ENDPOINT_IN { return Err(::Error::InvalidParam); }
            self.control_msg(request_type, request, value, index, BufVar::In(buf), timeout)
        }

        fn write_control(&self, request_type: u8, request: u8, value: u16, index: u16, buf: &[u8], timeout: Duration) -> ::Result<usize> {
            if request_type & LIBUSB_ENDPOINT_DIR_MASK != LIBUSB_ENDPOINT_OUT { return Err(::Error::InvalidParam); }
            self.control_msg(request_type, request, value, index, BufVar::Out(buf), timeout)
        }
    }
}

mod sync_io {
    use std::mem;
    use std::time::Duration;
    use libc::{c_int, c_uint, c_uchar};
    use libusb::*;

    use io::sync::SyncIo;
    use error::{self, Error};
    use device_handle_sync_api::DeviceHandleSyncApi;
    use super::DeviceHandle;

    impl<CtxMarker> DeviceHandleSyncApi for DeviceHandle<SyncIo, CtxMarker> {
        /// Reads from an interrupt endpoint.
        ///
        /// This function attempts to read from the interrupt endpoint with the address given by the
        /// `endpoint` parameter and fills `buf` with any data received from the endpoint. The function
        /// blocks up to the amount of time specified by `timeout`.
        ///
        /// If the return value is `Ok(n)`, then `buf` is populated with `n` bytes of data received
        /// from the endpoint.
        ///
        /// ## Errors
        ///
        /// If this function encounters any form of error while fulfilling the transfer request, an
        /// error variant will be returned. If an error variant is returned, no bytes were read.
        ///
        /// The errors returned by this function include:
        ///
        /// * `InvalidParam` if the endpoint is not an input endpoint.
        /// * `Timeout` if the transfer timed out.
        /// * `Pipe` if the endpoint halted.
        /// * `Overflow` if the device offered more data.
        /// * `NoDevice` if the device has been disconnected.
        /// * `Io` if the transfer encountered an I/O error.
        fn read_interrupt(&self, endpoint: u8, buf: &mut [u8], timeout: Duration) -> ::Result<usize> {
            if endpoint & LIBUSB_ENDPOINT_DIR_MASK != LIBUSB_ENDPOINT_IN {
                return Err(Error::InvalidParam);
            }

            let mut transferred: c_int = unsafe { mem::uninitialized() };

            let ptr = buf.as_mut_ptr() as *mut c_uchar;
            let len = buf.len() as c_int;
            let timeout_ms = (timeout.as_secs() * 1000 + timeout.subsec_nanos() as u64 / 1_000_000) as c_uint;

            match unsafe { libusb_interrupt_transfer(self.handle, endpoint, ptr, len, &mut transferred, timeout_ms) } {
                0 => {
                    Ok(transferred as usize)
                },
                err => {
                    if err == LIBUSB_ERROR_INTERRUPTED && transferred > 0 {
                        Ok(transferred as usize)
                    }
                    else {
                        Err(error::from_libusb(err))
                    }
                },
            }
        }

        /// Writes to an interrupt endpoint.
        ///
        /// This function attempts to write the contents of `buf` to the interrupt endpoint with the
        /// address given by the `endpoint` parameter. The function blocks up to the amount of time
        /// specified by `timeout`.
        ///
        /// If the return value is `Ok(n)`, then `n` bytes of `buf` were written to the endpoint.
        ///
        /// ## Errors
        ///
        /// If this function encounters any form of error while fulfilling the transfer request, an
        /// error variant will be returned. If an error variant is returned, no bytes were written.
        ///
        /// The errors returned by this function include:
        ///
        /// * `InvalidParam` if the endpoint is not an output endpoint.
        /// * `Timeout` if the transfer timed out.
        /// * `Pipe` if the endpoint halted.
        /// * `NoDevice` if the device has been disconnected.
        /// * `Io` if the transfer encountered an I/O error.
        fn write_interrupt(&self, endpoint: u8, buf: &[u8], timeout: Duration) -> ::Result<usize> {
            if endpoint & LIBUSB_ENDPOINT_DIR_MASK != LIBUSB_ENDPOINT_OUT {
                return Err(Error::InvalidParam);
            }

            let mut transferred: c_int = unsafe { mem::uninitialized() };

            let ptr = buf.as_ptr() as *mut c_uchar;
            let len = buf.len() as c_int;
            let timeout_ms = (timeout.as_secs() * 1000 + timeout.subsec_nanos() as u64 / 1_000_000) as c_uint;

            match unsafe { libusb_interrupt_transfer(self.handle, endpoint, ptr, len, &mut transferred, timeout_ms) } {
                0 => {
                    Ok(transferred as usize)
                },
                err => {
                    if err == LIBUSB_ERROR_INTERRUPTED && transferred > 0 {
                        Ok(transferred as usize)
                    }
                    else {
                        Err(error::from_libusb(err))
                    }
                },
            }
        }

        /// Reads from a bulk endpoint.
        ///
        /// This function attempts to read from the bulk endpoint with the address given by the
        /// `endpoint` parameter and fills `buf` with any data received from the endpoint. The function
        /// blocks up to the amount of time specified by `timeout`.
        ///
        /// If the return value is `Ok(n)`, then `buf` is populated with `n` bytes of data received
        /// from the endpoint.
        ///
        /// ## Errors
        ///
        /// If this function encounters any form of error while fulfilling the transfer request, an
        /// error variant will be returned. If an error variant is returned, no bytes were read.
        ///
        /// The errors returned by this function include:
        ///
        /// * `InvalidParam` if the endpoint is not an input endpoint.
        /// * `Timeout` if the transfer timed out.
        /// * `Pipe` if the endpoint halted.
        /// * `Overflow` if the device offered more data.
        /// * `NoDevice` if the device has been disconnected.
        /// * `Io` if the transfer encountered an I/O error.
        fn read_bulk(&self, endpoint: u8, buf: &mut [u8], timeout: Duration) -> ::Result<usize> {
            if endpoint & LIBUSB_ENDPOINT_DIR_MASK != LIBUSB_ENDPOINT_IN {
                return Err(Error::InvalidParam);
            }

            let mut transferred: c_int = unsafe { mem::uninitialized() };

            let ptr = buf.as_mut_ptr() as *mut c_uchar;
            let len = buf.len() as c_int;
            let timeout_ms = (timeout.as_secs() * 1000 + timeout.subsec_nanos() as u64 / 1_000_000) as c_uint;

            match unsafe { libusb_bulk_transfer(self.handle, endpoint, ptr, len, &mut transferred, timeout_ms) } {
                0 => {
                    Ok(transferred as usize)
                },
                err => {
                    if err == LIBUSB_ERROR_INTERRUPTED && transferred > 0 {
                        Ok(transferred as usize)
                    }
                    else {
                        Err(error::from_libusb(err))
                    }
                },
            }
        }

        /// Writes to a bulk endpoint.
        ///
        /// This function attempts to write the contents of `buf` to the bulk endpoint with the address
        /// given by the `endpoint` parameter. The function blocks up to the amount of time specified
        /// by `timeout`.
        ///
        /// If the return value is `Ok(n)`, then `n` bytes of `buf` were written to the endpoint.
        ///
        /// ## Errors
        ///
        /// If this function encounters any form of error while fulfilling the transfer request, an
        /// error variant will be returned. If an error variant is returned, no bytes were written.
        ///
        /// The errors returned by this function include:
        ///
        /// * `InvalidParam` if the endpoint is not an output endpoint.
        /// * `Timeout` if the transfer timed out.
        /// * `Pipe` if the endpoint halted.
        /// * `NoDevice` if the device has been disconnected.
        /// * `Io` if the transfer encountered an I/O error.
        fn write_bulk(&self, endpoint: u8, buf: &[u8], timeout: Duration) -> ::Result<usize> {
            if endpoint & LIBUSB_ENDPOINT_DIR_MASK != LIBUSB_ENDPOINT_OUT {
                return Err(Error::InvalidParam);
            }

            let mut transferred: c_int = unsafe { mem::uninitialized() };

            let ptr = buf.as_ptr() as *mut c_uchar;
            let len = buf.len() as c_int;
            let timeout_ms = (timeout.as_secs() * 1000 + timeout.subsec_nanos() as u64 / 1_000_000) as c_uint;

            match unsafe { libusb_bulk_transfer(self.handle, endpoint, ptr, len, &mut transferred, timeout_ms) } {
                0 => {
                    Ok(transferred as usize)
                },
                err => {
                    if err == LIBUSB_ERROR_INTERRUPTED && transferred > 0 {
                        Ok(transferred as usize)
                    }
                    else {
                        Err(error::from_libusb(err))
                    }
                },
            }
        }

        /// Reads data using a control transfer.
        ///
        /// This function attempts to read data from the device using a control transfer and fills
        /// `buf` with any data received during the transfer. The function blocks up to the amount of
        /// time specified by `timeout`.
        ///
        /// The parameters `request_type`, `request`, `value`, and `index` specify the fields of the
        /// control transfer setup packet (`bmRequestType`, `bRequest`, `wValue`, and `wIndex`
        /// respectively). The values for each of these parameters shall be given in host-endian byte
        /// order. The value for the `request_type` parameter can be built with the helper function,
        /// [request_type()](fn.request_type.html). The meaning of the other parameters depends on the
        /// type of control request.
        ///
        /// If the return value is `Ok(n)`, then `buf` is populated with `n` bytes of data.
        ///
        /// ## Errors
        ///
        /// If this function encounters any form of error while fulfilling the transfer request, an
        /// error variant will be returned. If an error variant is returned, no bytes were read.
        ///
        /// The errors returned by this function include:
        ///
        /// * `InvalidParam` if `request_type` does not specify a read transfer.
        /// * `Timeout` if the transfer timed out.
        /// * `Pipe` if the control request was not supported by the device.
        /// * `NoDevice` if the device has been disconnected.
        /// * `Io` if the transfer encountered an I/O error.
        fn read_control(&self, request_type: u8, request: u8, value: u16, index: u16, buf: &mut [u8], timeout: Duration) -> ::Result<usize> {
            if request_type & LIBUSB_ENDPOINT_DIR_MASK != LIBUSB_ENDPOINT_IN {
                return Err(Error::InvalidParam);
            }

            let ptr = buf.as_mut_ptr() as *mut c_uchar;
            let len = buf.len() as u16;
            let timeout_ms = (timeout.as_secs() * 1000 + timeout.subsec_nanos() as u64 / 1_000_000) as c_uint;

            let res = unsafe {
                libusb_control_transfer(self.handle, request_type, request, value, index, ptr, len, timeout_ms)
            };

            if res < 0 {
                Err(error::from_libusb(res))
            } else {
                Ok(res as usize)
            }
        }

        /// Writes data using a control transfer.
        ///
        /// This function attempts to write the contents of `buf` to the device using a control
        /// transfer. The function blocks up to the amount of time specified by `timeout`.
        ///
        /// The parameters `request_type`, `request`, `value`, and `index` specify the fields of the
        /// control transfer setup packet (`bmRequestType`, `bRequest`, `wValue`, and `wIndex`
        /// respectively). The values for each of these parameters shall be given in host-endian byte
        /// order. The value for the `request_type` parameter can be built with the helper function,
        /// [request_type()](fn.request_type.html). The meaning of the other parameters depends on the
        /// type of control request.
        ///
        /// If the return value is `Ok(n)`, then `n` bytes of `buf` were transfered.
        ///
        /// ## Errors
        ///
        /// If this function encounters any form of error while fulfilling the transfer request, an
        /// error variant will be returned. If an error variant is returned, no bytes were read.
        ///
        /// The errors returned by this function include:
        ///
        /// * `InvalidParam` if `request_type` does not specify a write transfer.
        /// * `Timeout` if the transfer timed out.
        /// * `Pipe` if the control request was not supported by the device.
        /// * `NoDevice` if the device has been disconnected.
        /// * `Io` if the transfer encountered an I/O error.
        fn write_control(&self, request_type: u8, request: u8, value: u16, index: u16, buf: &[u8], timeout: Duration) -> ::Result<usize> {
            if request_type & LIBUSB_ENDPOINT_DIR_MASK != LIBUSB_ENDPOINT_OUT {
                return Err(Error::InvalidParam);
            }

            let ptr = buf.as_ptr() as *mut c_uchar;
            let len = buf.len() as u16;
            let timeout_ms = (timeout.as_secs() * 1000 + timeout.subsec_nanos() as u64 / 1_000_000) as c_uint;

            let res = unsafe {
                libusb_control_transfer(self.handle, request_type, request, value, index, ptr, len, timeout_ms)
            };

            if res < 0 {
                Err(error::from_libusb(res))
            } else {
                Ok(res as usize)
            }
        }
    }
}

#[doc(hidden)]
pub unsafe fn from_libusb<IoHandle, CtxMarker>(ctx_marker: CtxMarker, io_handle: IoHandle, handle: *mut libusb_device_handle) -> DeviceHandle<IoHandle, CtxMarker> {
    DeviceHandle {
        ctx_marker: ctx_marker,
        io_handle: io_handle,
        handle: handle,
        interfaces: BitSet::with_capacity(u8::max_value() as usize + 1),
    }
}
