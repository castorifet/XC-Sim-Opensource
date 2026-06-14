use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI8, Ordering};
use std::sync::OnceLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};

pub const AUTH_HOST: &str = "auth2.luvxcby.love";
pub const AUTH_PORT: u16 = 26111;
const AUTH_TIMEOUT_SECS: u64 = 10;

pub const AUTH_PENDING: i8 = -1;
pub const AUTH_DENIED: i8 = 0;
pub const AUTH_OK: i8 = 1;

pub static AUTH_STATE: AtomicI8 = AtomicI8::new(AUTH_PENDING);
static DEVICE_ID: OnceLock<String> = OnceLock::new();

pub fn device_id() -> &'static str {
    DEVICE_ID.get().map(|s| s.as_str()).unwrap_or("")
}

pub fn auth_state() -> i8 {
    AUTH_STATE.load(Ordering::Relaxed)
}

pub fn is_authorized() -> bool {
    auth_state() == AUTH_OK
}

pub fn is_pending() -> bool {
    auth_state() == AUTH_PENDING
}

pub fn is_denied() -> bool {
    auth_state() == AUTH_DENIED
}

fn device_id_path() -> Result<PathBuf> {
    Ok(PathBuf::from(crate::dir::root()?).join("device_id"))
}

fn is_valid_device_id(s: &str) -> bool {
    s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit())
}

fn generate_device_id() -> String {
    let n: u32 = ::rand::random::<u32>() % 100_000_000;
    format!("{:08}", n)
}

pub fn load_or_create_device_id() -> Result<String> {
    let path = device_id_path()?;
    if path.exists() {
        let id = std::fs::read_to_string(&path)?.trim().to_owned();
        if is_valid_device_id(&id) {
            return Ok(id);
        }
    }
    let new_id = generate_device_id();
    std::fs::write(&path, &new_id)?;
    Ok(new_id)
}

pub fn init_device_id() -> String {
    let id = load_or_create_device_id().unwrap_or_else(|_| generate_device_id());
    let _ = DEVICE_ID.set(id.clone());
    id
}

pub async fn authenticate_remote(id: String) -> Result<bool> {
    let fut = async {
        let mut stream = TcpStream::connect((AUTH_HOST, AUTH_PORT)).await?;
        let msg = format!("AUTH {}\n", id);
        stream.write_all(msg.as_bytes()).await?;
        let mut buf = vec![0u8; 64];
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok::<bool, anyhow::Error>(false);
        }
        let resp = String::from_utf8_lossy(&buf[..n]);
        let line = resp.lines().next().unwrap_or("").trim();
        Ok::<bool, anyhow::Error>(line.eq_ignore_ascii_case("OK"))
    };
    match timeout(Duration::from_secs(AUTH_TIMEOUT_SECS), fut).await {
        Ok(res) => res,
        Err(_) => Err(anyhow!("auth timeout")),
    }
}

pub fn set_state(state: i8) {
    AUTH_STATE.store(state, Ordering::Relaxed);
}
