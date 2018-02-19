use std::slice;

use libusb::*;

use io::IoType;
use context::Context;
use device::{self, Device};


/// A list of detected USB devices.
pub struct DeviceList<IoHandle, CtxMarker> {
    ctx_marker: CtxMarker,
    io_handle: IoHandle,
    list: *const *mut libusb_device,
    len: usize,
}

impl<IoHandle, CtxMarker> Drop for DeviceList<IoHandle, CtxMarker> {
    /// Frees the device list.
    fn drop(&mut self) {
        unsafe {
            libusb_free_device_list(self.list, 1);
        }
    }
}

impl<IoHandle, CtxMarker> DeviceList<IoHandle, CtxMarker>
    where IoHandle: Clone,
          CtxMarker: Clone,
{
    /// Returns the number of devices in the list.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns an iterator over the devices in the list.
    ///
    /// The iterator yields a sequence of `Device` objects.
    pub fn iter<'dl>(&'dl self) -> Devices<'dl, IoHandle, CtxMarker> {
        Devices {
            ctx_marker: self.ctx_marker.clone(),
            io_handle: self.io_handle.clone(),
            devices: unsafe { slice::from_raw_parts(self.list, self.len) },
            index: 0,
        }
    }
}

/// Iterator over detected USB devices.
pub struct Devices<'dl, IoHandle, CtxMarker> {
    ctx_marker: CtxMarker,
    io_handle: IoHandle,
    devices: &'dl [*mut libusb_device],
    index: usize,
}

impl<'dl, IoHandle, CtxMarker> Iterator for Devices<'dl, IoHandle, CtxMarker>
    where IoHandle: Clone,
          CtxMarker: Clone,
{
    type Item = Device<IoHandle, CtxMarker>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.devices.len() {
            let device = self.devices[self.index];

            self.index += 1;
            Some(unsafe { device::from_libusb(self.ctx_marker.clone(), self.io_handle.clone(), device) })
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
pub unsafe fn from_libusb<IoHandle, CtxMarker>(ctx_marker: CtxMarker, io_handle: IoHandle, list: *const *mut libusb_device, len: usize) -> DeviceList<IoHandle, CtxMarker>
{
    DeviceList {
        ctx_marker: ctx_marker,
        io_handle: io_handle,
        list: list,
        len: len,
    }
}
