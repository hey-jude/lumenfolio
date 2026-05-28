import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'

const distDir = new URL('../dist', import.meta.url).pathname
const forbidden = [
  'data-testid',
  'harness=translation-linking',
  'TranslationLinkingHarness',
  'tauriCoreMock',
  'E2E Translation Linking',
  'p1-b1',
  'p2-b1',
  'p1-caption-title',
  'requested-translation-pages',
]

function walk(dir) {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry)
    return statSync(path).isDirectory() ? walk(path) : [path]
  })
}

const offenders = []
for (const file of walk(distDir)) {
  if (!/\.(html|js|css)$/.test(file)) continue
  const content = readFileSync(file, 'utf8')
  for (const token of forbidden) {
    if (content.includes(token)) offenders.push(`${file}: ${token}`)
  }
}

if (offenders.length) {
  console.error(`Production bundle contains e2e test attributes:\n${offenders.join('\n')}`)
  process.exit(1)
}
