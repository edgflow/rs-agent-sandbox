//! Lesson 5: what the sandbox may reach. Rust decides; the bridge asks.

pub struct Policy {
    allowed_hosts: Vec<String>,
}

impl Policy {
    pub fn new(allowed_hosts: Vec<String>) -> Self {
        Self { allowed_hosts }
    }

    /// Exact names only: allowing `example.com` does not allow
    /// `collect.example.com`. With an empty list, nothing is allowed.
    pub fn allows(&self, host: &str) -> bool {
        self.allowed_hosts.iter().any(|allowed| allowed == host)
    }
}
