// The bridge: runs unkernel for the Rust program, because unkernel's SDK is
// a JavaScript library. It decides nothing; it does what Rust asks.
//
//   node --experimental-wasm-jspi bridge.mjs <crate folder> <toolbox.img>
//
// One JSON object per line. Rust -> bridge (stdin):
//   {"type":"exec","id":1,"command":"ls"}
//   {"type":"changes","id":2}
// bridge -> Rust (stdout): {"type":"ready"} once the machine is up, then
//   {"type":"result","id":1,"exit_code":0,"output":"..."}
//   {"type":"changes","id":2,"files":[{"path":"src/lib.rs","content":"..."}]}
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import {
  MemoryFs,
  createKernel,
  fetchDistro,
  nodeFs,
  openDisk,
} from 'unkernel';

// stdout carries protocol lines only: anything else goes to stderr.
console.log = console.error;
const send = (message) => process.stdout.write(JSON.stringify(message) + '\n');

const [crate, toolbox] = process.argv.slice(2);

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
  memoryMiB: 1024,
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
    // Not awaited: the bridge keeps reading while the command runs.
    vm.exec('sh', ['-c', message.command], {
      cwd: '/work',
      // build output stays in the guest
      env: { CARGO_TARGET_DIR: '/tmp/target' },
    }).then(
      (r) =>
        send({
          type: 'result',
          id: message.id,
          exit_code: r.exitCode,
          output: r.stdout + r.stderr,
        }),
      (e) =>
        send({
          type: 'result',
          id: message.id,
          exit_code: null,
          output: `bridge: ${e.message}`,
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
  }
}

// stdin closed: Rust is done with us. Power the machine off.
await vm.exec('sudo', ['poweroff']).catch(() => {});
await vm.exited;
process.exit(0);
