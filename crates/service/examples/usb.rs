//! usbmon
use futures::StreamExt;
use msft_service::{
    device::{plug_events, prelude::*, TrackingError},
    util::wait,
};
use std::pin::pin;
use tokio::task::JoinHandle;
use tracing::{error, info};
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

    // Create an abort signal
    let (abort_set, abort) = wait::oneshot()?;

    // Signal to receive a port
    let (tx, rx) = tokio::sync::oneshot::channel();

    // Create a stream to listen for USB plug/unplug events
    let stream = plug_events("MyDeviceNotifications")?
        .take_until(abort)
        .track(vec![("2FE3", "0100")])?;

    // Spawn a task to listen for USB plug/unplug events
    let jh: JoinHandle<Result<(), TrackingError>> = tokio::spawn(async move {
        // Send the first connected device to our main task
        let mut pinned = pin!(stream);
        if let Some(tracked) = pinned.next().await {
            if let Err(error) = tx.send(tracked?) {
                error!(port = ?error.port, "failed to send port");
            }
        }

        // Keep listening to stream to track the unplug event
        while let Some(tracked) = pinned.next().await {
            let port = tracked?.port;
            info!(?port, "ignoring channel");
        }
        Ok(())
    });

    // get a new device and wait for its unplug
    let tracked = rx.await?;
    info!(?tracked.port, "waiting for unplug event");
    tracked.unplugged.await?;
    abort_set.set()?;
    jh.await??;
    Ok(())
}
