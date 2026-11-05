//! CLI to start the web server and begin working with the daemon.

use std::{net::SocketAddr, path::PathBuf, process::ExitCode};

use lockbox_proto::local;
use lockbox_web::{AppState, DaemonClient, router};

const USAGE: &str = "\
Usage: lockbox-web [--listen <addr>] [--socket <path>]

Options:
  --listen <addr>  Address to serve the page on (default: 127.0.0.1:8080)
  --socket <path>  Location of daemon's UNIX socket
                   (default: $LOCKBOX_SOCKET, else <runtime dir>/lockbox/daemon.sock)
  -h, --help       Print this help
  -V, --version    Print the version";

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("lockbox=info,warn")),
        )
        .init();

    let options = match Command::parse() {
        Ok(Command::Run(options)) => options,
        Ok(Command::Help) => {
            println!("{}", USAGE);
            return ExitCode::SUCCESS;
        }
        Ok(Command::Version) => {
            println!("lockbox-web v{}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("lockbox-web: {message}");
            eprintln!();
            eprintln!("{}", USAGE);
            return ExitCode::from(2);
        }
    };

    match run(options).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!("{err}");
            ExitCode::FAILURE
        }
    }
}

async fn run(options: Options) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind(options.listen).await?;
    let local_addr = listener.local_addr()?;
    let state = AppState::new(DaemonClient::new(&options.socket));

    tracing::info!(
        url = %format!("http://{local_addr}/"),
        socket = %options.socket.display(),
        "server listening to daemon"
    );

    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("server stopped");
    Ok(())
}

/// Handles any termination or keyboard interrupt signals.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!("failed to listen for ctrl-c: {err}");
            std::future::pending::<()>().await;
        }
    };

    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(err) => {
                tracing::error!("failed to listen for SIGTERM: {err}");
                std::future::pending::<()>().await;
            }
        }
    };

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    tracing::info!("shutting down");
}

/// Options for how the web server should be started.
struct Options {
    listen: SocketAddr,
    socket: PathBuf,
}

/// Possible options for CLI.
enum Command {
    Run(Options),
    Help,
    Version,
}

impl Command {
    fn parse() -> Result<Self, String> {
        let mut listen = None;
        let mut socket = None;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--listen" => {
                    let value = args.next().ok_or("--listen needs a valid address")?;
                    listen = Some(
                        value
                            .parse()
                            .map_err(|err| format!("bad --listen address {value:?}: {err}"))?,
                    );
                }
                "--socket" => {
                    let value = args.next().ok_or("--socket needs a path")?;
                    socket = Some(PathBuf::from(value));
                }
                "-h" | "--help" => return Ok(Self::Help),
                "-V" | "--version" => return Ok(Self::Version),
                other => return Err(format!("unknown argument {other:?}")),
            }
        }
        Ok(Self::Run(Options {
            listen: listen.unwrap_or(SocketAddr::from(([127, 0, 0, 1], 8080))),
            socket: socket.unwrap_or_else(local::socket_path),
        }))
    }
}
