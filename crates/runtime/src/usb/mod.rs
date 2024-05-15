//! Asyncronously open a USB device
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use std::fmt;
use windows_sys::Win32::{Devices::Communication::*, System::WindowsProgramming::*};

#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, FromPrimitive)]
pub enum DtrControl {
    Disable = DTR_CONTROL_DISABLE,
    Enable = DTR_CONTROL_ENABLE,
    Handshake = DTR_CONTROL_HANDSHAKE,
}
impl DtrControl {
    pub fn raw(&self) -> u32 {
        // safety: https://doc.rust-lang.org/reference/items/enumerations.html#pointer-casting
        // If the enumeration specifies a primitive representation, then the discriminant may
        // be reliably accessed via unsafe pointer casting:
        unsafe { *(self as *const Self as *const u32) }
    }
}

#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, FromPrimitive)]
pub enum RtsControl {
    Disable = RTS_CONTROL_DISABLE,
    Enable = RTS_CONTROL_ENABLE,
    Handshake = RTS_CONTROL_HANDSHAKE,
    Toggle = RTS_CONTROL_TOGGLE,
}
impl RtsControl {
    pub fn raw(&self) -> u32 {
        // safety: https://doc.rust-lang.org/reference/items/enumerations.html#pointer-casting
        // If the enumeration specifies a primitive representation, then the discriminant may
        // be reliably accessed via unsafe pointer casting:
        unsafe { *(self as *const Self as *const u32) }
    }
}

macro_rules! impl_set_bits {
    ($name:ident, $offset:path) => {
        #[allow(non_snake_case)]
        pub fn $name(&mut self, val: bool) -> &mut Self {
            match val {
                true => self.0 |= 0x01 << $offset,
                false => self.0 &= !(0x01 << $offset),
            };
            self
        }
    };

    ($name:ident, $value:ty, $offset:path) => {
        #[allow(non_snake_case)]
        pub fn $name(&mut self, val: $value) -> &mut Self {
            self.0 &= !(0x03 << $offset);
            self.0 |= ((val as u32) << $offset) as u32;
            self
        }
    };
}

macro_rules! impl_get_bits {
    ($name:ident, $offset:path) => {
        #[allow(non_snake_case)]
        pub fn $name(&self) -> bool {
            self.0 & (0x01 << $offset) > 0
        }
    };

    ($name:ident, $value:ident, $offset:path) => {
        #[allow(non_snake_case)]
        pub fn $name(&self) -> $value {
            let mut raw = (self.0 & (0x03 << $offset));
            raw >>= $offset;
            // Safety: We control these bits exclusivly so we know they are initialized
            unsafe { $value::from_u32(raw).unwrap_unchecked() }
        }
    };
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub struct DcbFlags(u32);
impl DcbFlags {
    pub const FBINARY: u8 = 0x00;
    pub const FPARITY: u8 = 0x01;
    pub const FOUTXCTSFLOW: u8 = 0x02;
    pub const FOUTXDSRFLOW: u8 = 0x03;
    pub const FDTRCONTROL: u8 = 0x04;
    pub const FDSRSENSITIVITY: u8 = 0x06;
    pub const FTXCONTINUEONXOFF: u8 = 0x07;
    pub const FOUTX: u8 = 0x08;
    pub const FINX: u8 = 0x09;
    pub const FERRORCHAR: u8 = 0x0A;
    pub const FNULL: u8 = 0x0B;
    pub const FRTSCONTROL: u8 = 0x0C;
    pub const FABORTONERROR: u8 = 0x0E;

    pub fn new(val: u32) -> Self {
        Self(val)
    }

    pub fn value(&self) -> u32 {
        self.0
    }

    impl_set_bits!(set_fBinary, Self::FBINARY);
    impl_set_bits!(set_fParity, Self::FPARITY);
    impl_set_bits!(set_fOutxCtsFlow, Self::FOUTXCTSFLOW);
    impl_set_bits!(set_fOutxDsrFlow, Self::FOUTXDSRFLOW);
    impl_set_bits!(set_fDsrSensitivity, Self::FDSRSENSITIVITY);
    impl_set_bits!(set_fTXContinueOnXoff, Self::FTXCONTINUEONXOFF);
    impl_set_bits!(set_fOutX, Self::FOUTX);
    impl_set_bits!(set_fInX, Self::FINX);
    impl_set_bits!(set_fErrorChar, Self::FERRORCHAR);
    impl_set_bits!(set_fNull, Self::FNULL);
    impl_set_bits!(set_fAbortOnError, Self::FABORTONERROR);
    impl_set_bits!(set_fDtrControl, DtrControl, Self::FDTRCONTROL);
    impl_set_bits!(set_fRtsControl, RtsControl, Self::FRTSCONTROL);

    impl_get_bits!(get_fBinary, Self::FBINARY);
    impl_get_bits!(get_fParity, Self::FPARITY);
    impl_get_bits!(get_fOutxCtsFlow, Self::FOUTXCTSFLOW);
    impl_get_bits!(get_fOutxDsrFlow, Self::FOUTXDSRFLOW);
    impl_get_bits!(get_fDsrSensitivity, Self::FDSRSENSITIVITY);
    impl_get_bits!(get_fTXContinueOnXoff, Self::FTXCONTINUEONXOFF);
    impl_get_bits!(get_fOutX, Self::FOUTX);
    impl_get_bits!(get_fInX, Self::FINX);
    impl_get_bits!(get_fErrorChar, Self::FERRORCHAR);
    impl_get_bits!(get_fNull, Self::FNULL);
    impl_get_bits!(get_fAbortOnError, Self::FABORTONERROR);
    impl_get_bits!(get_fDtrControl, DtrControl, Self::FDTRCONTROL);
    impl_get_bits!(get_fRtsControl, RtsControl, Self::FRTSCONTROL);
}

impl fmt::Debug for DcbFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DcbFlags")
            .field("fBinary", &self.get_fBinary())
            .field("fParity", &self.get_fParity())
            .field("fOutxCtsFlow", &self.get_fOutxCtsFlow())
            .field("fOutxDsrFlow", &self.get_fOutxDsrFlow())
            .field("fDsrSensitify", &self.get_fDsrSensitivity())
            .field("fOutX", &self.get_fOutX())
            .field("fErrorChar", &self.get_fErrorChar())
            .field("fNull", &self.get_fNull())
            .field("fAbortOnError", &self.get_fAbortOnError())
            .field("fDtrControl", &self.get_fDtrControl())
            .field("fRtsControl", &self.get_fRtsControl())
            .finish()
    }
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, FromPrimitive)]
pub enum Parity {
    None = NOPARITY,
    Even = EVENPARITY,
    Odd = ODDPARITY,
    Mark = MARKPARITY,
    Space = SPACEPARITY,
}

#[repr(u32)]
#[allow(non_camel_case_types)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, FromPrimitive)]
pub enum Baud {
    CBR_110 = CBR_110,
    CBR_300 = CBR_300,
    CBR_600 = CBR_600,
    CBR_1200 = CBR_1200,
    CBR_2400 = CBR_2400,
    CBR_4800 = CBR_4800,
    CBR_9600 = CBR_9600,
    CBR_14400 = CBR_14400,
    CBR_19200 = CBR_19200,
    CBR_38400 = CBR_38400,
    CBR_57600 = CBR_57600,
    CBR_115200 = CBR_115200,
    CBR_128000 = CBR_128000,
    CBR_256000 = CBR_256000,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, FromPrimitive)]
#[repr(u8)]
pub enum Stop {
    One = ONESTOPBIT,
    One5 = ONE5STOPBITS,
    Two = TWOSTOPBITS,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, FromPrimitive)]
pub enum FlowControl {
    None,
    Software,
    Hardware,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct DeviceControlSettings {
    pub baud: Baud,
    pub bytes: u8,
    pub parity: Parity,
    pub stop: Stop,
    pub flow_control: FlowControl,
}

impl Default for DeviceControlSettings {
    fn default() -> Self {
        Self {
            baud: Baud::CBR_115200,
            bytes: 8,
            parity: Parity::None,
            stop: Stop::One,
            flow_control: FlowControl::None,
        }
    }
}

/// https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-dcb
pub struct Dcb(DCB);
impl fmt::Debug for Dcb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let baud = Baud::from_u32(self.0.BaudRate).ok_or_else(std::fmt::Error::default)?;
        let parity = Parity::from_u8(self.0.Parity).ok_or_else(std::fmt::Error::default)?;
        let stop = Stop::from_u8(self.0.StopBits).ok_or_else(std::fmt::Error::default)?;
        let flags = DcbFlags::new(self.0._bitfield);
        f.debug_struct("Dcb")
            .field("BaudRate", &baud)
            .field("Parity", &parity)
            .field("StopBits", &stop)
            .field("fBinary", &flags.get_fBinary())
            .field("fParity", &flags.get_fParity())
            .field("fOutxCtsFlow", &flags.get_fOutxCtsFlow())
            .field("fOutxDsrFlow", &flags.get_fOutxDsrFlow())
            .field("fDsrSensitify", &flags.get_fDsrSensitivity())
            .field("fOutX", &flags.get_fOutX())
            .field("fErrorChar", &flags.get_fErrorChar())
            .field("fNull", &flags.get_fNull())
            .field("fAbortOnError", &flags.get_fAbortOnError())
            .field("fDtrControl", &flags.get_fDtrControl())
            .field("fRtsControl", &flags.get_fRtsControl())
            .field("ByteSize", &self.0.ByteSize)
            .field("XonLim", &self.0.XonLim)
            .field("XoffLim", &self.0.XoffLim)
            .field("XOnChar", &self.0.XonChar)
            .field("XoffChar", &self.0.XoffChar)
            .field("ErrorChar", &self.0.ErrorChar)
            .field("EofChar", &self.0.EofChar)
            .field("EvtChar", &self.0.EvtChar)
            .finish()
    }
}
