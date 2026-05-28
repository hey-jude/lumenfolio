import { existsSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'

const __dirname = dirname(fileURLToPath(import.meta.url))
const appRoot = resolve(__dirname, '..')
const repoRoot = resolve(appRoot, '..')
const venvDir = join(appRoot, '.venv-pdf2zh')
const localSourceDir = join(appRoot, 'external', 'PDFMathTranslate', 'pdf2zh', 'kernel', 'PDFMathTranslate-next.git')
const legacySourceDir = join(repoRoot, 'external', 'PDFMathTranslate', 'pdf2zh', 'kernel', 'PDFMathTranslate-next.git')
const sourceDir = existsSync(localSourceDir) ? localSourceDir : legacySourceDir
const venvPython = process.platform === 'win32'
  ? join(venvDir, 'Scripts', 'python.exe')
  : join(venvDir, 'bin', 'python')
const bootstrapPackages = [
  'scikit-learn>=1.7.1',
  'rapidocr-onnxruntime>=1.4.4',
]

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

function probePython(command) {
  const result = spawnSync(command, ['-c', 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")'], {
    encoding: 'utf8',
  })
  if (result.status !== 0) return null
  const version = String(result.stdout || '').trim()
  const [major, minor] = version.split('.').map((part) => Number(part))
  if (major !== 3 || minor < 11 || minor >= 13) return null
  return { command, version }
}

function selectPython() {
  const candidates = [
    process.env.PYTHON,
    'python3.12',
    'python3.11',
    'python3',
  ].filter(Boolean)
  for (const candidate of candidates) {
    const result = probePython(candidate)
    if (result) return result
  }
  console.error('PDFMathTranslate-next dependencies require Python 3.11 or 3.12. Set PYTHON=/path/to/python3.12 and retry.')
  process.exit(1)
}

function assertVenvPythonSupported() {
  if (!existsSync(venvPython)) return
  const result = probePython(venvPython)
  if (result) return
  console.error(`Existing venv uses an unsupported Python: ${venvPython}`)
  console.error('Remove .venv-pdf2zh and rerun npm run setup:pdf2zh with Python 3.11/3.12.')
  process.exit(1)
}

if (!existsSync(sourceDir)) {
  console.error(`PDFMathTranslate-next source not found: ${sourceDir}`)
  process.exit(1)
}

const python = selectPython()
assertVenvPythonSupported()

if (!existsSync(venvPython)) {
  run(python.command, ['-m', 'venv', venvDir])
}

run(venvPython, ['-m', 'pip', 'install', '--upgrade', 'pip'])
run(venvPython, ['-m', 'pip', 'install', ...bootstrapPackages])
run(venvPython, ['-m', 'pip', 'install', '-e', sourceDir])

console.log(`PDFMathTranslate-next runtime is ready: ${venvPython}`)
