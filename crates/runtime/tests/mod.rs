//! test

mod work;

#[test]
fn test_device_decoder() {
    assert!(true, "dummy");
}

#[test]
fn test_device_usb_dcb() {
    use msft_runtime::usb::{DcbFlags, DtrControl};
    let mut flags = DcbFlags::new(0);
    assert_eq!(0, flags.value());

    // fBinary
    flags.set_fBinary(true);
    assert_eq!(1, flags.value());
    assert!(flags.get_fBinary());
    flags.set_fBinary(false);
    assert_eq!(0, flags.value());
    assert!(!flags.get_fBinary());

    // fParity
    flags.set_fParity(true);
    assert_eq!(2, flags.value());
    assert!(flags.get_fParity());
    flags.set_fParity(false);
    assert_eq!(0, flags.value());
    assert!(!flags.get_fParity());

    // fDtrControl
    flags.set_fDtrControl(DtrControl::Enable);
    assert_eq!(0x10, flags.value());
    flags.set_fDtrControl(DtrControl::Disable);
    assert_eq!(0x00, flags.value());
    flags.set_fDtrControl(DtrControl::Handshake);
    assert_eq!(0x20, flags.value());
    flags.set_fDtrControl(DtrControl::Disable);

    // Read value with some noise bits
    flags.set_fParity(true);
    flags.set_fBinary(true);
    flags.set_fDtrControl(DtrControl::Enable);
    assert_eq!(DtrControl::Enable, flags.get_fDtrControl());
}
