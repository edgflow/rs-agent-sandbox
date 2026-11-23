//! agent-sandbox: an AI coding agent whose shell commands run where we
//! decide.
//!
//!   cargo run -- canary                  print what reaches 127.0.0.1:7878
//!   cargo run -- fix fixtures/tinycalc   the agent fixes the failing test,
//!                                        its commands run in the sandbox
//!   cargo run -- fix fixtures/tinycalc --on-host    ... on this Mac
//!   cargo run -- sh 'wget -O- http://example.com' --allow-host example.com
//!                                        one command in the sandbox, no agent

mod agent;
mod canary;
mod host;
mod openai;
mod policy;
mod review;
mod sandbox;

use agent::Executor;
use policy::Policy;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};

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
        /// The crate. The agent works on a copy; the crate is never written.
        repo: PathBuf,
        /// The disk with rust and cargo (made by bridge/make-toolbox.sh).
        #[arg(long, default_value = "bridge/toolbox.img")]
        toolbox: PathBuf,
        /// The most tool calls the agent may make.
        #[arg(long, default_value_t = 12)]
        max_steps: usize,
        /// Run the agent's commands on this Mac (Lesson 1), not in the
        /// sandbox.
        #[arg(long)]
        on_host: bool,
        #[command(flatten)]
        policy: PolicyArgs,
    },
    /// Run commands in the sandbox one after another, without the agent.
    Sh {
        commands: Vec<String>,
        /// The crate mounted at /src and copied to /work.
        #[arg(long, default_value = "fixtures/tinycalc")]
        repo: PathBuf,
        #[arg(long, default_value = "bridge/toolbox.img")]
        toolbox: PathBuf,
        #[command(flatten)]
        policy: PolicyArgs,
    },
}

/// The sandbox's policy, from the command line.
#[derive(Args)]
struct PolicyArgs {
    /// A host name the sandbox may look up and connect to (repeatable).
    /// None by default: no network at all.
    #[arg(long = "allow-host")]
    allow_hosts: Vec<String>,
    /// Seconds one command may run before it gets SIGKILL.
    #[arg(long, default_value_t = 60)]
    timeout_secs: u64,
    /// Bytes one command may print before it gets SIGKILL.
    #[arg(long, default_value_t = 1 << 20)]
    max_output: usize,
    /// The guest's memory, in MiB.
    #[arg(long, default_value_t = 1024)]
    memory_mib: u32,
}

impl From<PolicyArgs> for Policy {
    fn from(args: PolicyArgs) -> Self {
        Policy {
            allowed_hosts: args.allow_hosts,
            command_timeout: Duration::from_secs(args.timeout_secs),
            max_output_bytes: args.max_output,
            memory_mib: args.memory_mib,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Cmd::Canary { port } => canary::listen(port),
        Cmd::Sh {
            commands,
            repo,
            toolbox,
            policy,
        } => {
            check_toolbox(&toolbox)?;
            let mut sandbox =
                sandbox::Sandbox::start(&repo, &toolbox, policy.into()).await?;
            for command in commands {
                println!("$ {command}");
                let started = Instant::now();
                let out = sandbox.run(&command).await?;
                let secs = started.elapsed().as_secs_f32();
                print!("{}", out.text);
                println!("(exit code {:?}, {secs:.1} s)\n", out.exit_code);
            }
            Ok(())
        }
        Cmd::Fix {
            repo,
            toolbox,
            max_steps,
            on_host,
            policy,
        } => {
            let key = std::env::var("OPENAI_API_KEY")
                .context("OPENAI_API_KEY is not set")?;
            let llm = openai::OpenAi::new(key);
            if on_host {
                let run = prepare_run(&repo)?;
                let shell =
                    host::HostShell::new(run.join("work"), run.join("home"));
                agent::Agent::new(llm, shell, max_steps).fix().await?;
                return Ok(());
            }
            check_toolbox(&toolbox)?;
            let started = Instant::now();
            let sandbox =
                sandbox::Sandbox::start(&repo, &toolbox, policy.into()).await?;
            let ms = started.elapsed().as_millis();
            println!("sandbox ready in {ms} ms");
            let mut sandbox =
                agent::Agent::new(llm, sandbox, max_steps).fix().await?;
            let patch = review::diff(&repo, &sandbox.changes().await?)?;
            let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            let file = format!("target/fix-{stamp}.diff");
            std::fs::write(&file, &patch)?;
            println!("\nthe agent's changes ({file}), not applied:\n{patch}");
            Ok(())
        }
    }
}

fn check_toolbox(toolbox: &Path) -> Result<()> {
    if !toolbox.exists() {
        bail!(
            "{} is missing: run bridge/make-toolbox.sh",
            toolbox.display()
        );
    }
    Ok(())
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
