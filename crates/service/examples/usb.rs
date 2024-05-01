//! usbmon
use futures::StreamExt;
use msft_runtime::{
    codec::lines::LinesDecoder, io::ThreadpoolOptions, timer::TimerPool, usb::DeviceControlSettings,
};
use msft_service::device::{prelude::*, NotificationRegistry};
use std::time::Duration;
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
    let stop = timers.oneshot(Duration::from_secs(2)).await;

    // Look for a single event associated with vendor/product of interest
    let port = NotificationRegistry::with_capacity(3)
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
        .next()
        .await
        .unwrap()?;

    // Get the reader writer handles to begin I/O and write some Hello's
    let (mut reader, mut writer) = port.reader_writer();
    writer.write("hello\r\n").await?;
    writer.write("world\r\n").await?;

    // Setup a stream and read/write in a loop
    let mut stream = reader.stream().await.take_until(stop.start());
    let mut count = 0;
    while let Some(line) = stream.next().await {
        info!(line = line?, "received incoming");
        count += 1;
        writer.write(format!("Hello {count}\r\n").as_str()).await?;
    }

    info!("demo finished");
    Ok(())
}
