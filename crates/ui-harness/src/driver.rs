//! TCP client for the desktop app's ui-test driver.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

pub struct Driver {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
}

impl Driver {
    /// Connect, retrying until the app's listener is up.
    pub fn connect(port: u16, timeout: Duration) -> Result<Self> {
        let start = Instant::now();
        loop {
            if let Ok(stream) = TcpStream::connect(("127.0.0.1", port)) {
                stream.set_read_timeout(Some(Duration::from_secs(30)))?;
                let reader = BufReader::new(stream.try_clone()?);
                let mut driver = Self { stream, reader };
                if driver.send(&json!({"cmd": "ping"})).is_ok() {
                    return Ok(driver);
                }
            }
            if start.elapsed() > timeout {
                bail!(
                    "ui-test driver did not come up on port {port} — the app binary \
                     was likely rebuilt without the driver (any `cargo test/build -p \
                     streamx-desktop` without features drops it); rerun: \
                     cargo build -p streamx-desktop --features ui-test"
                );
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    pub fn send(&mut self, cmd: &Value) -> Result<Value> {
        let mut line = cmd.to_string();
        line.push('\n');
        self.stream.write_all(line.as_bytes())?;
        let mut reply = String::new();
        self.reader
            .read_line(&mut reply)
            .context("driver reply read")?;
        let value: Value = serde_json::from_str(reply.trim())?;
        if value.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            bail!("driver command failed: {cmd} -> {value}");
        }
        Ok(value)
    }

    pub fn page(&mut self) -> Result<String> {
        let v = self.send(&json!({"cmd": "page"}))?;
        Ok(v["page"].as_str().unwrap_or_default().to_string())
    }

    pub fn navigate(&mut self, page: &str) -> Result<()> {
        self.send(&json!({"cmd": "navigate", "page": page}))?;
        Ok(())
    }

    pub fn back(&mut self) -> Result<()> {
        self.send(&json!({"cmd": "back"}))?;
        Ok(())
    }

    pub fn keys(&mut self, keys: &[&str]) -> Result<()> {
        self.send(&json!({"cmd": "keys", "keys": keys}))?;
        Ok(())
    }

    pub fn type_text(&mut self, text: &str) -> Result<()> {
        self.send(&json!({"cmd": "type", "text": text}))?;
        Ok(())
    }

    pub fn state(&mut self) -> Result<Value> {
        self.send(&json!({"cmd": "state"}))
    }

    /// Capture the app window via GPUI's own renderer (pixel-identical
    /// across platforms).
    pub fn screenshot(&mut self, path: &std::path::Path) -> Result<()> {
        self.send(&json!({"cmd": "screenshot", "path": path.to_string_lossy()}))?;
        Ok(())
    }

    pub fn quit(&mut self) {
        let _ = self.send(&json!({"cmd": "quit"}));
    }

    /// Poll `state` until `pred` passes or the timeout elapses.
    pub fn wait_for(
        &mut self,
        timeout: Duration,
        what: &str,
        mut pred: impl FnMut(&Value) -> bool,
    ) -> Result<Value> {
        let start = Instant::now();
        loop {
            let state = self.state()?;
            if pred(&state) {
                return Ok(state);
            }
            if start.elapsed() > timeout {
                bail!("timed out waiting for {what}; last state: {state}");
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}
