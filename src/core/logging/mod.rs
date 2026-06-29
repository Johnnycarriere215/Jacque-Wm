//! Logging initialisation.
//!
//! Uses `tracing` + `tracing-appender` with a daily-rotating file in
//! `%APPDATA%\JacqueWM\logs\jacquewm.log`. The console layer is only
//! attached when `JACQUEWM_LOG=stdout` is set.

use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::WorkerGuard;

pub use tracing_subscriber::{EnvFilter, Layer};
pub use tracing_subscriber::layer::SubscriberExt;
pub use tracing_subscriber::util::SubscriberInitExt;

use crate::error::{JacqueError, Result};

/// Resolves `%APPDATA%\JacqueWM\logs` and creates it if missing.
pub fn logs_dir() -> Result<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(dirs::config_dir)
        .ok_or_else(|| JacqueError::Logging("could not resolve APPDATA".into()))?;
    let dir = base.join("JacqueWM").join("logs");
    std::fs::create_dir_all(&dir)
        .map_err(|e| JacqueError::Logging(format!("could not create logs directory: {}", e)))?;
    Ok(dir)
}

/// Logging initialisation.
///
/// Returns a [`LoggingGuard`] that *must be held* until the program
/// exits. Dropping the guard flushes the non-blocking writers.
pub struct LoggingGuard {
    _console: Option<WorkerGuard>,
    _file: WorkerGuard,
}

/// Initialise the global tracing subscriber.
///
/// `filter` — `tracing` filter string. Empty falls back to
/// `RUST_LOG` or `Config::default_log_filter()`.
/// `to_stdout` — whether to also write logs to stdout.
/// `enable_file` — whether to also write to a daily-rotating file
/// in `%APPDATA%\JacqueWM\logs\`.
pub fn init(filter: &str, to_stdout: bool, enable_file: bool) -> Result<LoggingGuard> {
    let resolved = if filter.is_empty() {
        std::env::var("RUST_LOG").unwrap_or_else(|_| {
            crate::core::config::Config::default_log_filter().to_string()
        })
    } else {
        filter.to_string()
    };
    let env_filter = EnvFilter::try_new(&resolved)
        .map_err(|e| JacqueError::Logging(format!("invalid log filter '{}': {}", filter, e)))?;

    let dir = if enable_file {
        let dir = logs_dir()?;
        ensure_writable(&dir)?;
        Some(dir)
    } else {
        None
    };

    let (file_writer, file_guard) = match &dir {
        Some(path) => {
            let appender = tracing_appender::rolling::daily(path, "jacquewm.log");
            tracing_appender::non_blocking(appender)
        }
        None => tracing_appender::non_blocking(std::io::sink()),
    };

    let console = if to_stdout {
        let (writer, guard) = tracing_appender::non_blocking(std::io::stdout());
        attach_subscriber(env_filter.clone(), Some(writer), file_writer);
        Some(guard)
    } else {
        attach_subscriber(env_filter, None, file_writer);
        None
    };

    tracing::info!(
        target: "jacquewm.logging",
        version = env!("CARGO_PKG_VERSION"),
        "logging initialised"
    );

    Ok(LoggingGuard {
        _console: console,
        _file: file_guard,
    })
}

/// Internal helper: install the global subscriber with the given
/// filter + optional console writer + always-on file writer.
fn attach_subscriber<W>(
    filter: EnvFilter,
    console_writer: Option<W>,
    file_writer: tracing_appender::non_blocking::Writer,
) where
    W: for<'writer> tracing_subscriber::fmt::MakeWriter<'writer> + Send + Sync + 'static,
{
    use tracing_subscriber::fmt::MakeWriter;

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false)
        .with_target(true);

    let sub: Box<dyn tracing::Subscriber + Send + Sync> = match console_writer {
        Some(cw) => {
            let console_layer = tracing_subscriber::fmt::layer()
                .with_writer(cw)
                .with_target(true);
            Box::new(
                tracing_subscriber::registry()
                    .with(filter)
                    .with(console_layer)
                    .with(file_layer),
            )
        }
        None => Box::new(
            tracing_subscriber::registry()
                .with(filter)
                .with(file_layer),
        ),
    };
    let _ = tracing::subscriber::set_global_default(sub);
}

fn ensure_writable(path: &Path) -> Result<()> {
    if path.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(path)?;
    Ok(())
}
