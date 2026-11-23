# Lesson 6 · Limits

Code: `bridge/bridge.mjs` (`signal`, `maxBuffer`, `memoryMiB` from Rust), `src/policy.rs` (the limits join the
host list), `src/main.rs` (`PolicyArgs`: one set of flags for `fix` and `sh`; `sh` takes several commands),
`src/sandbox.rs` (each `exec` carries its limits; a Rust-side grace timeout), `src/agent.rs` (`clip`, with tests).

```console
$ cargo run -- sh --timeout-secs 3 'while :; do :; done' 'echo still alive'
$ cargo run -- sh --max-output 100000 'yes' 'echo still alive'
$ cargo run -- sh --memory-mib 256 "awk 'BEGIN { s = \"x\"; while (1) s = s s }'" 'echo still alive'
$ cargo test
```

Files, tools and network are handed in on purpose now. What is left is quantity: a command that never ends, one
that prints without end, one that eats memory, and an agent that never stops. Inside the sandbox none of these
can reach our files, but each can still stall the task or flood the model.

## A · The problem

A test that loops forever, a `yes` by mistake, a program that allocates until it dies: on the Mac they are an
annoyance; for an agent they are a stuck run, a huge bill (every output goes back to the model) or a process that
never ends. Each needs a limit, enforced where it happens, with a clear message for the model.

## B · The code

**The bridge** passes three limits to unkernel:

```js
const [crate, toolbox, memoryMiB] = process.argv.slice(2);
...
createKernel({ ..., memoryMiB: Number(memoryMiB) })       // all the memory the guest will ever have
...
vm.exec('sh', ['-c', command], {
  ...,
  signal: AbortSignal.timeout(timeout_ms),                 // too long: SIGKILL
  maxBuffer: max_output,                                   // too much output: SIGKILL
}).then(ok, (e) => send({ ..., output:
  e.code === 'ABORTED' ? `killed: still running after ${timeout_ms / 1000} s\n`
  : e.code === 'ENOBUFS' ? `killed: printed more than ${max_output} bytes\n` : ... }));
```

**Rust** decides the numbers: `Policy` gains `command_timeout`, `max_output_bytes` and `memory_mib`; a
`PolicyArgs` struct of clap flags (`#[command(flatten)]` in `fix` and `sh`, defaults 60 s, 1 MiB, 1024 MiB) turns
into a `Policy` with `From`. Every `Exec` message carries `timeout_ms` and `max_output`. And `Sandbox::run` waits
at most the timeout plus 10 seconds, in case the bridge itself stops answering.

**The model** gets at most 4,000 characters of any output: `clip` keeps the start and the end, and says how much
was cut between them. The agent itself has had a limit since Lesson 1: 12 steps (Lesson 3's run hit it).

The experiments below are the lesson's real runs. The agent run with every limit set
(`captures/lesson06/run1.txt`) behaved like Lesson 5's: 5 steps, 26 seconds, the build script's lookup refused
twice, the same one-line diff, the canary silent. Limits that fit the work do not get in its way.

## C · The picture: where each limit sits

```text
 the model     at most 4,000 characters per tool result (start + end)        clip, in Rust
 the agent     at most 12 steps                                              max_steps, since Lesson 1
 one command   at most 60 s, then SIGKILL                                     AbortSignal, in the guest
               at most 1 MiB of output, then SIGKILL                          maxBuffer, in the guest
 the machine   1024 MiB, all it will ever have                                memoryMiB, fixed at boot
 the bridge    Rust waits at most timeout + 10 s; a late answer is ignored by its id
```

## D · The rules behind it

- **Kill inside, not just stop waiting.** A timeout only in Rust would let the command run on in the guest,
  burning CPU. The `AbortSignal` makes unkernel kill the program (SIGKILL), and the machine carries on.
- **An output cap protects the bridge and the model.** `maxBuffer` is per stream (stdout and stderr each); past it
  the program is killed. What the program printed before the kill is not returned, only the message.
- **Memory is fixed at boot.** `memoryMiB` is the guest's whole RAM. An allocation beyond it fails inside the
  guest (`awk: out of memory`), and nothing outside the guest grows.
- **Ids make a late answer harmless.** If Rust gives up waiting, a result that arrives later carries an id nobody
  waits for, and is skipped.
- **Honest limit: SIGKILL reaches the command, not everything it started.** A process the command put in the
  background survives (the challenge below). What ends it for sure is the machine itself, which is thrown away
  with the task.

## E · The way forward

Every boundary is in place: files, tools, network, time, size. One question is left, and it is the one that
matters to whoever runs this: what exactly happened, and should the fix be applied? Lesson 7.

## F · Every important line

- `bridge.mjs`, `AbortSignal.timeout(timeout_ms)`: a signal that aborts itself; `vm.exec` kills the program when
  it fires and rejects with `ABORTED`.
- `bridge.mjs`, `maxBuffer: max_output`: past it, SIGKILL and `ENOBUFS`.
- `bridge.mjs`, `e.code === 'ABORTED' ? ... : e.code === 'ENOBUFS' ? ...`: a plain sentence for the model instead
  of an error object.
- `policy.rs`, `pub command_timeout: Duration`: a `Duration`, not a bare number, so the unit is in the type.
- `main.rs`, `#[derive(Args)] struct PolicyArgs` with `#[command(flatten)]`: the same flags on two subcommands.
- `main.rs`, `impl From<PolicyArgs> for Policy`, then `policy.into()`: the conversion in one place.
- `main.rs`, `#[arg(long, default_value_t = 1 << 20)]`: 1 MiB.
- `sandbox.rs`, `tokio::time::timeout(grace, self.answer(id)).await.context(...)??`: the outer `?` for the
  timeout, the inner one for the answer.
- `agent.rs`, `clip`: counts `chars()`, not bytes, so it never cuts a UTF-8 character in half; two unit tests
  (`cargo test`: 2 passed).

## G · Experiment

Predict for each: is the command stopped, how fast, and does the machine survive? (`captures/lesson06/experiments.txt`)

```text
$ while :; do :; done                          (--timeout-secs 3)
killed: still running after 3 s
(exit code None, 3.0 s)
$ echo still alive
still alive

$ yes                                          (--max-output 100000)
killed: printed more than 100000 bytes
(exit code None, 0.3 s)
$ echo still alive
still alive

$ awk 'BEGIN { s = "x"; while (1) s = s s }'    (--memory-mib 256)
awk: out of memory
(exit code Some(1), 0.5 s)
$ echo still alive; free -m | head -2
still alive
              total        used        free      shared  buff/cache   available
Mem:            242           7         230           1           5         228
```

Each one stopped, in 3 s, 0.3 s and 0.5 s, and each time the next command ran in the same machine.

## H · Common mistakes

- "A timeout in Rust is enough." It stops the waiting, not the program.
- "Output is free." Every byte goes back to the model, and `yes` passes 100 KB in a third of a second.
- "Limit memory with `ulimit` in the guest." The agent has `sudo`; a limit the guest can change is a suggestion.
  `memoryMiB` is outside its reach.
- "One limit per task." Each layer needs its own: the model's context, the steps, each command, the machine.

## I · Challenge

`cargo run -- sh --timeout-secs 2 'sleep 100 & sleep 50' 'ps -o pid,args | grep sleep'`: after the kill, is
anything still sleeping? Predict, then run. The real result (`captures/lesson06/challenge_background.txt`):

```text
$ sleep 100 & sleep 50
killed: still running after 2 s
(exit code None, 2.1 s)

$ ps -o pid,args | grep -v grep | grep sleep || echo "no sleep left"
  112 sleep 100
```

The background `sleep 100` outlived the kill. Within a task it lives on; when the task ends, the whole machine,
and every process in it, goes away.
