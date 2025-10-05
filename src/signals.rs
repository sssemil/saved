use tokio::signal;
use tokio::signal::unix::{SignalKind, signal};

pub async fn handle_shutdown_signals() {
    let mut sigterm = signal(SignalKind::terminate()).expect("Failed to get SIGTERM handler.");
    let mut sigint = signal(SignalKind::interrupt()).expect("Failed to get SIGINT handler.");

    tokio::select! {
        _ = sigterm.recv() => {
            println!("Received SIGTERM");
        }
        _ = sigint.recv() => {
            println!("Received SIGINT");
        }
        _ = signal::ctrl_c() => {
            println!("Received Ctrl+C");
        }
    }
}
