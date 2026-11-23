//! agent-sandbox: an AI coding agent whose shell commands run where we
//! decide.
//!
//!   cargo run -- canary                  print what reaches 127.0.0.1:7878
//!   cargo run -- fix fixtures/tinycalc   the agent fixes the failing test,
//!                                        its commands run in the sandbox
//!   cargo run -- fix fixtures/tinycalc --on-host    ... on this Mac

mod agent;
mod canary;
mod host;
mod openai;
mod sandbox;

use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    about = "An AI coding agent whose shell commands run where we decide"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Listen on 127.0.0.1 and print every request that arrives.
    Canary {
        #[arg(long, default_value_t = 7878)]
        port: u16,
    },
    /// Give the agent one task: make the failing test of a crate pass.
    Fix {
        /// The crate. The agent works on a fresh copy of it.
        repo: PathBuf,
        /// The most tool calls the agent may make.
        #[arg(long, default_value_t = 12)]
        max_steps: usize,
        /// Run the agent's commands on this Mac (Lesson 1), not in the
        /// sandbox.
        #[arg(long)]
        on_host: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Cmd::Canary { port } => canary::listen(port),
        Cmd::Fix {
            repo,
            max_steps,
            on_host,
        } => {
            let key = std::env::var("OPENAI_API_KEY")
                .context("OPENAI_API_KEY is not set")?;
            let llm = openai::OpenAi::new(key);
            if on_host {
                let run = prepare_run(&repo)?;
                let shell =
                    host::HostShell::new(run.join("work"), run.join("home"));
                agent::Agent::new(llm, shell, max_steps).fix().await
            } else {
                let started = Instant::now();
                let sandbox = sandbox::Sandbox::start().await?;
                let ms = started.elapsed().as_millis();
                println!("sandbox ready in {ms} ms");
                agent::Agent::new(llm, sandbox, max_steps).fix().await
            }
        }
    }
}

/// A fresh copy of the crate and of the demo home for every run, so the
/// fixtures never change and every run starts from the same place.
fn prepare_run(repo: &Path) -> Result<PathBuf> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let run = std::env::current_dir()?
        .join("target/runs")
        .join(stamp.to_string());
    copy_dir(repo, &run.join("work"))?;
    copy_dir(Path::new("fixtures/demo-home"), &run.join("home"))?;
    println!("run folder: target/runs/{stamp}");
    Ok(run)
}

/// Copies a folder, leaving out build output (`target`).
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_name() == "target" {
            continue;
        }
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
