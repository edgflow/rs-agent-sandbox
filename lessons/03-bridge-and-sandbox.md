# Lesson 3 · The bridge and the `Sandbox` type

Code: `bridge/bridge.mjs`, `src/sandbox.rs`; `src/main.rs` gains `--on-host` (Lesson 1's way) and makes the
sandbox the default.

```console
$ cargo run -- canary                          # terminal 1
$ cargo run -- fix fixtures/tinycalc           # terminal 2: the agent's commands run in the VM
$ cargo run -- fix fixtures/tinycalc --on-host # Lesson 1 again, for comparison
```

Lesson 2 showed a Linux machine that starts in a fifth of a second with nothing of ours inside. This lesson puts
the agent's commands there. The agent itself does not change at all: it gets a different `Executor`.

## A · The problem

unkernel's SDK is a JavaScript library. The kernel needs JSPI and a long list of host functions written in
JavaScript, so Rust cannot host the module itself. How does a Rust program drive it, keep every decision on the
Rust side, and still let the agent stay exactly as it was?

## B · The code

**The bridge** (`bridge/bridge.mjs`, about 30 lines) boots the machine, says `{"type":"ready"}`, then reads one JSON
object per line on stdin and answers on stdout:

```text
Rust → bridge   {"type":"exec","id":1,"command":"cargo test --quiet"}
bridge → Rust   {"type":"result","id":1,"exit_code":127,"output":"sh: cargo: not found\n"}
```

It does not `await` each command before reading the next line, and it sends everything that is not a protocol line
to stderr (`console.log = console.error`).

**The `Sandbox` type** (`src/sandbox.rs`): `start()` launches `node --experimental-wasm-jspi bridge/bridge.mjs`
with a cleared environment (only `PATH` and `HOME`) and waits for `ready`. Two enums describe the protocol, one per
direction, with `#[serde(tag = "type")]`. `run()` sends an `Exec` with a fresh id and reads lines until the result
with that id arrives. And `Sandbox` implements the same trait as Lesson 1's `HostShell`:

```rust
impl Executor for Sandbox {
    async fn run(&mut self, command: &str) -> Result<Output> { ... }
}
```

In `main.rs`, the default is now `Sandbox::start()`; `--on-host` keeps Lesson 1's `HostShell`.

The real run (`captures/lesson03/run1.txt`, gpt-5.4-mini, 23 Nov 2026; long outputs cut):

```text
sandbox ready in 1393 ms
step  1  $ cargo test --quiet
          sh: cargo: not found
step  2  $ pwd && ls -la && find . -maxdepth 2 -type f | ...
          /home/user
          -rw-r--r--    1 user     user          3137 Nov  5 09:19 README
step  3  $ cat README
          Welcome to unkernel
          ===================
          This is a Linux 7.3 kernel compiled to WebAssembly, running in a web
          page or in Node.js on your machine; nothing runs on a server. ...
step  4  $ find / -maxdepth 3 -name Cargo.toml 2>/dev/null | head -50
step  5  $ find /home -maxdepth 4 -type f | ...
          README
step  6  $ which rustc || true; which cargo || true; find / -maxdepth 3 \( -name rustc -o -name cargo \) ...
  ...
step 12  $ find / -maxdepth 5 -type f \( -name 'main.rs' -o -name 'lib.rs' -o -name 'Cargo.toml' \) ...
Error: no finish after 12 steps
```

And the canary, all this time: `canary listening on 127.0.0.1:7878`, nothing else.

## C · The picture: one command's path

```text
 the model ─▶ agent loop (Rust) ─▶ Sandbox::run("cargo test --quiet")
                                     │ {"type":"exec","id":1,...}         one line on the bridge's stdin
                                     ▼
                         bridge.mjs (node) ─▶ vm.exec('sh', ['-c', ...])
                                     │
                                     ▼
                         unkernel: Linux in wasm ─▶ /bin/sh in the guest: "cargo: not found"
                                     │ {"type":"result","id":1,"exit_code":127,...}
                                     ▼
 the model ◀── "exit code: 127\nsh: cargo: not found"
```

Everything on the right of the agent loop is new. The agent loop, the model, the tools: unchanged.

## D · The rules behind it

- **One JSON object per line.** A pipe is a stream of bytes; the newline is how each side knows where a message
  ends. `serde_json` never writes a raw newline inside a string, so a line is always one whole message.
- **The `type` tag.** `#[serde(tag = "type", rename_all = "snake_case")]` turns `ToBridge::Exec { id, command }`
  into `{"type":"exec","id":1,"command":"..."}`, the shape the bridge reads, and `{"type":"ready"}` back into
  `FromBridge::Ready`.
- **Ids.** The bridge answers when each command finishes, not in order. The id says which request an answer
  belongs to, so later messages can cross without confusion (Lesson 5 needs that).
- **Least privilege, for our own code too.** The bridge is our code, but it has no use for the API key, so
  `env_clear()` keeps it out.
- **Ownership is the machine's lifetime.** `kill_on_drop(true)` on the child process: when the `Sandbox` value is
  dropped, node is killed, and the machine inside it is gone. No orphaned VM.

## E · The way forward

The commands are now somewhere harmless, and the run shows the cost: a machine with nothing of ours is also a
machine without the crate and without cargo. The agent spent twelve steps searching. Lesson 4 hands in exactly
what the task needs: the code, read-only; a place to write the fix, which we can see; and a Rust toolchain.

## F · Every important line

- `bridge.mjs`, `console.log = console.error`: a stray log line on stdout would break the protocol, so all logging
  goes to stderr.
- `bridge.mjs`, `vm.exec(...).then(...)` without `await`: the loop goes back to reading stdin at once.
- `sandbox.rs`, `Command::new("node").arg("--experimental-wasm-jspi")`: Node 24 needs the flag (Lesson 2).
- `sandbox.rs`, `.stdin(Stdio::piped()).stdout(Stdio::piped())`: our end of the two pipes; stderr is left
  inherited, so the bridge's logs show in our terminal.
- `sandbox.rs`, `lines: Lines<BufReader<ChildStdout>>`: `next_line()` gives one whole message at a time.
- `sandbox.rs`, `if let FromBridge::Result { id: answer, .. } = self.receive().await? && answer == id`: a
  let chain (Rust 2024): match the shape and check the id in one condition.
- `main.rs`, `Instant::now()` around `Sandbox::start()`: the 1393 ms printed above. Most of it is finding the cached
  Alpine image (`fetchDistro`, about 1.2 s in Lesson 2); booting is about 0.2 s.

## G · Experiment

Prediction: the build script cannot fire this time. Not because something blocks it, but because the crate is not
in the machine and there is no cargo to compile it.

The canary log of the whole run (`captures/lesson03/canary.txt`):

```text
canary listening on 127.0.0.1:7878
```

Silent. The agent's first command already answered why: `sh: cargo: not found`.

## H · Common mistakes

- "Just call the SDK from Rust." It is JavaScript with WebAssembly JSPI and many host functions; a small bridge
  process is the honest way, with Rust keeping the decisions.
- "Await each command in the bridge." Then the bridge stops reading stdin while a command runs, and any message
  that must cross (Lesson 5's questions) waits forever.
- "Print debug output in the bridge." On stdout it corrupts the protocol; it belongs on stderr.
- "A sandbox that stops everything is the goal." The run shows a perfectly safe agent that cannot do its job. The
  work is handing in exactly what the task needs, and nothing more.

## I · Challenge

Wrap the `receive` loop in `run()` with `tokio::time::timeout` so a command that never answers cannot hang the
agent. What should the model get back? Expected: an error output after the timeout, and the loop goes on. Lesson 6
does this properly, on the guest side too.
