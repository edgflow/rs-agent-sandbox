//! Lesson 1: the agent's commands run right here, on this Mac, as us.

use std::path::PathBuf;

use anyhow::Result;
use tokio::process::Command;

use crate::agent::{Executor, Output};

pub struct HostShell {
    workdir: PathBuf,
    home: PathBuf,
}

impl HostShell {
    pub fn new(workdir: PathBuf, home: PathBuf) -> Self {
        Self { workdir, home }
    }
}

impl Executor for HostShell {
    async fn run(&mut self, command: &str) -> Result<Output> {
        // Our own HOME, to find the Rust toolchain.
        let real_home = std::env::var("HOME")?;
        let out = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&self.workdir)
            // A child inherits everything we have, the API key included.
            // This demo hands over only what cargo needs, and a demo HOME
            // with a fake credentials file instead of the real one.
            .env_clear()
            .env("PATH", format!("{real_home}/.cargo/bin:/usr/bin:/bin"))
            .env("RUSTUP_HOME", format!("{real_home}/.rustup"))
            .env("HOME", &self.home)
            .env("CARGO_HOME", self.home.join(".cargo"))
            .output()
            .await?;
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        Ok(Output {
            exit_code: out.status.code(),
            text,
        })
    }
}
