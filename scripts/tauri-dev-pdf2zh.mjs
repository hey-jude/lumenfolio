import { existsSync, mkdirSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawn, spawnSync } from 'node:child_process'

const __dirname = dirname(fileURLToPath(import.meta.url))
const appRoot = resolve(__dirname, '..')
const repoRoot = resolve(appRoot, '..')
const localSourceDir = join(appRoot, 'external', 'PDFMathTranslate', 'pdf2zh', 'kernel', 'PDFMathTranslate-next.git')
const legacySourceDir = join(repoRoot, 'external', 'PDFMathTranslate', 'pdf2zh', 'kernel', 'PDFMathTranslate-next.git')
const sourceDir = existsSync(localSourceDir) ? localSourceDir : legacySourceDir
const worker = join(appRoot, 'resources', 'pdf2zh-sidecar', 'worker', 'lumenfolio_pdf2zh_worker.py')
const venvPython = process.platform === 'win32'
  ? join(appRoot, '.venv-pdf2zh', 'Scripts', 'python.exe')
  : join(appRoot, '.venv-pdf2zh', 'bin', 'python')
const probeRoot = process.env.LUMENFOLIO_PDF2ZH_DEV_PROBE_DIR || join(appRoot, 'artifacts', 'pdf2zh-dev-probe')
const passthroughArgs = process.argv.slice(2).filter((arg) => arg !== '--probe-only')
const probeOnly = process.argv.includes('--probe-only')

function npmCommand() {
  return process.platform === 'win32' ? 'npm.cmd' : 'npm'
}

function runSync(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: appRoot,
    env: process.env,
    stdio: 'inherit',
    ...options,
  })
  if (result.status !== 0) {
    process.exit(result.status || 1)
  }
}

function capture(command, args, options = {}) {
  return spawnSync(command, args, {
    cwd: appRoot,
    env: process.env,
    encoding: 'utf8',
    ...options,
  })
}

function assertFile(path, message) {
  if (!existsSync(path)) {
    console.error(message)
    process.exit(1)
  }
}

function probePython(path) {
  if (!existsSync(path)) return null
  const result = capture(path, ['-c', 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")'])
  if (result.status !== 0) return null
  const version = String(result.stdout || '').trim()
  const [major, minor] = version.split('.').map((part) => Number(part))
  if (major !== 3 || minor < 11 || minor >= 13) return null
  return version
}

function ensurePdf2zhRuntime() {
  assertFile(sourceDir, `PDFMathTranslate-next source not found: ${sourceDir}`)
  assertFile(worker, `PDFMathTranslate worker script not found: ${worker}`)

  const version = probePython(venvPython)
  if (version) {
    console.log(`[lumenfolio] PDFMathTranslate-next dev runtime found: ${venvPython} (Python ${version})`)
    return
  }

  console.log('[lumenfolio] PDFMathTranslate-next dev runtime missing or unsupported; running npm run setup:pdf2zh')
  runSync(npmCommand(), ['run', 'setup:pdf2zh'])
}

function runProbe() {
  for (const dir of ['logs', 'cache', 'artifacts', 'tmp']) {
    mkdirSync(join(probeRoot, dir), { recursive: true })
  }
  const payload = {
    appDataDir: probeRoot,
    logDir: join(probeRoot, 'logs'),
    cacheDir: join(probeRoot, 'cache'),
    artifactsDir: join(probeRoot, 'artifacts'),
    tmpDir: join(probeRoot, 'tmp'),
  }
  const request = {
    id: 'tauri-dev-pdf2zh-probe',
    protocolVersion: 'lumenfolio-pdf2zh-v1',
    kind: 'probe',
    payload,
  }
  const result = capture(venvPython, ['-u', worker], {
    input: `${JSON.stringify(request)}\n`,
    maxBuffer: 1024 * 1024 * 8,
  })
  if (result.status !== 0) {
    console.error(result.stderr || result.stdout || 'PDFMathTranslate probe failed')
    process.exit(result.status || 1)
  }
  const lines = String(result.stdout || '').split(/\n/).filter(Boolean)
  const events = lines.map((line) => {
    try {
      return JSON.parse(line)
    } catch {
      console.error(`PDFMathTranslate worker emitted non-JSON protocol output: ${line}`)
      process.exit(1)
    }
  })
  const event = events.at(-1)
  if (event?.event !== 'probe_result' || event.status !== 'succeeded') {
    console.error(`PDFMathTranslate probe did not succeed: ${JSON.stringify(event || events)}`)
    process.exit(1)
  }
  console.log(`[lumenfolio] PDFMathTranslate-next ready: pdf2zh ${event.pdf2zhVersion || 'unknown'}, BabelDOC ${event.babeldocVersion || 'unknown'}`)
}

function startDesktop() {
  const child = spawn(npmCommand(), ['run', 'tauri:dev', '--', ...passthroughArgs], {
    cwd: appRoot,
    env: process.env,
    stdio: 'inherit',
  })
  child.on('exit', (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal)
      return
    }
    process.exit(code ?? 0)
  })
}

ensurePdf2zhRuntime()
runProbe()
if (probeOnly) {
  console.log('[lumenfolio] probe-only complete; desktop was not started')
} else {
  startDesktop()
}
