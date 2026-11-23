// The bridge: runs unkernel for the Rust program, because unkernel's SDK is
// a JavaScript library. It decides nothing; it does what Rust asks.
//
//   node --experimental-wasm-jspi bridge.mjs \
//        <crate folder> <toolbox.img> <memory MiB>
//
// One JSON object per line. Rust -> bridge (stdin):
//   {"type":"exec","id":1,"command":"ls","timeout_ms":60000,
//    "max_output":1048576}
//   {"type":"changes","id":2}
//   {"type":"answer","question":1,"allow":false}
// bridge -> Rust (stdout): {"type":"ready"} once the machine is up, then
//   {"type":"result","id":1,"exit_code":0,"output":"..."}
//   {"type":"changes","id":2,"files":[{"path":"src/lib.rs","content":"..."}]}
//   {"type":"dns","question":1,"name":"example.com"}   (a question)
//   {"type":"refused","host":"1.2.3.4","port":80}      (a report)
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import {
  MemoryFs,
  createKernel,
  directBackend,
  fetchDistro,
  nodeFs,
  openDisk,
} from 'unkernel';

// stdout carries protocol lines only: anything else goes to stderr.
console.log = console.error;
const send = (message) => process.stdout.write(JSON.stringify(message) + '\n');

// Questions to Rust: each waits for the answer with its number.
const waiting = new Map();
let lastQuestion = 0;
const ask = (message) =>
  new Promise((resolve) => {
    const question = ++lastQuestion;
    waiting.set(question, resolve);
    send({ ...message, question });
  });

// The guest's network. Every DNS lookup is a question to Rust; a TCP
// connection is allowed only to an address an allowed lookup returned.
const internet = directBackend();
const allowedAddresses = new Set();
const net = {
  async lookup(name) {
    const { allow } = await ask({ type: 'dns', name });
    if (!allow)
      throw Object.assign(new Error('refused'), { code: 'ENOTFOUND' });
    const addresses = await internet.lookup(name);
    addresses.forEach((address) => allowedAddresses.add(address));
    return addresses;
  },
  connect(target, events) {
    if (allowedAddresses.has(target.host))
      return internet.connect(target, events);
    send({ type: 'refused', host: target.host, port: target.port });
    queueMicrotask(() => events.onClose(new Error('refused by policy')));
    return {
      write() {},
      end() {},
      destroy() {},
      pause() {},
      resume() {},
      buffered: 0,
    };
  },
  close() {
    internet.close?.();
  },
};

const [crate, toolbox, memoryMiB] = process.argv.slice(2);

// The agent's work folder: an in-memory copy of the crate (without build
// output). We see every change the guest makes to it.
const treeOf = (dir) =>
  Object.fromEntries(
    readdirSync(dir, { withFileTypes: true })
      .filter((entry) => entry.name !== 'target')
      .map((entry) => [
        entry.name,
        entry.isDirectory()
          ? treeOf(join(dir, entry.name))
          : readFileSync(join(dir, entry.name)),
      ]),
  );
const work = new MemoryFs(treeOf(crate));
const changed = new Set();
work.onChange((change) => {
  changed.add(change.path);
  if (change.to) changed.add(change.to);
});

const vm = await createKernel({
  distro: await fetchDistro('alpine'),
  memoryMiB: Number(memoryMiB), // all the memory the guest will ever have
  net,
  // rust and cargo, made once; writes go to memory and vanish at exit
  disks: [await openDisk(toolbox, { snapshot: true })],
  mounts: {
    // the original: look, don't touch
    '/src': { fs: nodeFs(crate), readOnly: true },
    // the copy the agent changes
    '/work': work,
  },
});
send({ type: 'ready' });

for await (const line of createInterface({ input: process.stdin })) {
  const message = JSON.parse(line);
  if (message.type === 'exec') {
    const { id, command, timeout_ms, max_output } = message;
    // Not awaited: the bridge keeps reading while the command runs.
    vm.exec('sh', ['-c', command], {
      cwd: '/work',
      // build output stays in the guest
      env: { CARGO_TARGET_DIR: '/tmp/target' },
      signal: AbortSignal.timeout(timeout_ms), // too long: SIGKILL
      maxBuffer: max_output, // too much output: SIGKILL
    }).then(
      (r) =>
        send({
          type: 'result',
          id,
          exit_code: r.exitCode,
          output: r.stdout + r.stderr,
        }),
      (e) =>
        send({
          type: 'result',
          id,
          exit_code: null,
          output:
            e.code === 'ABORTED'
              ? `killed: still running after ${timeout_ms / 1000} s\n`
              : e.code === 'ENOBUFS'
                ? `killed: printed more than ${max_output} bytes\n`
                : `bridge: ${e.message}\n`,
        }),
    );
  } else if (message.type === 'changes') {
    // Each changed path with its text now, or null if it no longer exists.
    const files = [...changed]
      .sort()
      .filter((path) => !work.exists(path) || work.stat(path).type !== 'dir')
      .map((path) => ({
        path: path.replace(/^\//, ''),
        content: work.exists(path) ? work.readText(path) : null,
      }));
    send({ type: 'changes', id: message.id, files });
  } else if (message.type === 'answer') {
    waiting.get(message.question)?.(message);
    waiting.delete(message.question);
  }
}

// stdin closed: Rust is done with us. Power the machine off.
await vm.exec('sudo', ['poweroff']).catch(() => {});
await vm.exited;
process.exit(0);
