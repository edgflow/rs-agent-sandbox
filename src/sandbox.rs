//! The agent's commands run in a Linux machine inside a WebAssembly module,
//! through the bridge (`bridge/bridge.mjs`). Lesson 3: the machine and the
//! protocol. Lesson 4: the crate in (read-only), the work folder, the
//! toolchain disk, and the changes back out.

use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::agent::{Executor, Output};

/// What Rust sends to the bridge, one JSON object per line.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ToBridge<'a> {
    Exec { id: u64, command: &'a str },
    Changes { id: u64 },
}

/// What the bridge sends back.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum FromBridge {
    Ready,
    Result {
        id: u64,
        exit_code: Option<i32>,
        output: String,
    },
    Changes {
        id: u64,
        files: Vec<Changed>,
    },
}

/// A file the agent created, changed or removed in its work folder:
/// its path in the crate, and its text now (`None`: removed).
#[derive(Deserialize)]
pub struct Changed {
    pub path: String,
    pub content: Option<String>,
}

pub struct Sandbox {
    // Kept so the bridge, and the machine in it, end when we drop it.
    _bridge: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

impl Sandbox {
    /// Starts the bridge and waits until its machine has booted, with
    /// `krate` mounted read-only at /src and copied to /work, and the
    /// toolchain disk attached.
    pub async fn start(krate: &Path, toolbox: &Path) -> Result<Self> {
        let mut bridge = Command::new("node")
            .arg("--experimental-wasm-jspi")
            .arg("bridge/bridge.mjs")
            .arg(krate.canonicalize()?)
            .arg(toolbox.canonicalize()?)
            // The bridge does not need our API key either: only PATH (to
            // find node) and HOME (where unkernel caches the Alpine image).
            .env_clear()
            .env("PATH", std::env::var("PATH")?)
            .env("HOME", std::env::var("HOME")?)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context("cannot start node")?;
        let stdin = bridge.stdin.take().context("no stdin")?;
        let stdout = bridge.stdout.take().context("no stdout")?;
        let mut sandbox = Self {
            _bridge: bridge,
            stdin,
            lines: BufReader::new(stdout).lines(),
            next_id: 0,
        };
        match sandbox.receive().await? {
            FromBridge::Ready => Ok(sandbox),
            _ => bail!("the bridge answered before ready"),
        }
    }

    /// Every file the agent changed in /work, read by the host. Nothing
    /// that runs in the guest is asked: not `diff`, not `git`.
    pub async fn changes(&mut self) -> Result<Vec<Changed>> {
        let id = self.fresh_id();
        self.send(&ToBridge::Changes { id }).await?;
        loop {
            if let FromBridge::Changes { id: answer, files } =
                self.receive().await?
                && answer == id
            {
                return Ok(files);
            }
        }
    }

    fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    async fn send(&mut self, message: &ToBridge<'_>) -> Result<()> {
        let mut line = serde_json::to_string(message)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await?;
        Ok(())
    }

    async fn receive(&mut self) -> Result<FromBridge> {
        let line = self.lines.next_line().await?;
        let line = line.context("the bridge exited")?;
        Ok(serde_json::from_str(&line)?)
    }
}

impl Executor for Sandbox {
    async fn run(&mut self, command: &str) -> Result<Output> {
        let id = self.fresh_id();
        self.send(&ToBridge::Exec { id, command }).await?;
        loop {
            if let FromBridge::Result {
                id: answer,
                exit_code,
                output,
            } = self.receive().await?
                && answer == id
            {
                return Ok(Output {
                    exit_code,
                    text: output,
                });
            }
        }
    }
}
