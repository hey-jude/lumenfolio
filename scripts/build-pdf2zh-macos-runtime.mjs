import { existsSync, rmSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const __dirname = dirname(fileURLToPath(import.meta.url))
const appRoot = resolve(__dirname, '..')
const repoRoot = resolve(appRoot, '..')
const sourceDir = join(repoRoot, 'external', 'PDFMathTranslate', 'pdf2zh', 'kernel', 'PDFMathTranslate-next.git')
const defaultOutputDir = join(appRoot, 'resources', 'pdf2zh-sidecar', 'python')
const args = new Set(process.argv.slice(2))
const dryRun = args.has('--dry-run')
const force = args.has('--force')
const bootstrapPackages = [
  'scikit-learn>=1.7.1',
  'rapidocr-onnxruntime>=1.4.4',
]

function optionValue(name, fallback) {
  const prefix = `${name}=`
  const value = process.argv.slice(2).find((arg) => arg.startsWith(prefix))
  return value ? value.slice(prefix.length) : fallback
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    stdio: 'inherit',
    cwd: appRoot,
    env: process.env,
    ...options,
  })
  if (result.status !== 0) {
    process.exit(result.status || 1)
  }
}

function capture(command, args) {
  const result = spawnSync(command, args, {
    cwd: appRoot,
    env: process.env,
    encoding: 'utf8',
  })
  if (result.status !== 0) return null
  return String(result.stdout || '').trim()
}

function probePython(command) {
  const version = capture(command, ['-c', 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")'])
  if (!version) return null
  const [major, minor] = version.split('.').map((part) => Number(part))
  if (major !== 3 || minor < 11 || minor >= 13) return null
  return { command, version }
}

function selectPython() {
  const explicit = optionValue('--python', process.env.PYTHON || '')
  const candidates = [
    explicit,
    'python3.12',
    'python3.11',
    'python3',
  ].filter(Boolean)
  for (const candidate of candidates) {
    const result = probePython(candidate)
    if (result) return result
  }
  console.error('PDFMathTranslate-next release runtime requires Python 3.11 or 3.12.')
  process.exit(1)
}

function runtimePython(outputDir) {
  return process.platform === 'win32'
    ? join(outputDir, 'Scripts', 'python.exe')
    : join(outputDir, 'bin', 'python3')
}

function verifyRuntime(outputPython) {
  const script = [
    'import json, pathlib, sys',
    'import pdf2zh_next, babeldoc',
    'payload = {',
    '  "python": sys.executable,',
    '  "pdf2zhNext": pathlib.Path(pdf2zh_next.__file__).as_posix(),',
    '  "babeldoc": pathlib.Path(babeldoc.__file__).as_posix(),',
    '}',
    'print(json.dumps(payload))',
  ].join('\n')
  const output = capture(outputPython, ['-c', script])
  if (!output) {
    console.error('Failed to verify packaged PDF2ZH runtime imports.')
    process.exit(1)
  }
  const payload = JSON.parse(output)
  if (payload.pdf2zhNext.includes('/external/PDFMathTranslate/')) {
    console.error(`Packaged runtime still imports editable PDFMathTranslate source: ${payload.pdf2zhNext}`)
    process.exit(1)
  }
  return payload
}

if (process.platform === 'win32') {
  console.error('This script builds the macOS/private-Python runtime layout. Use a frozen sidecar build for Windows.')
  process.exit(1)
}

if (!existsSync(sourceDir)) {
  console.error(`PDFMathTranslate-next source not found: ${sourceDir}`)
  process.exit(1)
}

const outputDir = resolve(optionValue('--output', process.env.LUMENFOLIO_PDF2ZH_RUNTIME_OUTPUT || defaultOutputDir))
const python = selectPython()
const outputPython = runtimePython(outputDir)

if (dryRun) {
  console.log(JSON.stringify({
    ok: true,
    dryRun: true,
    python,
    sourceDir,
    outputDir,
    outputPython,
    bootstrapPackages,
  }, null, 2))
  process.exit(0)
}

if (existsSync(outputDir)) {
  if (!force) {
    console.error(`Runtime output already exists: ${outputDir}`)
    console.error('Rerun with --force to replace it.')
    process.exit(1)
  }
  rmSync(outputDir, { recursive: true, force: true })
}

run(python.command, ['-m', 'venv', outputDir])
run(outputPython, ['-m', 'pip', 'install', '--upgrade', 'pip'])
run(outputPython, ['-m', 'pip', 'install', ...bootstrapPackages])
run(outputPython, ['-m', 'pip', 'install', sourceDir])

const verification = verifyRuntime(outputPython)
console.log(JSON.stringify({
  ok: true,
  outputDir,
  outputPython,
  verification,
}, null, 2))
