// Fails the lint gate when a component reaches into another's styles with
// `:deep()`, `::v-deep` or `/deep/`. ESLint does not parse `<style>` blocks, so
// this scans them the way `no-section-comments.mjs` scans tracked sources.
import { execSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';

const DEEP = /:deep\(|::v-deep|\/deep\//;

const components = execSync('git ls-files --cached --others --exclude-standard', {
  encoding: 'utf8'
})
  .split('\n')
  .filter((path) => path.endsWith('.vue'))
  .filter((path) => existsSync(path));

const offenders = components.flatMap((path) => {
  const source = readFileSync(path, 'utf8');
  const styleStart = source.indexOf('<style');
  if (styleStart < 0) return [];
  return source
    .slice(styleStart)
    .split('\n')
    .flatMap((line, index) =>
      DEEP.test(line) ? [`${path}:${source.slice(0, styleStart).split('\n').length + index}`] : []
    );
});

if (offenders.length) {
  console.error('Deep selectors are banned: style the element in the component that renders it.');
  console.error(offenders.map((offender) => `  ${offender}`).join('\n'));
  process.exit(1);
}
