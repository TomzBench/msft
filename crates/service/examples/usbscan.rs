//! usb scan

use futures::StreamExt;
use msft_runtime::{codec::lines::LinesDecoder, io::ThreadpoolOptions, usb::DeviceControlSettings};
use msft_service::device::{prelude::*, NotificationRegistry};
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
    let mut stream = NotificationRegistry::with_capacity(3)
        .with(NotificationRegistry::WCEUSBS)
        .with(NotificationRegistry::USBDEVICE)
        .with(NotificationRegistry::PORTS)
        .start("MyDeviceNotifications")?
        .filter_for_ids(vec![("2FE3", "0001")])?
        .try_open(|port, _, _, _| {
            port.configure(DeviceControlSettings::default())?
                .run(ThreadpoolOptions {
                    environment: None,
                    decoder: LinesDecoder::default(),
                    capacity: 4095,
                    queue: 8,
                })
                .map_err(|e| e.into())
        })
        .take(4);

    while let Some(ev) = stream.next().await {
        // Log the event
        debug!(ok = ev.is_ok(), "found usb event");
    }

    Ok(())
}
