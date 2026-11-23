# agent-sandbox

An AI coding agent in Rust whose shell commands run where we decide: first on our own machine, then inside a
Linux machine that lives in a WebAssembly module ([unkernel](https://github.com/pithings/unkernel)), with Rust
deciding what it may read, where it may connect and how long it may run.

One task is followed all the way: `fixtures/tinycalc` has a failing test, and its `build.rs` is a demo payload
that reads a credentials file and posts it to a collector. The file is a fake canary
(`fixtures/demo-home`), and the collector is our own listener on 127.0.0.1, so nothing ever leaves the machine.

Needs Rust 1.100 and Node 24 (tested with 24.19, which needs `--experimental-wasm-jspi`). Then `npm install`
in `bridge/`.

```console
$ cargo run -- canary                     # terminal 1: print what reaches 127.0.0.1:7878
$ export OPENAI_API_KEY=...               # terminal 2
$ cargo run -- fix fixtures/tinycalc             # the agent's commands run in the sandbox
$ cargo run -- fix fixtures/tinycalc --on-host   # ... on this machine (Lesson 1)
```

| # | Lesson | Write-up |
|---|---|---|
| 1 | An agent with a shell | [lessons/01-agent-with-a-shell.md](lessons/01-agent-with-a-shell.md) |
| 2 | A Linux machine inside a WebAssembly module | [lessons/02-linux-in-a-wasm-module.md](lessons/02-linux-in-a-wasm-module.md) |
| 3 | The bridge and the `Sandbox` type | [lessons/03-bridge-and-sandbox.md](lessons/03-bridge-and-sandbox.md) |

Every run on screen is a real run; its output is quoted in the write-up.
