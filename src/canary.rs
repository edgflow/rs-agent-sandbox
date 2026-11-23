//! The canary: a listener on 127.0.0.1 that stands in for an attacker's
//! collector and prints every request that reaches it.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;

use anyhow::Result;

pub fn listen(port: u16) -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    println!("canary listening on 127.0.0.1:{port}");
    for stream in listener.incoming() {
        let stream = stream?;
        let mut reader = BufReader::new(&stream);
        let mut request_line = String::new();
        reader.read_line(&mut request_line)?;
        let mut length = 0;
        loop {
            let mut header = String::new();
            reader.read_line(&mut header)?;
            if header.trim().is_empty() {
                break;
            }
            let header = header.to_ascii_lowercase();
            if let Some(value) = header.strip_prefix("content-length:") {
                length = value.trim().parse()?;
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body)?;
        println!("CANARY HIT: {}", request_line.trim_end());
        println!("{}", String::from_utf8_lossy(&body).trim_end());
        (&stream).write_all(b"HTTP/1.1 204 No Content\r\n\r\n")?;
    }
    Ok(())
}
