//! usbmon
use futures::future::ready;
use futures::{SinkExt, StreamExt, TryStreamExt};
use msft_runtime::{
    timer::TimerPool,
    usb::{self, DeviceControlSettings},
};
use msft_service::device::{prelude::*, NotificationRegistry, PlugEvent};
use std::{io, pin::pin, time::Duration};
use tokio::fs::OpenOptions;
use tokio_util::codec::{Framed, LinesCodec};
use tracing::info;
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

    // Create a timer to signal end of our application
    let mut timers = TimerPool::new(&Default::default())?;
    let _stop = timers.oneshot(Duration::from_secs(2)).await;

    // Look for a single event associated with vendor/product of interest
    let fut = NotificationRegistry::with_capacity(3)
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
        .and_then(|port| ready(usb::configure(port, DeviceControlSettings::default())));
    let file = pin!(fut).next().await.unwrap()?;

    let mut io = Framed::new(file, LinesCodec::new());
    io.send("hello").await?;
    let response = io.next().await.unwrap()?;
    info!(response, "demo over");
    Ok(())
}
