//! console
use msft_service::device::NotificationRegistry;
use tracing::info;
use tracing_subscriber::{
    filter::LevelFilter, fmt, fmt::time::ChronoLocal, layer::SubscriberExt, prelude::*,
};
use win_etw_tracing::TracelogSubscriber;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup logging for Event Windows Tracing, A "daily" rolling log file, and stdout console
    let timer = ChronoLocal::new("%I:%M:%S".to_string());
    let guid = msft_service::util::guid::new("a9214533-3f5f-475b-8140-cb96b289270b");
    let etw = TracelogSubscriber::new(guid, "Altronix Service Tracelog").unwrap();
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
        .with_target(true);
    tracing_subscriber::registry()
        .with(stdout)
        .with(file)
        .with(etw)
        .with(LevelFilter::TRACE)
        .init();

    // Welcome message
    info!("Application service starting...");

    // Listen for USB Plug/Unplug events
    let notifications = NotificationRegistry::with_capacity(3)
        .with(NotificationRegistry::WCEUSBS)
        .with(NotificationRegistry::USBDEVICE)
        .with(NotificationRegistry::PORTS)
        .start("MyDeviceNotifications")?;
    let mut s = futures::executor::block_on_stream(notifications);

    let mut count = 4;
    while let Some(ev) = s.next() {
        info!(ev = ?ev.ty, "device event");
        count = count - 1;
        if count == 0 {
            drop(s);
            break;
        }
    }
    Ok(())
}
