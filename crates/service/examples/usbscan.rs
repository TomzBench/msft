//! usb scan

use futures::StreamExt;
use msft_service::device::{NotificationRegistry, UsbStreamExt};
use tracing::{debug, info};
use tracing_subscriber::{filter::LevelFilter, fmt, layer::SubscriberExt, prelude::*};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup logging
    let stdout = fmt::layer()
        .compact()
        .with_ansi(true)
        .with_level(true)
        .with_file(false)
        .with_line_number(false)
        .with_target(true);
    tracing_subscriber::registry()
        .with(stdout)
        .with(LevelFilter::TRACE)
        .init();

    // Welcome message
    info!("Application service starting...");

    // Look for a single event associated with vendor/product of interest
    let ev = NotificationRegistry::with_capacity(3)
        .with(NotificationRegistry::WCEUSBS)
        .with(NotificationRegistry::USBDEVICE)
        .with(NotificationRegistry::PORTS)
        .start("MyDeviceNotifications")?
        .filter_for_ids(vec![("2FE3", "0001")])?
        .take(1)
        .next()
        .await
        .unwrap()?;

    // Log the event
    debug!(?ev, "found usb event");

    Ok(())
}
