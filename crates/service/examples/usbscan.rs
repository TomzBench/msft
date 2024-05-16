//! usb scan

use futures::StreamExt;
use futures::TryStreamExt;
use msft_service::device::{plug_events, prelude::*, TrackingError};
use std::pin::pin;
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
    let mut stream = plug_events("MyDeviceNotifications")?
        .track(vec![("2FE3", "0001")])?
        .and_then(|tracked| async {
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(tracked.port)
                .await
                .map_err(TrackingError::from)
        })
        .take(4);

    let mut pinned = pin!(stream);
    while let Some(ev) = pinned.next().await {
        // Log the event
        debug!(ok = ev.is_ok(), "found usb event");
    }

    Ok(())
}
