import { createHash } from 'crypto';
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from 'fs';
import { join } from 'path';

const MANIFEST = 'session-core/Cargo.toml';
const SOURCE_DIR = 'session-core/src';
const STAMP = 'session-core/pkg/.source-hash';

function rustSources(dir) {
  return readdirSync(dir)
    .sort()
    .flatMap((entry) => {
      const path = join(dir, entry);
      if (statSync(path).isDirectory()) return rustSources(path);
      return path.endsWith('.rs') ? [path] : [];
    });
}

function sourceHash() {
  const hash = createHash('sha256');
  for (const file of [MANIFEST, ...rustSources(SOURCE_DIR)]) {
    hash.update(file);
    hash.update(readFileSync(file));
  }
  return hash.digest('hex');
}

const current = sourceHash();

if (process.argv.includes('--write')) {
  writeFileSync(STAMP, `${current}\n`);
  process.exit(0);
}

function stale(reason) {
  console.error(`session-core/pkg is stale: ${reason}`);
  console.error('The frontend and its tests run against the built wasm, not the Rust source,');
  console.error('so a change to session-core is invisible here until it is rebuilt.');
  console.error('Run: yarn build:wasm');
  process.exit(1);
}

if (!existsSync(STAMP)) stale('never built, or built before this check existed');
if (readFileSync(STAMP, 'utf8').trim() !== current) stale('built from different sources');
