use std::slice;

use libusb::*;

use context::Context;
use device::{self, Device};

/// A list of detected USB devices.
pub struct DeviceList<'ctx, Io: 'static> {
    context: &'ctx Context<Io>,
    list: *const *mut libusb_device,
    len: usize,
}

impl<'ctx, Io> Drop for DeviceList<'ctx, Io> {
    /// Frees the device list.
    fn drop(&mut self) {
        unsafe {
            libusb_free_device_list(self.list, 1);
        }
    }
}

impl<'ctx, Io> DeviceList<'ctx, Io> {
    /// Returns the number of devices in the list.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns an iterator over the devices in the list.
    ///
    /// The iterator yields a sequence of `Device` objects.
    pub fn iter<'dl>(&'dl self) -> Devices<'ctx, 'dl, Io> {
        Devices {
            context: self.context,
            devices: unsafe { slice::from_raw_parts(self.list, self.len) },
            index: 0,
        }
    }
}

/// Iterator over detected USB devices.
pub struct Devices<'ctx, 'dl, Io: 'static> {
    context: &'ctx Context<Io>,
    devices: &'dl [*mut libusb_device],
    index: usize,
}

impl<'ctx, 'dl, Io> Iterator for Devices<'ctx, 'dl, Io> {
    type Item = Device<'ctx, Io>;

    fn next(&mut self) -> Option<Device<'ctx, Io>> {
        if self.index < self.devices.len() {
            let device = self.devices[self.index];

            self.index += 1;
            Some(unsafe { device::from_libusb(self.context, device) })
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
pub unsafe fn from_libusb<'ctx, Io>(context: &'ctx Context<Io>, list: *const *mut libusb_device, len: usize,) -> DeviceList<'ctx, Io> {
    DeviceList {
        context: context,
        list: list,
        len: len,
    }
}
