// The bridge: runs unkernel for the Rust program, because unkernel's SDK is
// a JavaScript library. It decides nothing; it does what Rust asks.
//
// One JSON object per line. Rust -> bridge (stdin):
//   {"type":"exec","id":1,"command":"ls"}
// bridge -> Rust (stdout): {"type":"ready"} once the machine is up, then
//   {"type":"result","id":1,"exit_code":0,"output":"..."}
import { createInterface } from 'node:readline';
import { createKernel, fetchDistro } from 'unkernel';

// stdout carries protocol lines only: anything else goes to stderr.
console.log = console.error;
const send = (message) => process.stdout.write(JSON.stringify(message) + '\n');

const vm = await createKernel({ distro: await fetchDistro('alpine') });
send({ type: 'ready' });

for await (const line of createInterface({ input: process.stdin })) {
  const message = JSON.parse(line);
  if (message.type === 'exec') {
    // Not awaited: the bridge keeps reading while the command runs.
    vm.exec('sh', ['-c', message.command]).then(
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
  }
}

// stdin closed: Rust is done with us. Power the machine off.
await vm.exec('sudo', ['poweroff']).catch(() => {});
await vm.exited;
process.exit(0);
