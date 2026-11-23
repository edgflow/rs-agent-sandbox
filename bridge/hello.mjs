// The smallest program that boots Linux inside this process and runs
// commands in it, timing each step. Lesson 2 of agent-sandbox.
//
//   node --experimental-wasm-jspi hello.mjs
import { createKernel, fetchDistro } from 'unkernel';

const start = performance.now();
const at = () => `${Math.round(performance.now() - start)} ms`;

const distro = await fetchDistro('alpine'); // downloaded once, then cached
console.log(`alpine image ready       at ${at()}`);
const boot = performance.now();
const vm = await createKernel({ distro });
console.log(`kernel created           at ${at()}`);
const first = await vm.exec('true', []);
const booted = Math.round(performance.now() - boot);
console.log(`first command done       ${booted} ms after createKernel`);

// What can a command in here see of ours?
const probes = [
  'uname -sm',
  'ls /Users 2>&1',
  'cat ~/.cargo/credentials.toml 2>&1',
  'env | sort',
  'wget -q -T 3 -O- http://collect.localtest.me:7878 2>&1',
];
for (const command of probes) {
  const t0 = performance.now();
  const r = await vm.exec('sh', ['-c', command]);
  const took = Math.round(performance.now() - t0);
  console.log(`\n$ ${command}    (${took} ms, done at ${at()})`);
  console.log(r.stdout.trimEnd());
}

await vm.exec('sudo', ['poweroff']).catch(() => {});
console.log(`\nexit: ${JSON.stringify(await vm.exited)}`);
