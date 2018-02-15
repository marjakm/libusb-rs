use std::marker::PhantomData;
use std::mem;

use libusb::*;

use io::IoType;
use context::Context;
use device_handle::{self, DeviceHandle};
use device_descriptor::{self, DeviceDescriptor};
use config_descriptor::{self, ConfigDescriptor};
use fields::{self, Speed};


/// A reference to a USB device.
pub struct Device<'ctx, Io>
    where Io: IoType<'ctx>,
{
    context: PhantomData<&'ctx Context<Io>>,
    io_handle: <Io as IoType<'ctx>>::Handle,
    device: *mut libusb_device,
}

impl<'ctx, Io> Drop for Device<'ctx, Io>
    where Io: IoType<'ctx>,
{
    /// Releases the device reference.
    fn drop(&mut self) {
        unsafe {
            libusb_unref_device(self.device);
        }
    }
}

unsafe impl<'ctx, Io: IoType<'ctx>> Send for Device<'ctx, Io> {}
unsafe impl<'ctx, Io: IoType<'ctx>> Sync for Device<'ctx, Io> {}

impl<'ctx, Io> Device<'ctx, Io>
    where Io: IoType<'ctx>,
{
    /// Reads the device descriptor.
    pub fn device_descriptor(&self) -> ::Result<DeviceDescriptor> {
        let mut descriptor: libusb_device_descriptor = unsafe { mem::uninitialized() };

        // since libusb 1.0.16, this function always succeeds
        try_unsafe!(libusb_get_device_descriptor(self.device, &mut descriptor));

        Ok(device_descriptor::from_libusb(descriptor))
    }

    /// Reads a configuration descriptor.
    pub fn config_descriptor(&self, config_index: u8) -> ::Result<ConfigDescriptor> {
        let mut config: *const libusb_config_descriptor = unsafe { mem::uninitialized() };

        try_unsafe!(libusb_get_config_descriptor(self.device, config_index, &mut config));

        Ok(unsafe { config_descriptor::from_libusb(config) })
    }

    /// Reads the configuration descriptor for the current configuration.
    pub fn active_config_descriptor(&self) -> ::Result<ConfigDescriptor> {
        let mut config: *const libusb_config_descriptor = unsafe { mem::uninitialized() };

        try_unsafe!(libusb_get_active_config_descriptor(self.device, &mut config));

        Ok(unsafe { config_descriptor::from_libusb(config) })
    }

    /// Returns the number of the bus that the device is connected to.
    pub fn bus_number(&self) -> u8 {
        unsafe {
            libusb_get_bus_number(self.device)
        }
    }

    /// Returns the device's address on the bus that it's connected to.
    pub fn address(&self) -> u8 {
        unsafe {
            libusb_get_device_address(self.device)
        }
    }

    /// Returns the device's connection speed.
    pub fn speed(&self) -> Speed {
        fields::speed_from_libusb(unsafe {
            libusb_get_device_speed(self.device)
        })
    }

    /// Opens the device.
    pub fn open(&self) -> ::Result<DeviceHandle<'ctx, Io>> {
        let mut handle: *mut libusb_device_handle = unsafe { mem::uninitialized() };

        try_unsafe!(libusb_open(self.device, &mut handle));

        Ok(unsafe { device_handle::from_libusb(PhantomData, self.io_handle.clone(), handle) })
    }
}

#[doc(hidden)]
pub unsafe fn from_libusb<'ctx, Io>(context: PhantomData<&'ctx Context<Io>>, io_handle: <Io as IoType<'ctx>>::Handle, device: *mut libusb_device) -> Device<'ctx, Io>
    where Io: IoType<'ctx>,
{
    libusb_ref_device(device);

    Device {
        context: context,
        io_handle: io_handle,
        device: device,
    }
}
