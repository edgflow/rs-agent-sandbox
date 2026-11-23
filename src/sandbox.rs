//! The agent's commands run in a Linux machine inside a WebAssembly module,
//! through the bridge (`bridge/bridge.mjs`). Lesson 3: the machine and the
//! protocol. Lesson 4: the crate in (read-only), the work folder, the
//! toolchain disk, and the changes back out. Lesson 5: every DNS question
//! from the guest is answered here, by the policy. Lesson 6: every command
//! carries its limits.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::agent::{Executor, Output};
use crate::policy::Policy;

/// What Rust sends to the bridge, one JSON object per line.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ToBridge<'a> {
    Exec {
        id: u64,
        command: &'a str,
        timeout_ms: u128,
        max_output: usize,
    },
    Changes {
        id: u64,
    },
    Answer {
        question: u64,
        allow: bool,
    },
}

/// What the bridge sends back: answers to our requests (with their `id`),
/// and the bridge's own questions and reports.
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
    Dns {
        question: u64,
        name: String,
    },
    Refused {
        host: String,
        port: u16,
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
    policy: Policy,
}

impl Sandbox {
    /// Starts the bridge and waits until its machine has booted, with
    /// `krate` mounted read-only at /src and copied to /work, the toolchain
    /// disk attached, and the network asking `policy`.
    pub async fn start(
        krate: &Path,
        toolbox: &Path,
        policy: Policy,
    ) -> Result<Self> {
        let mut bridge = Command::new("node")
            .arg("--experimental-wasm-jspi")
            .arg("bridge/bridge.mjs")
            .arg(krate.canonicalize()?)
            .arg(toolbox.canonicalize()?)
            .arg(policy.memory_mib.to_string())
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
            policy,
        };
        match sandbox.answer(0).await? {
            FromBridge::Ready => Ok(sandbox),
            _ => bail!("the bridge answered before ready"),
        }
    }

    /// Every file the agent changed in /work, read by the host. Nothing
    /// that runs in the guest is asked: not `diff`, not `git`.
    pub async fn changes(&mut self) -> Result<Vec<Changed>> {
        let id = self.fresh_id();
        self.send(&ToBridge::Changes { id }).await?;
        match self.answer(id).await? {
            FromBridge::Changes { files, .. } => Ok(files),
            _ => bail!("the bridge sent the wrong answer"),
        }
    }

    /// Reads until the answer to request `id` arrives (`Ready` counts as the
    /// answer to 0), answering the bridge's questions on the way.
    async fn answer(&mut self, id: u64) -> Result<FromBridge> {
        loop {
            match self.receive().await? {
                FromBridge::Dns { question, name } => {
                    let allow = self.policy.allows(&name);
                    let verdict = if allow { "allowed" } else { "refused" };
                    println!("          [net] lookup {name}: {verdict}");
                    self.send(&ToBridge::Answer { question, allow }).await?;
                }
                FromBridge::Refused { host, port } => {
                    println!("          [net] connect {host}:{port}: refused");
                }
                FromBridge::Ready if id == 0 => return Ok(FromBridge::Ready),
                answer @ (FromBridge::Result { id: got, .. }
                | FromBridge::Changes { id: got, .. })
                    if got == id =>
                {
                    return Ok(answer);
                }
                _ => {}
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
        let timeout = self.policy.command_timeout;
        self.send(&ToBridge::Exec {
            id,
            command,
            timeout_ms: timeout.as_millis(),
            max_output: self.policy.max_output_bytes,
        })
        .await?;
        // The guest kills the command at `timeout`; this is for a bridge
        // that stops answering at all. A late answer is ignored by its id.
        let grace = timeout + Duration::from_secs(10);
        let answer = tokio::time::timeout(grace, self.answer(id))
            .await
            .context("the bridge did not answer in time")??;
        match answer {
            FromBridge::Result {
                exit_code, output, ..
            } => Ok(Output {
                exit_code,
                text: output,
            }),
            _ => bail!("the bridge sent the wrong answer"),
        }
    }
}
