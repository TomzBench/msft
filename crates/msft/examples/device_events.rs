//! device events

use msft::service::device::NotificationRegistry;
use tracing::info;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Welcome message
    info!("Application service starting...");

    // Listen for USB Plug/Unplug events
    let notifications = NotificationRegistry::with_capacity(3)
        .with(NotificationRegistry::WCEUSBS)
        .with(NotificationRegistry::USBDEVICE)
        .with(NotificationRegistry::PORTS)
        .start("MyDeviceNotifications")?;
    let mut s = futures::executor::block_on_stream(notifications);

    let mut count = 4;
    while let Some(ev) = s.next() {
        info!(ev = ?ev.ty, "device event");
        count = count - 1;
        if count == 0 {
            drop(s);
            break;
        }
    }
    Ok(())
}
