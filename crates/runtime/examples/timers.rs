//! timers

use futures::StreamExt;
use msft_runtime::timer::{TimerPool, TimerThreadpoolOptions};
use std::{io, time::Duration};
use tracing::info;
use tracing_subscriber::{filter::LevelFilter, fmt, layer::SubscriberExt, prelude::*};

#[tokio::main]
async fn main() -> io::Result<()> {
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

    // Print welcome message
    info!("Starting timer demo");

    // Configure threadpool for timers see [`TimerThreadpoolOptions`]
    let opts = TimerThreadpoolOptions {
        env: None,
        capacity: 8,
        window: Some(Duration::from_millis(100)),
    };

    // Create 2 timer pool workers
    let mut poola = TimerPool::new(&opts)?;
    let mut poolb = TimerPool::new(&opts)?;

    // Create a periodic timer
    let timeouts = poola
        .periodic(Duration::from_millis(2500), Duration::from_millis(500))
        .await;

    // Create a oneshot timer
    let stop = poolb.oneshot(Duration::from_millis(5000)).await;

    // Create a stream of timeouts that completes when stop timer timeouts
    let mut stream = timeouts.start().take_until(stop.start());

    // Log timer events until stop timer occurs
    info!("Please wait 2.5s for 5 500ms timeouts");
    let mut timeouts = 0;
    while let Some(_) = stream.next().await {
        timeouts += 1;
        info!(timeouts, "timeout");
    }

    // Print exit message
    info!("all done");
    Ok(())
}
