# Lesson 4 · The code in, the fix out, and the tools

Code: `bridge/make-toolbox.sh`, `bridge/bridge.mjs` (mounts, the work copy, change tracking), `src/sandbox.rs`
(`start(krate, toolbox)`, `changes()`), `src/review.rs` (new), `src/agent.rs` (`fix` hands the executor back),
`src/main.rs` (`--toolbox`, the diff). New dependency: `similar` 3.2.1.

```console
$ bridge/make-toolbox.sh                       # once: a disk with rust and cargo (about 35 s)
$ cargo run -- canary                          # terminal 1
$ cargo run -- fix fixtures/tinycalc           # terminal 2
```

Lesson 3 ended with a safe agent that could not work: no crate, no cargo. This lesson hands in exactly what the
task needs, and nothing that undoes Lesson 3: the code, read-only; a place to write that we can watch; a
toolchain. And the result comes back as a diff that a human reads before anything touches the real crate.

## A · The problem

The obvious move, mounting the real folder writable, gives the agent (and every build script it runs) our real
files again. We want three different things: a copy to work on, an original nobody in the guest can change, and
the result in a form we can review.

## B · The code

**The toolbox** (`make-toolbox.sh`, once): the unkernel CLI boots Alpine with a data disk, `toolbox.img`, and
installs Alpine's `rust` and `cargo` onto it. Real run: 34 s, 552 MB used (`captures/lesson04/make-toolbox.raw`).
This one-time setup uses the package mirror; the agent's tasks do not.

**The bridge** now builds the machine around the task:

```js
const work = new MemoryFs(treeOf(crate));          // an in-memory copy, without target/
work.onChange((change) => { changed.add(change.path); ... });
const vm = await createKernel({
  distro: await fetchDistro('alpine'),
  memoryMiB: 1024,
  disks: [await openDisk(toolbox, { snapshot: true })],   // rust + cargo, writes thrown away
  mounts: {
    '/src': { fs: nodeFs(crate), readOnly: true },        // the original
    '/work': work,                                         // the copy the agent changes
  },
});
```

Each command runs with `cwd: '/work'` and `CARGO_TARGET_DIR=/tmp/target`. A new request, `{"type":"changes"}`,
answers with every changed path and its text now (or `null`).

**Rust**: `Sandbox::start(krate, toolbox)` passes both paths; `Sandbox::changes()` asks for the list;
`review::diff` reads the real crate on the host and makes a unified diff with `similar`; `main.rs` saves it under
`target/` and prints it. Nothing applies it. `Agent::fix` now returns the executor when the model calls `finish`,
so `main` can ask the sandbox what the agent left.

The real run (`captures/lesson04/run1.txt`, gpt-5.4-mini, 23 Nov 2026; long outputs cut):

```text
sandbox ready in 1295 ms
step  1  $ pwd && ls -la && cargo test --quiet
          /work
          -rw-r--r--    1 user     user          1443 Nov 23 06:07 build.rs
          ...
          tests::mean_keeps_the_fraction --- FAILED
step  2  $ sed -n '1,200p' src/lib.rs && ... sed -n '1,200p' Cargo.toml
              Some((sum / xs.len() as i64) as f64)
step  3  $ python - <<'PY' ... PY
          cargo test --quiet
          tests::mean_keeps_the_fraction --- FAILED
step  4  $ perl -0pi -e '...' src/lib.rs
          cargo test --quiet
          tests::mean_keeps_the_fraction --- FAILED
step  5  $ cat > src/lib.rs <<'RS'
          ... the whole file, with  Some(sum as f64 / xs.len() as f64) ...
          RS
          cargo test --quiet
          test result: ok. 3 passed; 0 failed; ...
done: Fixed the mean calculation to divide in floating point before truncation, and cargo test now passes.

the agent's changes (target/fix-1795414081.diff), not applied:
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -7,7 +7,7 @@
         return None;
     }
     let sum: i64 = xs.iter().sum();
-    Some((sum / xs.len() as i64) as f64)
+    Some(sum as f64 / xs.len() as f64)
 }
```

35 seconds from start to diff. This Alpine has neither `python` nor `perl` (`which` finds none,
`captures/lesson04/why_edits_failed.json`), so steps 3 and 4 changed nothing and the agent wrote the whole file
instead. The diff shows the one line that matters. The canary printed nothing, and `git status` showed the real
crate untouched.

## C · The picture

```text
 the guest (Alpine on unkernel)
 ├─ /src          the crate, read-only (a host folder)          a write: Read-only file system
 ├─ /work         an in-memory copy, held by the bridge          every change reported to the host
 ├─ /tmp/target   build output, guest memory only
 └─ rust, cargo   from toolbox.img, opened in throw-away mode    gone when the task ends

 the result:  agent edits /work ─▶ the bridge notes the path ─▶ Rust asks "changes"
              ─▶ the bridge reads the text (host side) ─▶ Rust diffs against the real crate ─▶ a patch, not applied
```

The build script ran too, in the guest: it found no credentials (`/home/user/.cargo` does not exist there) and
had no network to send anything with.

## D · The rules behind it

- **A read-only mount is refused by the host.** With `readOnly: true` the host answers every change with EROFS
  (the SDK's documentation of `MountOptions`), so not even root in the guest can write: `sudo` changes nothing.
- **A throw-away disk.** `openDisk(path, { snapshot: true })`: writes go to an in-memory layer and are lost at exit,
  so the image never changes and every task starts from the same toolchain.
- **Don't let the guest report its own result.** The agent has `sudo`; it could replace `diff` or `git` inside
  the guest. The host reads the changed files itself and Rust makes the diff.
- **Build output stays out of the watched folder.** `CARGO_TARGET_DIR=/tmp/target`: hundreds of compiler files
  would otherwise flow through the work folder and its change list.
- **Ids again.** In a by-hand test the `changes` answer arrived before the `exec` result that was sent first;
  the ids kept them apart.

## E · The way forward

The agent works, its result is reviewable, and the original is safe. But a real agent sometimes needs the network:
to install a tool, to fetch a crate. The moment we give the machine a network, the build script has one too.
Lesson 5 puts Rust in charge of every connection.

## F · Every important line

- `make-toolbox.sh`, `--data toolbox.img`: the CLI's persistent data disk; Alpine keeps the installed packages on
  it.
- `bridge.mjs`, `treeOf(crate)`: the crate as a plain object tree, `target` left out, the files' bytes as values.
- `bridge.mjs`, `work.onChange(...)`: called for every change the guest makes; a rename records both paths.
- `bridge.mjs`, `openDisk(toolbox, { snapshot: true })`: the toolchain, never written.
- `bridge.mjs`, `{ fs: nodeFs(crate), readOnly: true }`: the original, visible and refused.
- `bridge.mjs`, the `changes` answer: directories left out, removed files as `content: null`.
- `sandbox.rs`, `pub struct Changed { path, content: Option<String> }`: `None` means removed.
- `review.rs`, `TextDiff::from_lines(before, after).unified_diff().context_radius(3).header(&a, &b)`: the usual
  `--- a/` `+++ b/` format, three lines of context.
- `review.rs`, `before == after` and the both-missing case: an editor's temporary file, made and removed, leaves
  no trace in the patch.
- `agent.rs`, `return Ok(self.exec)`: the agent gives the sandbox back when it is done.

## G · Experiment

Predict: can anything in the guest write to the original, as `user` or as root? Does a second task see what the
first one left in the machine?

Two tasks in a row, each writing to `/src` as user and as root, then making `~/marker`
(`captures/lesson04/experiment.txt`):

```text
--- task 1
ls: /home/user/marker: No such file or directory
made /home/user/marker
sh: can't create /src/src/lib.rs: Read-only file system
sh: can't create /src/src/lib.rs: Read-only file system
--- task 2
ls: /home/user/marker: No such file or directory
made /home/user/marker
sh: can't create /src/src/lib.rs: Read-only file system
sh: can't create /src/src/lib.rs: Read-only file system
(real crate clean)
```

Root is refused too, and task 2 starts without task 1's marker.

## H · Common mistakes

- "The agent must edit, so mount the folder writable." Then the agent and every build script can change your real
  files. Give it a copy, and read the copy.
- "Ask the guest for `git diff`." Output made inside the sandbox can be faked by whatever runs there.
- "Install the toolchain in every task." Measured in the spike: 24 s each time. The toolbox costs 34 s once.
- "Build output in the shared folder." Slow, and it floods the change list.

## I · Challenge

Ask the agent (change `TASK`) to also add a test in a new file, `tests/more.rs`. What does the diff show for it?
Expected: a diff from nothing, `--- a/tests/more.rs` with every line added, because `review::diff` treats a file
missing from the real crate as empty.
