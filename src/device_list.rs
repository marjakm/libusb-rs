use std::marker::PhantomData;
use std::slice;

use libusb::*;

use context::Context;
use device::{self, Device};


/// A list of detected USB devices.
pub struct DeviceList<'ctx, Io, IoRef>
    where Io: 'static,
{
    context: PhantomData<&'ctx Context<Io>>,
    ioref: IoRef,
    list: *const *mut libusb_device,
    len: usize,
}

impl<'ctx, Io, IoRef> Drop for DeviceList<'ctx, Io, IoRef> {
    /// Frees the device list.
    fn drop(&mut self) {
        unsafe {
            libusb_free_device_list(self.list, 1);
        }
    }
}

impl<'ctx, Io, IoRef> DeviceList<'ctx, Io, IoRef>
    where IoRef: Clone
{
    /// Returns the number of devices in the list.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns an iterator over the devices in the list.
    ///
    /// The iterator yields a sequence of `Device` objects.
    pub fn iter<'dl>(&'dl self) -> Devices<'ctx, 'dl, Io, IoRef> {
        Devices {
            context: PhantomData,
            ioref: self.ioref.clone(),
            devices: unsafe { slice::from_raw_parts(self.list, self.len) },
            index: 0,
        }
    }
}

/// Iterator over detected USB devices.
pub struct Devices<'ctx, 'dl, Io, IoRef>
    where Io: 'static,
{
    context: PhantomData<&'ctx Context<Io>>,
    ioref: IoRef,
    devices: &'dl [*mut libusb_device],
    index: usize,
}

impl<'ctx, 'dl, Io, IoRef> Iterator for Devices<'ctx, 'dl, Io, IoRef>
    where IoRef: Clone
{
    type Item = Device<'ctx, Io, IoRef>;

    fn next(&mut self) -> Option<Device<'ctx, Io, IoRef>> {
        if self.index < self.devices.len() {
            let device = self.devices[self.index];

            self.index += 1;
            Some(unsafe { device::from_libusb(self.context, self.ioref.clone(), device) })
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
pub unsafe fn from_libusb<'ctx, Io, IoRef>(_context: &'ctx Context<Io>, ioref: IoRef, list: *const *mut libusb_device, len: usize,) -> DeviceList<'ctx, Io, IoRef> {
    DeviceList {
        context: PhantomData,
        ioref: ioref,
        list: list,
        len: len,
    }
}
