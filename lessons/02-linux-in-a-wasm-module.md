# Lesson 2 · A Linux machine inside a WebAssembly module

Code: `bridge/package.json` (unkernel pinned to `0.2.0-261122-4e58555`), `bridge/hello.mjs`.

```console
$ cd bridge && npm install
$ node --experimental-wasm-jspi node_modules/.bin/unkernel     # an Alpine shell; Ctrl-] quits
$ node --experimental-wasm-jspi hello.mjs                       # boot, probe, time, power off
```

Lesson 1 ended with a command that needs somewhere else to run: a place where our files, our environment and our
network are simply not there. This lesson meets that place: [unkernel](https://github.com/pithings/unkernel), a
real Linux kernel compiled to WebAssembly, running inside our own process.

## A · The problem

Where can an agent's command run, if not on our Mac? A machine in the cloud (slow to start, costs money, our code
leaves the machine), a container (shares our machine's kernel), a full virtual machine (heavy). We want something
that starts in a blink, needs nothing installed beyond Node, and lets our program decide every way in and out.

## B · Meeting it

The README says `npx unkernel`. On Node 24.19 that fails, for real:

```text
[host] boot failed: BootError: This engine has no JSPI (WebAssembly.Suspending / WebAssembly.promising),
which the kernel needs
```

JSPI is a WebAssembly feature (JavaScript Promise Integration) that the kernel uses to switch between its tasks;
Node 24 still keeps it behind a flag. With the flag:

```text
$ node --experimental-wasm-jspi node_modules/.bin/unkernel
Welcome to Alpine Linux v3.24 on unkernel (aarch64)
~ ⇒ uname -a; cat /etc/alpine-release; id
Linux unkernel 7.3.4+ #254cb70ca319 Sun Nov 22 10:04:18 UTC 2026 aarch64 Linux
3.24.4
uid=1000(user) gid=1000(user) groups=10(wheel),18(audio),23(input),27(video),100(users),1000(user)
~ ⇒ poweroff
[    4.293956] reboot: Power down
```

(`captures/lesson02/cli_boot.raw`.) A real Alpine Linux, kernel 7.3.4, on an aarch64 CPU, inside a Node process
on a Mac. The same thing from code is `hello.mjs`: `fetchDistro('alpine')` (the 3.9 MiB Alpine image, downloaded
once and cached), `createKernel({ distro })`, then `vm.exec(...)` for each command.

## C · What is inside

```text
 node, our process
 └─ unkernel.wasm: ONE WebAssembly module
    ├─ the Linux kernel, compiled to wasm: scheduler, memory management, file systems, TCP/IP
    └─ kwemu: an aarch64 CPU written in Zig, linked into the kernel
       └─ Alpine's programs (sh, wget, cargo ...): ordinary aarch64 Linux binaries, unmodified
 devices: small drivers inside the kernel that call JavaScript functions
          (the disk with the Alpine image, the console, and only what we add: folders, a network)
```

Only the programs are emulated: kwemu runs their instructions. When a program makes a system call, kwemu returns
into the kernel, which handles it as compiled WebAssembly, the same Linux code a server runs. kwemu also walks the
kernel's own page tables, so `fork`, `mmap` and copy-on-write behave as on real hardware. (Details: the project's
README and `.agents/` notes.)

## D · Why that makes a sandbox

- **A WebAssembly module can touch only its own memory and the functions its host imports into it.** It cannot
  open our files, read our environment or open a socket on its own.
- **In unkernel those imports are the devices**, and our program chooses them. No `mounts`: no folder of ours.
  No `net`: no network (the README: networking is off unless you pass `net`). The guest's environment is the
  guest's own.
- **It has its own kernel.** A container's processes are processes of the host's kernel, with restrictions.
  Here every system call of the agent's command is answered by a separate Linux kernel inside the module.

So the boundary is exactly what our program hands in. That is also the honest limit: a writable mount, a
permissive network, or a bug in our own JavaScript opens it again.

## E · The way forward

A Linux that starts in a fifth of a second, with nothing of ours inside: a good place for the agent's commands.
Lesson 3 makes Rust drive it.

## F · Every important line (`hello.mjs`)

- `import { createKernel, fetchDistro } from 'unkernel'`: the SDK, a library with no dependencies (MIT; the
  kernel inside it is GPL-2.0).
- `fetchDistro('alpine')`: the path of the Alpine image, downloaded from the package's GitHub release the first
  time, checked, and cached in `~/.cache/unkernel`.
- `createKernel({ distro })`: instantiates the module and boots. Nothing else is passed: no mounts, no network.
- `vm.exec('sh', ['-c', command])`: runs a program in the guest and resolves with `{ stdout, stderr, exitCode,
  signal }`. It runs as the guest's `user`, in `/home/user`.
- `vm.exec('sudo', ['poweroff'])`, then `await vm.exited`: a clean shutdown, `{ code: 0, kind: "halt" }`.

## G · Experiment

Predict, before running: can a command in the guest see our Mac's folders, the credentials file from Lesson 1,
our environment, the collector?

The real run (`captures/lesson02/hello_run3.txt`; three runs gave the same picture):

```text
alpine image ready       at 1185 ms
kernel created           at 1258 ms
first command done       207 ms after createKernel

$ uname -sm    (3 ms)
Linux aarch64

$ ls /Users 2>&1    (3 ms)
ls: /Users: No such file or directory

$ cat ~/.cargo/credentials.toml 2>&1    (3 ms)
cat: can't open '/home/user/.cargo/credentials.toml': No such file or directory

$ env | sort    (5 ms)
HOME=/home/user
LOGNAME=user
PATH=/run/kw/bin:/home/user/.local/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/usr/lib/kw/bin
PWD=/home/user
SHELL=/bin/sh
SHLVL=1
USER=user

$ wget -q -T 3 -O- http://collect.localtest.me:7878 2>&1
wget: bad address 'collect.localtest.me:7878'
```

None of it. The boot itself: `createKernel` about 70 ms, the first command done about 200 ms after it, every
further command a few milliseconds. (Finding the cached image, `fetchDistro`, took about 1.2 s of its own.)

## H · Common mistakes

- "An emulator is slow." The kernel is compiled code, and a command like `ls` takes a few milliseconds. Heavy
  computation in the guest is slower than native (Lesson 6 measures `cargo test`).
- "It runs my Mac's programs." It runs aarch64 Linux programs: Alpine's packages, or a static Linux binary.
- "A sandbox protects against everything." It protects exactly as far as what we hand in. Lessons 4 to 6 hand
  things in, one at a time, on purpose.
- "`npx unkernel` just works on Node 24." Node 24.19 needs `--experimental-wasm-jspi`.

## I · Challenge

Give the guest a network, `createKernel({ distro, net: directBackend() })`, keep the canary running on the Mac,
and run the `wget` probe again. Predict what happens. The real result (`captures/lesson02/challenge_direct_net.txt`):

```text
$ nslookup collect.localtest.me 2>&1 | tail -2
Address: 127.0.0.1
$ wget -q -T 5 -O- http://collect.localtest.me:7878 2>&1; echo "exit=$?"
wget: can't connect to remote host (127.0.0.1): Connection refused
$ wget -q -T 5 -O- http://example.com 2>&1 | head -c 120
<!doctype html><html lang=en><head><meta charset=utf-8> ...
```

The name now resolves, but 127.0.0.1 inside the guest is the guest itself, so our demo collector on the Mac
stays silent. A real collector would be a real server, and `example.com` shows the guest can reach real servers
once it has a network. Who decides which ones is Lesson 5.
