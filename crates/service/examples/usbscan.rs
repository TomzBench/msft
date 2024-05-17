//! usb scan

use futures::{future, StreamExt};
use msft_service::device::{plug_events, prelude::*, NotificationRegistry};
use std::pin::pin;
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

    // Create a handle to listen for device events
    let scanner = NotificationRegistry::new()
        .with_serial_port()
        .spawn("MyDeviceNotifications")?;

    scanner.scan()?.scan()?.scan()?;

    // create a stream to listen for usb plug/unplug events
    let stream = scanner
        .listen()
        .filter_map(|ev| future::ready(plug_events(ev)))
        .track(vec![("2FE3", "0100")])?
        .take(3);

    let mut pinned = pin!(stream);
    while let Some(tracking) = pinned.next().await {
        // Log the event
        let port = tracking?.port;
        debug!(?port, "found usb event");
    }

    Ok(())
}
