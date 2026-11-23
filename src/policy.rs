//! What the sandbox may reach (Lesson 5) and how much it may use (Lesson 6).
//! Rust decides; the bridge asks, and enforces what it is told.

use std::time::Duration;

pub struct Policy {
    /// Host names the guest may look up and connect to: exact names only.
    pub allowed_hosts: Vec<String>,
    /// How long one command may run before it gets SIGKILL.
    pub command_timeout: Duration,
    /// How much one command may print (stdout and stderr, each) before it
    /// gets SIGKILL.
    pub max_output_bytes: usize,
    /// All the memory the guest has, for everything in it.
    pub memory_mib: u32,
}

impl Policy {
    /// Exact names only: allowing `example.com` does not allow
    /// `collect.example.com`. With an empty list, nothing is allowed.
    pub fn allows(&self, host: &str) -> bool {
        self.allowed_hosts.iter().any(|allowed| allowed == host)
    }
}
