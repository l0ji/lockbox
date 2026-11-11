//! Fixtures shared by this crate's tests.

#![allow(dead_code)]

use std::{
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// Temporary directory for one test, removed when it goes out of scope.
pub struct Temp(PathBuf);

impl Temp {
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("lockbox-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temporary directory");
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Make `dir` read-only and report on whether writes are refused.
#[cfg(unix)]
pub fn lock(dir: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(dir, fs::Permissions::from_mode(0o500)).expect("lock directory");
    let probe = dir.join(".probe");
    let locked = fs::write(&probe, "").is_err();
    let _ = fs::remove_file(&probe);
    locked
}

/// Unlock `dir` and give it full permissions again.
#[cfg(unix)]
pub fn unlock(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).expect("unlock directory");
}

/// A reply which the stub server gives to every request.
#[derive(Debug, Clone)]
pub struct Reply {
    /// Status code to answer with.
    pub status: u16,
    /// Body to answer with.
    pub body: String,
    /// Validator to answer with.
    pub etag: Option<String>,
    /// How long the server takes before it answers.
    pub delay: Option<std::time::Duration>,
}

impl Reply {
    /// A document which the server has.
    pub fn document(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: body.into(),
            etag: None,
            delay: None,
        }
    }

    /// The same reply, answered after `delay`.
    pub fn after(mut self, delay: std::time::Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    /// The same document with a validator which it can be asked for.
    pub fn with_etag(mut self, etag: impl Into<String>) -> Self {
        self.etag = Some(etag.into());
        self
    }
}

/// A stub HTTP server on a port the system picks.
pub struct Site {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Site {
    /// Serve `reply` on every request until the test ends.
    pub async fn start(reply: Reply) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind the stub server");
        let addr = listener.local_addr().expect("stub server address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);

        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let request = read_request(&mut stream).await;
                let asked = request.lines().any(|line| {
                    line.to_lowercase().starts_with("if-none-match:")
                        && reply
                            .etag
                            .as_deref()
                            .is_some_and(|etag| line.contains(etag))
                });
                seen.lock().expect("requests").push(request);
                if let Some(delay) = reply.delay {
                    tokio::time::sleep(delay).await;
                }
                let _ = stream.write_all(&answer(&reply, asked)).await;
                let _ = stream.shutdown().await;
            }
        });

        Self { addr, requests }
    }

    /// Address of a path on the stub server.
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    /// Every request which the stub server was asked, in order.
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests").clone()
    }
}

/// The head of one request, which is everything the stub server reads.
async fn read_request(stream: &mut tokio::net::TcpStream) -> String {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    while let Ok(read) = stream.read(&mut chunk).await {
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&buffer).into_owned()
}

/// The whole reply for one request.
fn answer(reply: &Reply, asked: bool) -> Vec<u8> {
    if asked && reply.etag.is_some() {
        return b"HTTP/1.1 304 Not Modified\r\nConnection: close\r\n\r\n".to_vec();
    }

    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        reply.status,
        reason(reply.status),
        reply.body.len()
    );
    if let Some(etag) = &reply.etag {
        head.push_str(&format!("Etag: {etag}\r\n"));
    }
    head.push_str("\r\n");

    let mut response = head.into_bytes();
    response.extend_from_slice(reply.body.as_bytes());
    response
}

/// Reason phrase of a status code.
fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        304 => "Not Modified",
        404 => "Not Found",
        _ => "Unknown",
    }
}
