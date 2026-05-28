import { existsSync, readdirSync, statSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const __dirname = dirname(fileURLToPath(import.meta.url))
const appRoot = resolve(__dirname, '..')
const resourceRoot = join(appRoot, 'resources', 'pdf2zh-sidecar')
const runtimeRoot = join(resourceRoot, 'python')
const shouldSign = process.argv.includes('--sign')
const identity = process.env.LUMENFOLIO_PDF2ZH_CODESIGN_IDENTITY || '-'

function fail(message) {
  console.error(message)
  process.exit(1)
}

function walkFiles(root) {
  const output = []
  if (!existsSync(root)) return output
  const stack = [root]
  while (stack.length > 0) {
    const current = stack.pop()
    for (const entry of readdirSync(current)) {
      const path = join(current, entry)
      const stat = statSync(path)
      if (stat.isDirectory()) {
        stack.push(path)
      } else if (stat.isFile()) {
        output.push(path)
      }
    }
  }
  return output.sort()
}

function isMachO(path) {
  try {
    const output = execFileSync('file', [path], { encoding: 'utf8' })
    return output.includes('Mach-O')
  } catch {
    return false
  }
}

function verifyCodeSignature(path) {
  try {
    execFileSync('codesign', ['--verify', '--strict', path], { encoding: 'utf8', stdio: 'pipe' })
    return ''
  } catch (error) {
    return String(error.stderr || error.stdout || error.message || '').trim()
  }
}

function signCode(path) {
  const args = ['--force', '--sign', identity]
  if (identity === '-') args.push('--timestamp=none')
  args.push(path)
  execFileSync('codesign', args, { encoding: 'utf8', stdio: 'pipe' })
}

if (process.platform !== 'darwin') {
  console.log(JSON.stringify({
    ok: true,
    skipped: true,
    reason: 'macOS codesign is only available on darwin',
  }, null, 2))
  process.exit(0)
}

if (!existsSync(runtimeRoot)) {
  fail(`PDF2ZH macOS runtime missing: ${runtimeRoot}`)
}

const candidates = [
  ...walkFiles(runtimeRoot),
  join(resourceRoot, 'pdf2zh-sidecar'),
].filter((path) => existsSync(path) && isMachO(path))

if (shouldSign) {
  for (const path of candidates) {
    signCode(path)
  }
}

const failures = []
for (const path of candidates) {
  const error = verifyCodeSignature(path)
  if (error) failures.push({ path, error })
}

const result = {
  ok: failures.length === 0,
  signed: shouldSign,
  identity,
  runtimeRoot,
  machOFiles: candidates.length,
  failures: failures.slice(0, 20),
}

console.log(JSON.stringify(result, null, 2))
if (failures.length > 0) process.exit(1)
