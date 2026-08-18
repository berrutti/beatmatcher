// Fails the lint gate when a section-divider comment (// ── ...) exists in any
// tracked source file. Scans `git ls-files`, so ignored and generated files are
// excluded automatically and only what would be committed is checked.
import { execSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { extname } from 'node:path';

const BINARY_EXTENSIONS = new Set([
  '.png',
  '.jpg',
  '.jpeg',
  '.gif',
  '.webp',
  '.ico',
  '.icns',
  '.svg',
  '.woff',
  '.woff2',
  '.ttf',
  '.otf',
  '.eot',
  '.pdf',
  '.zip',
  '.gz',
  '.tar',
  '.7z',
  '.mp3',
  '.mp4',
  '.mov',
  '.wav',
  '.flac',
  '.wasm',
  '.lock'
]);

const DIVIDER = /^\s*(\/\/|<!--|#)\s*(─|-{3,})/;

// Untracked files are included and deleted-but-unstaged ones skipped, or a new file
// escapes the check entirely and a removed one crashes it.
const trackedFiles = execSync('git ls-files --cached --others --exclude-standard', {
  encoding: 'utf8'
})
  .split('\n')
  .filter((path) => path.length > 0 && !BINARY_EXTENSIONS.has(extname(path)))
  .filter((path) => existsSync(path));

const offenders = trackedFiles.flatMap((path) =>
  readFileSync(path, 'utf8')
    .split('\n')
    .flatMap((line, index) => (DIVIDER.test(line) ? [`${path}:${index + 1}`] : []))
);

if (offenders.length) {
  console.error('Section-divider comments are banned:');
  console.error(offenders.map((offender) => `  ${offender}`).join('\n'));
  process.exit(1);
}
