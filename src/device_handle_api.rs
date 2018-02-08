pub use self::sync_api::DeviceHandleSyncApi;


mod sync_api {
    use std::slice;
    use std::time::Duration;
    use language::Language;
    use device_descriptor::DeviceDescriptor;
    use config_descriptor::ConfigDescriptor;
    use interface_descriptor::InterfaceDescriptor;
    use fields::{Direction, RequestType, Recipient, request_type};
    use error::Error;
    use libusb::*;

    pub trait DeviceHandleSyncApi {
        fn read_interrupt(&self, endpoint: u8, buf: &mut [u8], timeout: Duration) -> ::Result<usize>;
        fn write_interrupt(&self, endpoint: u8, buf: &[u8], timeout: Duration) -> ::Result<usize>;
        fn read_bulk(&self, endpoint: u8, buf: &mut [u8], timeout: Duration) -> ::Result<usize>;
        fn write_bulk(&self, endpoint: u8, buf: &[u8], timeout: Duration) -> ::Result<usize>;
        fn read_control(&self, request_type: u8, request: u8, value: u16, index: u16, buf: &mut [u8], timeout: Duration) -> ::Result<usize>;
        fn write_control(&self, request_type: u8, request: u8, value: u16, index: u16, buf: &[u8], timeout: Duration) -> ::Result<usize>;

        /// Reads the languages supported by the device's string descriptors.
        ///
        /// This function returns a list of languages that can be used to read the device's string
        /// descriptors.
        fn read_languages(&self, timeout: Duration) -> ::Result<Vec<Language>> {
            let mut buf = Vec::<u8>::with_capacity(256);

            let mut buf_slice = unsafe {
                slice::from_raw_parts_mut((&mut buf[..]).as_mut_ptr(), buf.capacity())
            };

            let len = try!(self.read_control(request_type(Direction::In, RequestType::Standard, Recipient::Device),
                                             LIBUSB_REQUEST_GET_DESCRIPTOR,
                                             (LIBUSB_DT_STRING as u16) << 8,
                                             0,
                                             buf_slice,
                                             timeout));

            unsafe {
                buf.set_len(len);
            }

            Ok(buf.chunks(2).skip(1).map(|chunk| {
                let lang_id = chunk[0] as u16 | (chunk[1] as u16) << 8;
                ::language::from_lang_id(lang_id)
            }).collect())
        }

        /// Reads a string descriptor from the device.
        ///
        /// `language` should be one of the languages returned from [`read_languages`](#method.read_languages).
        fn read_string_descriptor(&self, language: Language, index: u8, timeout: Duration) -> ::Result<String> {
            let mut buf = Vec::<u8>::with_capacity(256);

            let mut buf_slice = unsafe {
                slice::from_raw_parts_mut((&mut buf[..]).as_mut_ptr(), buf.capacity())
            };

            let len = try!(self.read_control(request_type(Direction::In, RequestType::Standard, Recipient::Device),
                                             LIBUSB_REQUEST_GET_DESCRIPTOR,
                                             (LIBUSB_DT_STRING as u16) << 8 | index as u16,
                                             language.lang_id(),
                                             buf_slice,
                                             timeout));

            unsafe {
                buf.set_len(len);
            }

            let utf16: Vec<u16> = buf.chunks(2).skip(1).map(|chunk| {
                chunk[0] as u16 | (chunk[1] as u16) << 8
            }).collect();

            String::from_utf16(&utf16[..]).map_err(|_| Error::Other)
        }

        /// Reads the device's manufacturer string descriptor.
        fn read_manufacturer_string(&self, language: Language, device: &DeviceDescriptor, timeout: Duration) -> ::Result<String> {
            match device.manufacturer_string_index() {
                None => Err(Error::InvalidParam),
                Some(n) => self.read_string_descriptor(language, n, timeout)
            }
        }

        /// Reads the device's product string descriptor.
        fn read_product_string(&self, language: Language, device: &DeviceDescriptor, timeout: Duration) -> ::Result<String> {
            match device.product_string_index() {
                None => Err(Error::InvalidParam),
                Some(n) => self.read_string_descriptor(language, n, timeout)
            }
        }

        /// Reads the device's serial number string descriptor.
         fn read_serial_number_string(&self, language: Language, device: &DeviceDescriptor, timeout: Duration) -> ::Result<String> {
            match device.serial_number_string_index() {
                None => Err(Error::InvalidParam),
                Some(n) => self.read_string_descriptor(language, n, timeout)
            }
        }

        /// Reads the string descriptor for a configuration's description.
        fn read_configuration_string(&self, language: Language, configuration: &ConfigDescriptor, timeout: Duration) -> ::Result<String> {
            match configuration.description_string_index() {
                None => Err(Error::InvalidParam),
                Some(n) => self.read_string_descriptor(language, n, timeout)
            }
        }

        /// Reads the string descriptor for a interface's description.
        fn read_interface_string(&self, language: Language, interface: &InterfaceDescriptor, timeout: Duration) -> ::Result<String> {
            match interface.description_string_index() {
                None => Err(Error::InvalidParam),
                Some(n) => self.read_string_descriptor(language, n, timeout)
            }
        }
    }
}
