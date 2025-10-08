use tokio::signal;
#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal};

#[derive(Debug, Clone, Copy)]
pub enum ShutdownReason {
    #[cfg(unix)]
    SigTerm,
    #[cfg(unix)]
    SigInt,
    CtrlC,
}

/// Usage:
/// ```
/// let shutdown = shutdown_signal();
/// tokio::pin!(shutdown);
///
/// loop {
///     select! {
///         biased;
///         reason = &mut shutdown =>  {
///             println!("Received shutdown signal: {:?}", reason);
///             break;
///         }
///     }
/// }
/// ```
pub async fn shutdown_signal() -> ShutdownReason {
    #[cfg(unix)]
    {
        let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler");
        let mut sigint = signal(SignalKind::interrupt()).expect("SIGINT handler");

        tokio::select! {
            _ = sigterm.recv() => ShutdownReason::SigTerm,
            _ = sigint.recv()  => ShutdownReason::SigInt,
            _ = signal::ctrl_c() => ShutdownReason::CtrlC,
        }
    }

    #[cfg(not(unix))]
    {
        signal::ctrl_c().await.expect("ctrl_c");
        ShutdownReason::CtrlC
    }
}
