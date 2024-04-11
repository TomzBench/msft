//! usbmon
use futures::StreamExt;
use msft_runtime::{
    codec::lines::LinesDecoder, io::ThreadpoolOptions, timer::TimerPool, usb::DeviceControlSettings,
};
use msft_service::device::{NotificationRegistry, ScanError};
use std::{ffi::OsString, future::ready, time::Duration};
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

    // Look for the Zephyr USB device
    let port = find_usb_device("2FE3", "0001").await?;
    info!(?port, "found zephyr USB device");

    // Open the USB port and put I/O resource on windows default threadpool runtime
    let io = msft_runtime::usb::open(port)?
        .await?
        .configure(DeviceControlSettings::default())?
        .run(ThreadpoolOptions {
            environment: None,
            decoder: LinesDecoder::default(),
            capacity: 4095,
            queue: 8,
        })?;

    // Get the reader writer handles to begin I/O and write some Hello's
    let (mut reader, mut writer) = io.reader_writer();
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

    Ok(())
}

/// This task will listen for USB Plug/Unplug events
async fn find_usb_device(vid: &str, pid: &str) -> Result<OsString, ScanError> {
    // Setup listener to for COM port arrivals
    let mut notifications = NotificationRegistry::with_capacity(3)
        .with(NotificationRegistry::WCEUSBS)
        .with(NotificationRegistry::USBDEVICE)
        .with(NotificationRegistry::PORTS)
        .start("MyDeviceNotifications")?
        .filter_map(|ev| ready(ev.filter_port_arrival().ok()));

    // Scan COM port VID and PID number and return if matches callers VID/PID values
    while let Some(port) = notifications.next().await {
        match msft_service::device::scan_for(&port)?.matches(vid, pid) {
            true => return Ok(port),
            _ => {}
        }
    }

    // The stream never ends, therefore we shall never reach here.
    unreachable!()
}
