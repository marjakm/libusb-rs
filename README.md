# Libusb
This crate provides a safe wrapper around the native [`libusb`][libusb] library. It applies the RAII pattern
and Rust lifetimes to ensure safe usage of all `libusb` functionality. The RAII pattern ensures that
all acquired resources are released when they're no longer needed, and Rust lifetimes ensure that
resources are released in a proper order.

* [Documentation](http://dcuddeback.github.io/libusb-rs/libusb/)

## Dependencies
In order to use the `libusb` crate, you must have the native `libusb` library installed where it can
be found by `pkg-config`.

All systems supported by the native `libusb` library are also supported by the `libusb` crate. It's
been tested on Linux, OS X, and Windows.

### Cross-Compiling
The `libusb` crate can be used when cross-compiling to a foreign target. Details on how to
cross-compile `libusb` are explained in the [`libusb-sys` crate's
README](https://github.com/dcuddeback/libusb-sys#cross-compiling).

## Usage
Add `libusb` as a dependency in `Cargo.toml`:

```toml
[dependencies]
libusb = "0.3"
```

Import the `libusb` crate. The starting point for nearly all `libusb` functionality is to create a
context object. With a context object, you can list devices, read their descriptors, open them, and
communicate with their endpoints:

```rust
extern crate libusb;

fn main() {
    let mut context = libusb::Context::new().unwrap();

    for mut device in context.devices().unwrap().iter() {
        let device_desc = device.device_descriptor().unwrap();

        println!("Bus {:03} Device {:03} ID {:04x}:{:04x}",
            device.bus_number(),
            device.address(),
            device_desc.vendor_id(),
            device_desc.product_id());
    }
}
```

## Contributors
* [dcuddeback](https://github.com/dcuddeback)
* [nibua-r](https://github.com/nibua-r)
* [kevinmehall](https://github.com/kevinmehall)


## Async API
Should currently work for unix-like systems that don't require [time-based event handling][poll]:

* Darwin
* Linux, provided that the following version requirements are satisfied:
    - Linux v2.6.27 or newer, compiled with timerfd support
    - glibc v2.9 or newer
    - libusb v1.0.5 or newer

### Requirements
* Must be usable with mio without extra threads for unix-like systems
* Must support multithreaded operation

### Notes
Libusb's [asynchronous api][async] works by polling file file descriptors, but if multiple threads poll
at the same time, then only one will be woken. Libusb describes how it overcomes this [here][mtasync],
but as far as I can see, still only one thread can actually poll at a time - others will block with another
mechanism, that can not be registered in mio. To avoid runtime problems, rust api should make polling
exclusively available for only the mio registrable object. That either means ensuring that the mio
registrable object allways wins the race to get the events lock (polling rights) or making it the
only thing that can take the lock. For simplicity the current solution takes the `only thing that
can take the lock` option. As libusb [synchronous api][sync] is internally built on the async api,
every sync api function will try to take the events lock, so they all need to be disabled for async
functions to be available. This is achieved by splitting the api in sync and async parts and making
only one available at a time - you choose which when creating a context.


[libusb]: http://libusb.info/
[sync]: http://libusb.sourceforge.net/api-1.0/group__syncio.html
[async]: http://libusb.sourceforge.net/api-1.0/group__asyncio.html
[poll]: http://libusb.sourceforge.net/api-1.0/group__poll.html
[capi]: http://libusb.sourceforge.net/api-1.0/api.html
[mtasync]: http://libusb.sourceforge.net/api-1.0/mtasync.html



## License
Copyright © 2015 David Cuddeback

Distributed under the [MIT License](LICENSE).
