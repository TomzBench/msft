//! usb

mod open;

// We re-export types used when opening a usb device
pub use self::open::{
    Baud, ConfiguredUsb, Dcb, DcbFlags, DeviceControlSettings, DtrControl, FlowControl, OpenFuture,
    Parity, RtsControl, Stop, UsbHandle,
};

pub fn open<O: Into<std::ffi::OsString>>(port: O) -> std::io::Result<open::OpenFuture> {
    open::OpenFuture::new(port)
}
