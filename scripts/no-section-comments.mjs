// Fails the lint gate when a section-divider comment (// ── ...) exists in any
// source file, across TypeScript, Vue, and Rust
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

const ROOTS = ['src/renderer/src', 'src-tauri/src', 'session-core/src', 'session-core/tests'];
const EXTENSIONS = ['.ts', '.vue', '.rs'];
const DIVIDER = /^\s*(\/\/|<!--)\s*─/;

const offenders = [];

function walk(dir) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) {
      walk(path);
      continue;
    }
    if (!EXTENSIONS.some((extension) => path.endsWith(extension))) continue;
    const lines = readFileSync(path, 'utf8').split('\n');
    lines.forEach((line, index) => {
      if (DIVIDER.test(line)) offenders.push(`${path}:${index + 1}`);
    });
  }
}

for (const root of ROOTS) walk(root);

if (offenders.length > 0) {
  console.error('Section-divider comments are banned:');
  for (const offender of offenders) console.error(`  ${offender}`);
  process.exit(1);
}
