//! Persistent daemon meant for locally configured clients.
//!
//! Binds the node, serves the local API on a UNIX socket and runs until
//! stopped.

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
};

use lockbox_core::{BoxError, Node, config, fetch::Fetcher};
use lockbox_proto::{config::LOCAL_DB, local};
use lockbox_store::{Db, local::open};
use lockboxd::{AppState, Keypair, load_keypair, serve};

const USAGE: &str = "\
Usage: lockboxd [--socket <path>] [--user-keys]

Options:
  --socket <path>  Location to serve UNIX socket
                   (default: $LOCKBOX_SOCKET, else <runtime dir>/lockbox/daemon.sock)
  --user-keys      Use the current user's local keypair over the system's
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

    let (socket, user_keys) = match Command::parse() {
        Ok(Command::Run { socket, user_keys }) => (socket, user_keys),
        Ok(Command::Help) => {
            println!("{}", USAGE);
            return ExitCode::SUCCESS;
        }
        Ok(Command::Version) => {
            println!("lockboxd v{}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("lockboxd: {message}");
            eprintln!();
            eprintln!("{}", USAGE);
            return ExitCode::from(2);
        }
    };

    match run(socket, user_keys).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!("{err}");
            ExitCode::FAILURE
        }
    }
}

async fn run(socket: PathBuf, user_keys: bool) -> Result<(), BoxError> {
    let dir = if user_keys {
        config::user_dir()?
    } else {
        config::dir()?
    };
    let keypair = load_keypair(&dir)?;
    announce(&keypair);
    let db = open_database(&dir)?;

    let node = Arc::new(Node::bind(keypair.secret).await?);
    let node_id = node.id();
    tracing::info!(node_id = %node_id, "node bound");

    // Passively waits in the background to connect to the relay
    let online = Arc::clone(&node);
    tokio::spawn(async move {
        online.online().await;
        tracing::info!("node online");
    });

    let fetcher = Fetcher::new().map_err(|err| format!("cannot build the HTTP client: {err}"))?;
    let state = AppState::new(Arc::clone(&node), db, fetcher);
    let result = serve(&socket, state, shutdown_signal()).await;
    node.shutdown().await;
    result?;
    Ok(())
}

/// Open the local database, creating a new one when there is none.
fn open_database(dir: &Path) -> Result<Arc<Db>, BoxError> {
    let path = dir.join(LOCAL_DB);
    let db = open(&path).map_err(|err| format!("cannot open {}: {err}", path.display()))?;
    Ok(Arc::new(db))
}

/// Tells the user which account this daemon runs as.
fn announce(keypair: &Keypair) {
    let public = keypair.public();
    let fingerprint = keypair.fingerprint();
    if !keypair.created {
        tracing::info!(
            public_key = %public,
            fingerprint = %fingerprint,
            "running daemon with public key"
        );
        return;
    }
    tracing::warn!(
        path = %keypair.path.display(),
        "generated a new account keypair"
    );
    tracing::warn!(
        public_key = %public,
        fingerprint = %fingerprint,
        "public key of this account"
    );
    tracing::warn!(
        "The secret key is your account's passkey: back it up and never share it with anyone!"
    );
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

/// Possible command line options
enum Command {
    Run { socket: PathBuf, user_keys: bool },
    Help,
    Version,
}

impl Command {
    fn parse() -> Result<Self, String> {
        let mut socket = None;
        let mut user_keys = false;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--socket" => {
                    let value = args.next().ok_or("--socket needs a path")?;
                    socket = Some(PathBuf::from(value));
                }
                "--user-keys" => user_keys = true,
                "-h" | "--help" => return Ok(Self::Help),
                "-V" | "--version" => return Ok(Self::Version),
                other => return Err(format!("unknown argument {other:?}")),
            }
        }
        Ok(Self::Run {
            socket: socket.unwrap_or_else(local::socket_path),
            user_keys,
        })
    }
}
