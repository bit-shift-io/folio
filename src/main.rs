//! Folio: a fast, single-binary local web file explorer.
//!
//! Serves the filesystem over plain HTTP on `127.0.0.1`, starting at one
//! directory, with a tiny WebSocket channel that pushes change hints when the
//! filesystem moves. Browsing is unrestricted — `..` and the path bar reach
//! anywhere a shell can.

use folio::cli::{self, Outcome};
use folio::server;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let args = match cli::parse_from(std::env::args().skip(1)) {
        Ok(Outcome::Run(args)) => args,
        Ok(Outcome::Help) => {
            println!("{}", cli::USAGE);
            return;
        }
        Ok(Outcome::Version) => {
            println!("{}", cli::VERSION);
            return;
        }
        Err(e) => {
            tracing::error!("{e}\n\n{}", cli::USAGE);
            std::process::exit(2);
        }
    };

    let root = std::fs::canonicalize(&args.root).unwrap_or_else(|_| args.root.clone());

    if !root.is_dir() {
        tracing::error!("root is not a directory: {}", root.display());
        std::process::exit(1);
    }

    let state = server::AppState::new(root.clone());
    state.spawn_watcher();
    let app = server::build_router(state);

    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", args.port)).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("failed to bind 127.0.0.1:{}: {e}", args.port);
            std::process::exit(1);
        }
    };

    tracing::info!(
        "folio serving {} at http://127.0.0.1:{}/",
        root.display(),
        args.port
    );

    axum::serve(listener, app).await.expect("server error");
}
