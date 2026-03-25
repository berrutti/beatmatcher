import { execSync } from 'child_process'
import { readFileSync, writeFileSync } from 'fs'

const bump = (process.argv[2] ?? 'patch') as 'major' | 'minor' | 'patch'

if (!['major', 'minor', 'patch'].includes(bump)) {
  console.error('Usage: tsx scripts/release.ts [patch|minor|major]')
  process.exit(1)
}

const pkg = JSON.parse(readFileSync('package.json', 'utf8'))
const [major, minor, patch] = pkg.version.split('.').map(Number)

const next = {
  major: `${major + 1}.0.0`,
  minor: `${major}.${minor + 1}.0`,
  patch: `${major}.${minor}.${patch + 1}`
}[bump]

const tag = `v${next}`
console.log(`Bumping ${pkg.version} → ${next}`)

pkg.version = next
writeFileSync('package.json', JSON.stringify(pkg, null, 2) + '\n')

const run = (cmd: string) => execSync(cmd, { stdio: 'inherit' })

run(`git add package.json`)
run(`git commit -m "chore: release ${tag}"`)
run(`git tag ${tag}`)
run(`git push origin main ${tag}`)

console.log(`Released ${tag}`)
