//! usb scan

use futures::StreamExt;
use futures::{future::ready, TryStreamExt};
use msft_service::device::{prelude::*, NotificationRegistry, PlugEvent};
use std::{io, pin::pin};
use tokio::fs::OpenOptions;
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
        .filter_map(|ev| match ev {
            Ok(PlugEvent::Plug { port, ids }) => ready(Some(Ok((port, ids)))),
            Ok(PlugEvent::Unplug { .. }) => ready(None),
            Err(e) => ready(Some(Err(io::Error::from(e)))),
        })
        .and_then(|(port, _)| async {
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(port)
                .await
        })
        .take(4);

    let mut pinned = pin!(stream);
    while let Some(ev) = pinned.next().await {
        // Log the event
        debug!(ok = ev.is_ok(), "found usb event");
    }

    Ok(())
}
