import { spawnSync } from 'child_process';

const STEPS = [
  ['wasm', 'yarn check:wasm'],
  ['commands', 'yarn check:commands'],
  ['format', 'yarn format:check'],
  ['types', 'yarn typecheck'],
  ['lint', 'yarn lint'],
  ['cycles', 'yarn circular-deps'],
  ['test', 'yarn vitest run'],
  ['clippy', 'yarn lint:rust'],
  ['rust', 'yarn test:rust'],
  ['dead', 'yarn dead-code'],
  ['dup', 'yarn duplication']
];

// Only the failing step's output is worth reading, and only its tail.
const TAIL_LINES = 30;

const green = (text) => `[32m${text}[0m`;
const red = (text) => `[31m${text}[0m`;
const dim = (text) => `[90m${text}[0m`;

const started = Date.now();

for (const [label, command] of STEPS) {
  const at = Date.now();
  const run = spawnSync(command, { shell: true, encoding: 'utf8' });
  const took = `${((Date.now() - at) / 1000).toFixed(1)}s`;

  if (run.status === 0) {
    console.log(`${green('✓')} ${label.padEnd(9)} ${dim(took)}`);
    continue;
  }

  console.log(`${red('✗')} ${label.padEnd(9)} ${dim(took)}`);
  const output = `${run.stdout ?? ''}${run.stderr ?? ''}`
    .split('\n')
    .filter((line) => line.trim() && !/^(yarn run|\$ |info Visit|Done in )/.test(line))
    .slice(-TAIL_LINES)
    .join('\n');
  console.log(`\n${output}\n`);
  console.log(red(`failed: ${command}`));
  process.exit(1);
}

console.log(green(`all green ${dim(`${((Date.now() - started) / 1000).toFixed(0)}s`)}`));
