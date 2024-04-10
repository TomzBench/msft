//! usb

use futures::StreamExt;
use msft_runtime::{codec::lines::LinesDecoder, io::ThreadpoolOptions, usb::DeviceControlSettings};
use msft_service::device::{NotificationRegistry, ScanError};
use std::{ffi::OsString, future::ready};
use tracing::info;
use tracing_subscriber::{
    filter::LevelFilter, fmt, fmt::time::ChronoLocal, layer::SubscriberExt, prelude::*,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup logging for Event Windows Tracing, A "daily" rolling log file, and stdout console
    let timer = ChronoLocal::new("%I:%M:%S".to_string());
    let rolling = tracing_appender::rolling::daily("C:\\Users\\Tom\\Documents", "console.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(rolling);
    let file = fmt::layer()
        .with_target(true)
        .with_writer(non_blocking)
        .with_ansi(false);
    let stdout = fmt::layer()
        .compact()
        .with_timer(timer)
        .with_ansi(true)
        .with_level(true)
        .with_file(false)
        .with_line_number(false)
        .with_target(true);
    tracing_subscriber::registry()
        .with(stdout)
        .with(file)
        .with(LevelFilter::TRACE)
        .init();

    // Welcome message
    info!("Application service starting...");

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
    // TODO add a timer to end loop (ie: take_until)
    let mut stream = reader.stream().await;
    let mut count = 0;
    while let Some(line) = stream.next().await {
        info!(?line, "received incoming");
        count += 1;
        writer
            .write(format!("sending {count}\r\n").as_str())
            .await?;
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
