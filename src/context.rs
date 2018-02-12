use std::marker::PhantomData;
use std::mem;
use libc::c_int;
use libusb::*;

use io::{IoType, IoRefType};
use device_list::{self, DeviceList};
use device_handle::{self, DeviceHandle};
use error;

/// A `libusb` context.
pub struct Context<Io> {
    context: *mut libusb_context,
    io: Io,
}

impl<Io> Drop for Context<Io> {
    /// Closes the `libusb` context.
    fn drop(&mut self) {
        unsafe {
            libusb_exit(self.context);
        }
    }
}

unsafe impl<Io> Sync for Context<Io> {}
unsafe impl<Io> Send for Context<Io> {}

impl<Io> Context<Io>
    where Io: IoType,
          for<'io> &'io Io: IoRefType
{
    /// Opens a new `libusb` context.
    pub fn new() -> ::Result<Self> {
        let mut context = unsafe { mem::uninitialized() };

        try_unsafe!(libusb_init(&mut context));

        Ok(Context { io: Io::new(context), context: context })
    }

    /// Sets the log level of a `libusb` context.
    pub fn set_log_level(&mut self, level: LogLevel) {
        unsafe {
            libusb_set_debug(self.context, level.as_c_int());
        }
    }

    pub fn has_capability(&self) -> bool {
        unsafe {
            libusb_has_capability(LIBUSB_CAP_HAS_CAPABILITY) != 0
        }
    }

    /// Tests whether the running `libusb` library supports hotplug.
    pub fn has_hotplug(&self) -> bool {
        unsafe {
            libusb_has_capability(LIBUSB_CAP_HAS_HOTPLUG) != 0
        }
    }

    /// Tests whether the running `libusb` library has HID access.
    pub fn has_hid_access(&self) -> bool {
        unsafe {
            libusb_has_capability(LIBUSB_CAP_HAS_HID_ACCESS) != 0
        }
    }

    /// Tests whether the running `libusb` library supports detaching the kernel driver.
    pub fn supports_detach_kernel_driver(&self) -> bool {
        unsafe {
            libusb_has_capability(LIBUSB_CAP_SUPPORTS_DETACH_KERNEL_DRIVER) != 0
        }
    }

    /// Returns a list of the current USB devices. The context must outlive the device list.
    pub fn devices<'ctx>(&'ctx self) -> ::Result<DeviceList<'ctx, Io, <&'ctx Io as IoRefType>::Handle>> {
        let mut list: *const *mut libusb_device = unsafe { mem::uninitialized() };

        let n = unsafe { libusb_get_device_list(self.context, &mut list) };

        if n < 0 {
            Err(error::from_libusb(n as c_int))
        }
        else {
            Ok(unsafe { device_list::from_libusb(self, (&self.io).handle(), list, n as usize) })
        }
    }

    /// Convenience function to open a device by its vendor ID and product ID.
    ///
    /// This function is provided as a convenience for building prototypes without having to
    /// iterate a [`DeviceList`](struct.DeviceList.html). It is not meant for production
    /// applications.
    ///
    /// Returns a device handle for the first device found matching `vendor_id` and `product_id`.
    /// On error, or if the device could not be found, it returns `None`.
    pub fn open_device_with_vid_pid<'ctx>(&'ctx self, vendor_id: u16, product_id: u16) -> Option<DeviceHandle<'ctx, Io, <&'ctx Io as IoRefType>::Handle>> {
        let handle = unsafe { libusb_open_device_with_vid_pid(self.context, vendor_id, product_id) };

        if handle.is_null() {
            None
        }
        else {
            Some(unsafe { device_handle::from_libusb(PhantomData, (&self.io).handle(), handle) })
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod async_io {
    use std::io;
    use std::ptr;
    use std::slice;
    use std::os::unix::io::RawFd;
    use std::thread::sleep;
    use std::time::Duration;
    use mio::event::Evented;
    use mio::unix::EventedFd;
    use mio::{Poll, Token, Ready, PollOpt};
    use libc::{POLLIN, POLLOUT};
    use libusb::*;

    use ::io::async::AsyncIo;
    use ::error::from_libusb;
    use super::Context;

    impl Context<AsyncIo> {
        pub fn handle(&self, poll: &Poll) -> ::Result<()> {
            let mut ir = self.io.reg.lock().expect("Could not unlock AsyncIo reg mutex");
            match (*ir).as_mut() {
                None => return Err("Register in Poll before calling handle".into()),
                Some(ofds) => {
                    let res = match unsafe { libusb_handle_events_locked(self.context, ptr::null()) } {
                        0 => Ok(()),
                        e => Err(from_libusb(e))
                    };
                    if unsafe { libusb_event_handling_ok(self.context) } == 0 {
                        unsafe { libusb_unlock_events(self.context) };
                        self.spin_until_locked_and_ok_to_handle_events();
                    }
                    let fds = self.get_pollfd_list();
                    if ofds.1 != fds {
                        for &(ref fd, _) in ofds.1.iter() {
                            poll.deregister(&EventedFd(fd)).map_err(|e| e.to_string())?;
                        }
                        for &(ref fd, ref rdy) in fds.iter() {
                            poll.register(&EventedFd(fd), ofds.0, *rdy, PollOpt::level()).map_err(|e| e.to_string())?;
                        }
                    }
                    ofds.1 = fds;
                    res
                }
            }
        }

        fn get_pollfd_list(&self) -> Vec<(RawFd, Ready)> {
            let pfdl = unsafe { libusb_get_pollfds(self.context) };
            let mut v = Vec::new();
            let sl: &[*mut libusb_pollfd] = unsafe { slice::from_raw_parts(pfdl, 1024) };
            while let Some(x) = sl.iter().next() {
                if *x == ptr::null_mut() { break; }
                let pfd = unsafe { &**x as &libusb_pollfd };
                let mut rdy = Ready::empty();
                if (pfd.events & POLLIN ) != 0 { rdy = rdy | Ready::readable(); }
                if (pfd.events & POLLOUT) != 0 { rdy = rdy | Ready::writable(); }
                v.push((pfd.fd, rdy));
            }
            unsafe { libusb_free_pollfds(pfdl) };
            v.sort();
            v
        }

        fn spin_until_locked_and_ok_to_handle_events(&self) {
            'retry: loop {
                if unsafe { libusb_try_lock_events(self.context) } == 0 {
                    // got lock
                    if unsafe { libusb_event_handling_ok(self.context) } == 0 {
                        unsafe { libusb_unlock_events(self.context) };
                        warn!("libusb_event_handling_ok returned not ok, busy wait until ok (with 10ms sleep)");
                        sleep(Duration::from_millis(10));
                        continue 'retry;
                    }
                    break
                } else {
                    warn!("could not get events lock with libusb_try_lock_events, busy wait until ok (with 10ms sleep)");
                    sleep(Duration::from_millis(10));
                }
            }
        }
    }

    impl Evented for Context<AsyncIo> {
        fn register(&self, poll: &Poll, token: Token, _interest: Ready, _opts: PollOpt) -> io::Result<()> {
            let mut ir = self.io.reg.lock().expect("Could not unlock AsyncIo reg mutex");
            if ir.is_some() { panic!("It is not safe to register libusb file descriptors multiple times") }
            self.spin_until_locked_and_ok_to_handle_events();
            let fds = self.get_pollfd_list();
            for &(ref fd, ref rdy) in fds.iter() {
                poll.register(&EventedFd(fd), token, *rdy, PollOpt::level())?;
            }
            *ir = Some((token, fds));
            Ok(())
        }

        fn reregister(&self, poll: &Poll, token: Token, interest: Ready, _opts: PollOpt) -> io::Result<()> {
            self.deregister(poll)?;
            self.register(poll, token, interest, PollOpt::level())
        }

        fn deregister(&self, poll: &Poll) -> io::Result<()> {
            let mut ir = self.io.reg.lock().expect("Could not unlock AsyncIo reg mutex");
            match ir.take() {
                Some((_, fds)) => for (fd, _) in fds.into_iter() { poll.deregister(&EventedFd(&fd))?; },
                None => panic!("Unable to deregister libusb file descriptors when they are not registered")
            }
            unsafe { libusb_unlock_events(self.context) };
            Ok(())
        }
    }
}


/// Library logging levels.
pub enum LogLevel {
    /// No messages are printed by `libusb` (default).
    None,

    /// Error messages printed to `stderr`.
    Error,

    /// Warning and error messages are printed to `stderr`.
    Warning,

    /// Informational messages are printed to `stdout`. Warnings and error messages are printed to
    /// `stderr`.
    Info,

    /// Debug and informational messages are printed to `stdout`. Warnings and error messages are
    /// printed to `stderr`.
    Debug,
}

impl LogLevel {
    fn as_c_int(&self) -> c_int {
        match *self {
            LogLevel::None    => LIBUSB_LOG_LEVEL_NONE,
            LogLevel::Error   => LIBUSB_LOG_LEVEL_ERROR,
            LogLevel::Warning => LIBUSB_LOG_LEVEL_WARNING,
            LogLevel::Info    => LIBUSB_LOG_LEVEL_INFO,
            LogLevel::Debug   => LIBUSB_LOG_LEVEL_DEBUG,
        }
    }
}
