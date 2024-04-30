//! usb

mod open;

// We re-export types used when opening a usb device
pub use self::open::{
    Baud, Dcb, DcbFlags, DeviceControlSettings, DtrControl, FlowControl, OpenFuture, Parity,
    RtsControl, Stop,
};

pub fn open<O: Into<std::ffi::OsString>>(port: O) -> std::io::Result<open::OpenFuture> {
    open::OpenFuture::new(port)
}
