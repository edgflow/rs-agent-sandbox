//! Lesson 7: everything that happened in a task, as it happened, in an
//! append-only JSON Lines file written by Rust on the host. Nothing in the
//! guest can write to it.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use serde::Serialize;

/// One thing that happened. Each becomes one line: `{"ms":…,"kind":…,…}`.
#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Start {
        repo: String,
        allowed_hosts: Vec<String>,
        timeout_secs: u64,
        max_output_bytes: usize,
        memory_mib: u32,
    },
    ToolCall {
        step: usize,
        tool: String,
        arguments: String,
    },
    CommandDone {
        step: usize,
        exit_code: Option<i32>,
        /// What the model got back (clipped).
        output: String,
    },
    Lookup {
        name: String,
        allowed: bool,
    },
    ConnectRefused {
        host: String,
        port: u16,
    },
    Finish {
        summary: String,
    },
    Changes {
        files: Vec<String>,
    },
    Decision {
        applied: bool,
    },
}

/// A handle to the log. Cloning it shares the same file, so the agent and
/// the sandbox write to one log, in order.
#[derive(Clone)]
pub struct Audit {
    log: Arc<Mutex<Log>>,
}

struct Log {
    file: File,
    started: Instant,
    events: Vec<Event>,
}

#[derive(Serialize)]
struct Line<'a> {
    ms: u128,
    #[serde(flatten)]
    event: &'a Event,
}

impl Audit {
    /// A new log file; refuses to overwrite one that exists.
    pub fn create(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(path)
            .with_context(|| format!("cannot create {}", path.display()))?;
        let log = Log {
            file,
            started: Instant::now(),
            events: Vec::new(),
        };
        Ok(Self {
            log: Arc::new(Mutex::new(log)),
        })
    }

    /// Appends one event and flushes it to disk before returning.
    pub fn record(&self, event: Event) -> Result<()> {
        let mut log = self.log.lock().expect("the audit log lock");
        let ms = log.started.elapsed().as_millis();
        let line = serde_json::to_string(&Line { ms, event: &event })?;
        writeln!(log.file, "{line}")?;
        log.file.flush()?;
        log.events.push(event);
        Ok(())
    }

    /// Every event so far, for the review.
    pub fn events(&self) -> Vec<Event> {
        self.log.lock().expect("the audit log lock").events.clone()
    }
}
