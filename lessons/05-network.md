# Lesson 5 · Network: Rust answers every DNS question

Code: `bridge/bridge.mjs` (a `net` backend that asks), `src/policy.rs` (new), `src/sandbox.rs` (answers questions
while it waits), `src/main.rs` (`--allow-host`, and `sh`: one command in the sandbox, no agent), `src/agent.rs`
(the instructions mention `sudo apk add`).

```console
$ cargo run -- sh 'wget -q -T 5 -O- http://example.com | head -c 60'                       # refused
$ cargo run -- sh 'wget -q -T 5 -O- http://example.com | head -c 60' --allow-host example.com
$ cargo run -- fix fixtures/tinycalc --allow-host dl-cdn.alpinelinux.org
```

Lesson 4's agent works, offline. Real agents sometimes need the network: a package, a crate, documentation. But a
machine with a network gives it to every program inside, the build script included. This lesson gives the
machine a network in which every name lookup is a question to Rust.

## A · The problem

How do we give the guest some network without giving it all? Blocking by name alone is not enough: a program can
skip DNS and connect to an address directly. And the decision should be ours, in Rust, at the moment it happens,
written down where we can see it.

## B · The code

**The bridge's network** (`bridge.mjs`), passed to `createKernel({ net })`. unkernel's network stack hands the
guest's DNS lookups and TCP connections to an object with two methods:

```js
const net = {
  async lookup(name) {
    const { allow } = await ask({ type: 'dns', name });          // a question to Rust
    if (!allow) throw Object.assign(new Error('refused'), { code: 'ENOTFOUND' });
    const addresses = await internet.lookup(name);                // directBackend(): the Mac's resolver
    addresses.forEach((address) => allowedAddresses.add(address));
    return addresses;
  },
  connect(target, events) {
    if (allowedAddresses.has(target.host)) return internet.connect(target, events);
    send({ type: 'refused', host: target.host, port: target.port });  // a report to Rust
    ...                                                                // and the connection closes
  },
};
```

`ask` sends `{"type":"dns","question":1,"name":"..."}` and waits for `{"type":"answer","question":1,"allow":...}`.

**The policy** (`policy.rs`): a list of exact host names; empty by default.

**The sandbox** (`sandbox.rs`): `answer(id)` reads until the answer to its own request arrives, and on the way
answers every `dns` question with `Policy::allows` and prints every decision:

```rust
FromBridge::Dns { question, name } => {
    let allow = self.policy.allows(&name);
    println!("          [net] lookup {name}: {}", if allow { "allowed" } else { "refused" });
    self.send(&ToBridge::Answer { question, allow }).await?;
}
```

The real agent run, with only Alpine's package mirror allowed (`captures/lesson05/run1.txt`, gpt-5.4-mini,
23 Nov 2026; long outputs cut):

```text
sandbox ready in 1336 ms
step  1  $ pwd && ls -la && cargo test --quiet
          [net] lookup collect.localtest.me: refused
          /work
          ...
          tests::mean_keeps_the_fraction --- FAILED
step  2  $ sed -n '1,200p' src/lib.rs && ... && sed -n '1,200p' build.rs
step  3  $ python3 - <<'PY' ... PY
          cargo test --quiet
          tests::mean_keeps_the_fraction --- FAILED
step  4  $ perl -0pi -e '...' src/lib.rs
          cargo test --quiet
          tests::mean_keeps_the_fraction --- FAILED
step  5  $ cat > src/lib.rs <<'EOF' ... EOF
          cargo test --quiet
          [net] lookup collect.localtest.me: refused
          test result: ok. 3 passed; 0 failed; ...
done: Fixed mean to compute with floating-point division so fractional averages are preserved, and confirmed
all cargo tests pass.

the agent's changes (target/fix-1795414572.diff), not applied:
-    Some((sum / xs.len() as i64) as f64)
+    Some(sum as f64 / xs.len() as f64)
```

There is the build script, caught in the act: during the agent's very first command, and again when cargo ran it
after the fix. The same two moments as Lesson 1's two canary hits; this time Rust said no both times, and the
canary stayed silent. The agent did not need the mirror in this run (no `apk add`), so no lookup was allowed.

## C · The picture: one lookup

```text
 guest:  build.rs ─ getaddrinfo("collect.localtest.me")
           ─▶ the guest kernel sends a DNS query ─▶ unkernel's network stack (JS) ─▶ net.lookup(name)
 bridge:   {"type":"dns","question":1,"name":"collect.localtest.me"}  ─────────────────────────▶ Rust
                                                                     Policy::allows → false
 bridge:   ◀──────────────────────────────── {"type":"answer","question":1,"allow":false}
           ENOTFOUND ─▶ in the guest: the name does not exist, the build script returns quietly

 guest:  connect(1.1.1.1:80) ─▶ net.connect ─▶ no allowed lookup returned 1.1.1.1 ─▶ refused, reported
```

The question arrives while Rust is waiting for the result of `cargo test`. That is why the bridge does not await
commands (Lesson 3), and why `answer(id)` handles questions while it waits.

## D · The rules behind it

- **Every packet leaves through our functions.** The guest runs its own TCP/IP stack, but its network device ends
  in the host, at the `net` object we pass. No `net`, no network (Lessons 3 and 4); our `net`, our rules.
- **Allow by name, enforce by address.** A connection is allowed only to an address that an allowed lookup
  returned. So a raw IP is refused, as the experiment shows.
- **Exact names, default deny.** `example.com` does not allow `collect.example.com`, and an empty list allows
  nothing.
- **Decisions in Rust, enforcement in the bridge.** The bridge only asks and obeys; the answer, and the log line,
  come from Rust.
- **Honest limits.** An allowed host is fully reachable, so data could still be sent to it (in a URL, a request
  body): allow only hosts you trust with what is in the machine. And we filter by address: one CDN address may
  serve many sites.

## E · The way forward

Files, tools and network are now handed in on purpose. What is left is time and size: a command that never ends, an
output that floods the model, an agent that never stops. Lesson 6.

## F · Every important line

- `bridge.mjs`, `ask(message)`: a `Promise` stored under a question number; the `answer` handler in the stdin loop
  resolves it.
- `bridge.mjs`, `throw ... { code: 'ENOTFOUND' }`: unkernel turns that into "no such name" for the guest.
- `bridge.mjs`, `allowedAddresses`: filled only by allowed lookups; `connect` checks it.
- `bridge.mjs`, the refused connection: a report to Rust, then `onClose` with an error, so the guest sees
  "connection refused".
- `policy.rs`, `allows`: `iter().any(|allowed| allowed == host)`, exact comparison.
- `sandbox.rs`, `answer(id)`: one loop for every kind of message; `Ready` counts as the answer to 0.
- `sandbox.rs`, `answer @ (FromBridge::Result { id: got, .. } | FromBridge::Changes { id: got, .. }) if got == id`:
  an or-pattern that binds the same `got` in both variants, a guard, and `@` to keep the whole value.
- `main.rs`, `#[arg(long = "allow-host")] allow_hosts: Vec<String>`: repeat the flag to allow several hosts.
- `main.rs`, `sh`: `sandbox.run(&command)`, the same `Executor` method the agent uses.

## G · Experiment

Three predictions, three runs (`captures/lesson05/experiments.txt`):

```text
$ agent-sandbox sh "wget -q -T 5 -O- http://example.com | head -c 60"
          [net] lookup example.com: refused
wget: bad address 'example.com'

$ agent-sandbox sh "...example.com...; ...1.1.1.1..." --allow-host example.com
          [net] lookup example.com: allowed
          [net] connect 1.1.1.1:80: refused
<!doctype html><html lang=en><head><meta charset=utf-8><link
wget: can't connect to remote host (1.1.1.1): Connection refused

$ agent-sandbox sh "cargo build"
          [net] lookup collect.localtest.me: refused
   Compiling tinycalc v0.1.0 (/work)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.21s
```

Nothing allowed: refused at the name. `example.com` allowed: the page arrives, but `1.1.1.1`, which no allowed
lookup returned, is refused at the connection. And `cargo build` alone is enough to expose the build script.

## H · Common mistakes

- "Block bad names." Default deny and allow good ones: the list of bad names never ends.
- "Filter DNS only." A program can connect to an address directly; the raw-IP run shows why addresses are checked.
- "Allow `*.example.com`." Anyone who controls a subdomain gets through. Exact names.
- "The network is the agent's business." The build script used it, not the agent. Everything in the machine
  shares the network.

## I · Challenge

Allow the collector's own name, `--allow-host collect.localtest.me`, and build again. Does the canary on the Mac
get the token now? The real run (`captures/lesson05/challenge_allow_collector.txt`):

```text
          [net] lookup collect.localtest.me: allowed
          [net] lookup collect.localtest.me: allowed
          [net] lookup collect.localtest.me: allowed
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.20s

wget: can't connect to remote host (127.0.0.1): Connection refused
```

Allowed, and still nothing reached the Mac: the name points at 127.0.0.1, which inside the guest is the guest
itself (Lesson 2). A real collector would be a real server, and an allowed name would reach it. That is why the
list stays short.
