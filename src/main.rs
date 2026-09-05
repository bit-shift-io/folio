//! Folio: a fast, single-binary local web file explorer.
//!
//! Serves the filesystem over plain HTTP on `127.0.0.1`, starting at one
//! directory, with a tiny WebSocket channel that pushes change hints when the
//! filesystem moves. Browsing is unrestricted — `..` and the path bar reach
//! anywhere a shell can.

use std::path::PathBuf;

use clap::Parser;

use folio::server;

#[derive(Parser)]
#[command(name = "folio", about = "Local web file explorer")]
struct Cli {
    /// Initial directory to browse (default: current directory)
    #[arg(long, default_value = ".")]
    root: PathBuf,

    /// Port to bind on 127.0.0.1
    #[arg(long, default_value_t = 4000)]
    port: u16,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let cli = Cli::parse();

    let root = std::fs::canonicalize(&cli.root)
        .unwrap_or_else(|_| cli.root.clone());

    if !root.is_dir() {
        tracing::error!("root is not a directory: {}", root.display());
        std::process::exit(1);
    }

    let state = server::AppState::new(root.clone());
    state.spawn_watcher();
    let app = server::build_router(state);

    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", cli.port)).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("failed to bind 127.0.0.1:{}: {e}", cli.port);
            std::process::exit(1);
        }
    };

    tracing::info!(
        "folio serving {} at http://127.0.0.1:{}/",
        root.display(),
        cli.port
    );

    axum::serve(listener, app).await.expect("server error");
}