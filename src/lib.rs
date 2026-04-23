use std::fs;
use std::future::{pending, Future};
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;

pub mod assets;
pub mod cli;
pub mod config;
pub mod error;
pub mod events;
pub mod exit;
pub mod logging;
pub mod render;
pub mod server;
pub mod sse;
pub mod state;
pub mod template;

pub use config::{Config, ConfigOverrides};
pub use error::{DiscussError, Result};
pub use events::{Event, EventEmitter, EventKind};
pub use exit::exit_code_for_error;
pub use logging::init_tracing;
pub use render::render;
pub use server::{serve, AppState};
pub use sse::{BroadcastEvent, EventBus};
pub use template::render_page;

pub const DEFAULT_PORT: u16 = 7777;

pub async fn run(args: cli::Args) -> Result<()> {
    run_with_shutdown(args, pending()).await
}

pub async fn run_with_shutdown<F>(args: cli::Args, shutdown: F) -> Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let cli::Args {
        port,
        file,
        command,
    } = args;
    let config = Config::resolve(ConfigOverrides {
        port,
        ..ConfigOverrides::default()
    })?;
    init_tracing(&config)?;
    tracing::debug!("tracing initialized");

    match command {
        Some(cli::Commands::Update) => Ok(()),
        None => {
            let Some(file) = file else {
                return Ok(());
            };
            let markdown_source = read_markdown_file(&file)?;
            let port = config.port.unwrap_or(DEFAULT_PORT);
            let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));

            serve(
                addr,
                AppState::for_process().with_markdown_source(markdown_source),
                shutdown,
            )
            .await
        }
    }
}

fn read_markdown_file(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|source| match source.kind() {
        io::ErrorKind::NotFound => DiscussError::FileNotFound {
            path: path.to_path_buf(),
        },
        _ => DiscussError::FileNotReadable {
            path: path.to_path_buf(),
            source,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    #[test]
    fn missing_markdown_file_maps_to_file_not_found() {
        let temp_dir = tempdir().expect("tempdir should be created");
        let missing_path = temp_dir.path().join("missing.md");
        let error = read_markdown_file(&missing_path).expect_err("missing file should fail");

        assert!(matches!(error, DiscussError::FileNotFound { .. }));
    }
}
