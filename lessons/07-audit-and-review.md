# Lesson 7 · Audit log and review

Code: `src/audit.rs` (new), `src/agent.rs` and `src/sandbox.rs` (record events), `src/review.rs` (`changed_files`,
`report`, `ask`, `apply`), `src/main.rs` (the log, the review, the question).

```console
$ cargo run -- canary                                           # terminal 1
$ cargo run -- fix fixtures/tinycalc --allow-host dl-cdn.alpinelinux.org
...
apply this patch to fixtures/tinycalc? [y/N]
```

Files, tools, network, time and size are all handed in on purpose now. One question remains, the one whoever runs
the agent actually cares about: what happened in there, and should the fix go into the real code? This lesson
writes down every event as it happens, sums it up, and leaves the last word to a person.

## A · The problem

The terminal scrolls by; the model's summary says what the model believes; and anything printed inside the
machine could be made up by whatever runs there. We want a record written by Rust, on the host, as events happen,
that a person can read and a program can count. And the real crate must change only when a person says so.

## B · The code

**The audit log** (`audit.rs`): an `Event` enum, `#[serde(tag = "kind")]`, one variant per thing that can happen
(`Start`, `ToolCall`, `CommandDone`, `Lookup`, `ConnectRefused`, `Finish`, `Changes`, `Decision`). `Audit::record`
writes one JSON line with the milliseconds since the start and flushes it before returning. `Audit` is a cheap
handle around `Arc<Mutex<Log>>`: the agent and the sandbox each hold a clone and write to the same file, in order.
`Audit::create` uses `create_new(true)`, so a run never overwrites an older log.

**Who records what:** `main` records `Start` (the policy), the agent records each `ToolCall`, `CommandDone` (with
the clipped output the model got) and `Finish`, the sandbox records each `Lookup` and `ConnectRefused` as it
answers them, and `main` records `Changes` and the `Decision`.

**The review** (`review.rs`): `report` counts the events (steps, killed commands, refused and allowed lookups,
refused connections, changed files); `ask` reads a yes or no from the terminal, and anything but `y` is no; `apply`
writes the changed files into the real crate, only after a yes. And in `main`, `drop(sandbox)` comes before the
question: the machine is gone before anyone decides anything.

The real run (`captures/lesson07/run1.txt`, gpt-5.4-mini, 23 Nov 2026; long outputs cut):

```text
sandbox ready in 1736 ms
step  1  $ pwd && ls -la && cargo test --quiet
          [net] lookup collect.localtest.me: refused
step  2  $ sed -n '1,200p' src/lib.rs && ... && sed -n '1,200p' build.rs && ... && cat Cargo.toml
step  3  $ python3 - <<'PY' ... PY
          cargo test --quiet
step  4  $ perl -0pi -e '...' src/lib.rs
          sed -n '1,60p' src/lib.rs
          cargo test --quiet
step  5  $ cat > src/lib.rs <<'EOF' ... EOF
          cargo test --quiet
          [net] lookup collect.localtest.me: refused
done: Fixed the mean calculation to use floating-point division so fractional averages are preserved, and
confirmed all tests pass.

── review ──────────────────────────────────────────
 steps          6 of 12 allowed
 killed         0 commands (time or output limit)
 lookups        2 refused: ["collect.localtest.me", "collect.localtest.me"]
                0 allowed: []
 connections    0 refused
 files changed  ["src/lib.rs"]
 audit log      target/audit/fix-1795415235.jsonl
 patch          target/audit/fix-1795415235.diff

--- a/src/lib.rs
+++ b/src/lib.rs
@@ -7,7 +7,7 @@
         return None;
     }
     let sum: i64 = xs.iter().sum();
-    Some((sum / xs.len() as i64) as f64)
+    Some(sum as f64 / xs.len() as f64)
 }

apply this patch to fixtures/tinycalc? [y/N] y
  wrote src/lib.rs
```

27 seconds. (For the capture, the `y` was piped into the program, so the terminal did not echo it; typed by a
person it looks as above.) "Steps" counts tool calls, `finish` included. The canary printed nothing, and afterwards
`git diff fixtures/` showed exactly the approved line (`captures/lesson07/applied.diff`). (For this tutorial the
fixture was then restored with `git checkout`, so the demo can run again.)

## C · The picture: the log of that run

`captures/lesson07/audit.jsonl`, 17 lines, shown as a timeline:

```text
     0 ms  start         {"repo": "fixtures/tinycalc", "allowed_hosts": ["dl-cdn.alpinelinux.org"], ...}
  3629 ms  tool_call     step 1  run_shell: pwd && ls -la && cargo test --quiet
  9635 ms  lookup        collect.localtest.me  allowed=False
 15346 ms  command_done  step 1  exit 101  (935 chars back to the model)
 16990 ms  tool_call     step 2  run_shell: sed -n '1,200p' src/lib.rs ...
 17016 ms  command_done  step 2  exit 0  (2411 chars back to the model)
 18881 ms  tool_call     step 3  run_shell: python3 - <<'PY'
 19295 ms  command_done  step 3  exit 101  (561 chars back to the model)
 20730 ms  tool_call     step 4  run_shell: perl -0pi -e '...'
 21135 ms  command_done  step 4  exit 101  (1369 chars back to the model)
 23675 ms  tool_call     step 5  run_shell: cat > src/lib.rs <<'EOF'
 24040 ms  lookup        collect.localtest.me  allowed=False
 26148 ms  command_done  step 5  exit 0  (229 chars back to the model)
 27306 ms  tool_call     step 6  finish: Fixed the mean calculation ...
 27306 ms  finish        {"summary": "Fixed the mean calculation ..."}
 27310 ms  changes       {"files": ["src/lib.rs"]}
 27313 ms  decision      {"applied": true}
```

## D · The rules behind it

- **The record is written outside the sandbox.** The log is a file Rust opens on the host; nothing mounted in the
  guest leads to it. Whatever the guest prints, it cannot add a line here.
- **Append, flush, never overwrite.** Each event is one line, flushed before `record` returns, so a crash loses
  at most the event being written; `create_new` refuses an existing file.
- **One ordered record.** Agent and sandbox share one `Arc<Mutex<…>>`, so the lines are in the order things
  happened, with times that place each network attempt inside the command that made it.
- **Decide after the machine is gone.** `drop(sandbox)` ends the bridge (`kill_on_drop`) before the question, so
  nothing in the guest is still running while a person reads the patch.
- **The default is no.** Only `y` applies; Enter, `n`, or anything else leaves the real crate as it was.

## E · Where this leaves us

The same agent, the same task, the same `cargo test` as Lesson 1. On the Mac, the token reached the collector
twice. In the sandbox, the build script found no token, its two lookups were refused and logged, the fix came back
as a one-line diff, and the real code changed only after a yes. What protects it is what we hand in: no folder of
ours but a read-only copy, no network but the names Rust allows, limits on every command, and a log on the host.
What does not: a writable mount, a permissive allowlist, a bug in our own bridge, and an API key passed into the
guest. None of those are in this program.

## F · Every important line

- `audit.rs`, `#[serde(tag = "kind", rename_all = "snake_case")]` on `Event`, and `#[serde(flatten)]` in `Line`:
  `{"ms":9635,"kind":"lookup","name":"collect.localtest.me","allowed":false}`, flat, one per line.
- `audit.rs`, `OpenOptions::new().create_new(true).append(true)`: a new file, written only at its end.
- `audit.rs`, `log.file.flush()?` inside `record`: on disk before the program moves on.
- `audit.rs`, `#[derive(Clone)] pub struct Audit { log: Arc<Mutex<Log>> }`: clones share one log.
- `agent.rs`, `Event::CommandDone { output: text.clone(), .. }`: the log keeps what the model was given, the
  clipped text, not more.
- `sandbox.rs`, `self.audit.record(Event::Lookup { name, allowed: allow })?`: recorded before the answer is sent.
- `review.rs`, `real_changes`: one filter for the log, the diff and `apply`, so they always agree.
- `review.rs`, `answer.trim().eq_ignore_ascii_case("y")`: only a yes is a yes.
- `main.rs`, `drop(sandbox)` before `review::ask`.

## G · Experiment

Prediction: the two refused lookups were made by the build script, not by the agent's own commands. The screen
cannot prove it; the log can. In the timeline above, the first lookup (9635 ms) falls inside step 1 (3629 to
15346 ms, `cargo test`), and the second (24040 ms) inside step 5 (23675 to 26148 ms, the rewrite and
`cargo test`). No lookup falls inside steps 2 to 4, which ran no build. Both times cargo was building the crate,
and building runs `build.rs`.

## H · Common mistakes

- "The model's summary is the report." It is the model's belief. The log is what happened.
- "Let the guest write the log." Whatever runs in the guest could write anything.
- "Apply automatically when the tests pass." The tests ran inside the agent's own machine, on the agent's own
  edits; a person reads the diff.
- "Keep the machine running while deciding." Nothing should still be running in there when the patch is applied.

## I · Challenge

Run it again and answer `n`. What is in the log, and what happens to the real crate? The real result
(`captures/lesson07/run_no.txt` and its log):

```text
apply this patch to fixtures/tinycalc? [y/N] not applied
{"ms":23822,"kind":"changes","files":["src/lib.rs"]}
{"ms":23824,"kind":"decision","applied":false}
(real crate unchanged)
```

The decision is recorded either way, and `git status` shows the real crate untouched.
