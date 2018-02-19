use std::mem;
use std::marker::PhantomData;
use libusb::*;



pub trait MarkerType<'a> {
    type Marker;
    fn construct<T>(t: T) -> Self::Marker;
}


