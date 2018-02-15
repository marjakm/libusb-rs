use std::marker::PhantomData;
use std::slice;

use libusb::*;

use io::IoType;
use context::Context;
use device::{self, Device};


/// A list of detected USB devices.
pub struct DeviceList<'ctx, Io>
    where Io: IoType<'ctx>,
{
    context: PhantomData<&'ctx Context<Io>>,
    io_handle: <Io as IoType<'ctx>>::Handle,
    list: *const *mut libusb_device,
    len: usize,
}

impl<'ctx, Io> Drop for DeviceList<'ctx, Io>
    where Io: IoType<'ctx>,
{
    /// Frees the device list.
    fn drop(&mut self) {
        unsafe {
            libusb_free_device_list(self.list, 1);
        }
    }
}

impl<'ctx, Io> DeviceList<'ctx, Io>
    where Io: IoType<'ctx>,
{
    /// Returns the number of devices in the list.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns an iterator over the devices in the list.
    ///
    /// The iterator yields a sequence of `Device` objects.
    pub fn iter<'dl>(&'dl self) -> Devices<'ctx, 'dl, Io> {
        Devices {
            context: PhantomData,
            io_handle: self.io_handle.clone(),
            devices: unsafe { slice::from_raw_parts(self.list, self.len) },
            index: 0,
        }
    }
}

/// Iterator over detected USB devices.
pub struct Devices<'ctx, 'dl, Io>
    where Io: IoType<'ctx>,
{
    context: PhantomData<&'ctx Context<Io>>,
    io_handle: <Io as IoType<'ctx>>::Handle,
    devices: &'dl [*mut libusb_device],
    index: usize,
}

impl<'ctx, 'dl, Io> Iterator for Devices<'ctx, 'dl, Io>
    where Io: IoType<'ctx>,
{
    type Item = Device<'ctx, Io>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.devices.len() {
            let device = self.devices[self.index];

            self.index += 1;
            Some(unsafe { device::from_libusb(self.context, self.io_handle.clone(), device) })
        }
        else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.devices.len() - self.index;
        (remaining, Some(remaining))
    }
}


#[doc(hidden)]
pub unsafe fn from_libusb<'ctx, Io>(_context: &'ctx Context<Io>, io_handle: <Io as IoType<'ctx>>::Handle, list: *const *mut libusb_device, len: usize,) -> DeviceList<'ctx, Io>
    where Io: IoType<'ctx>,
{
    DeviceList {
        context: PhantomData,
        io_handle: io_handle,
        list: list,
        len: len,
    }
}
