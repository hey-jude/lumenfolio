import { existsSync, mkdirSync, rmSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const __dirname = dirname(fileURLToPath(import.meta.url))
const appRoot = resolve(__dirname, '..')
const resourceRoot = join(appRoot, 'resources', 'pdf2zh-sidecar')
const worker = join(resourceRoot, 'worker', 'lumenfolio_pdf2zh_worker.py')
const outputExe = join(resourceRoot, 'pdf2zh-sidecar.exe')
const defaultPython = join(appRoot, '.venv-pdf2zh', 'Scripts', 'python.exe')
const buildRoot = join(appRoot, 'artifacts', 'pdf2zh-windows-build')
const args = new Set(process.argv.slice(2))
const skipInstall = args.has('--skip-install')
const skipReadiness = args.has('--skip-readiness')
const clean = args.has('--clean')
const requireWindows = process.env.LUMENFOLIO_PDF2ZH_REQUIRE_WINDOWS === '1'

function optionValue(name, fallback) {
  const prefix = `${name}=`
  const value = process.argv.slice(2).find((arg) => arg.startsWith(prefix))
  return value ? value.slice(prefix.length) : fallback
}

function fail(message, code = 1) {
  console.error(message)
  process.exit(code)
}

function run(command, commandArgs, options = {}) {
  const result = spawnSync(command, commandArgs, {
    cwd: appRoot,
    env: process.env,
    stdio: 'inherit',
    ...options,
  })
  if (result.status !== 0) {
    process.exit(result.status || 1)
  }
}

function capture(command, commandArgs) {
  const result = spawnSync(command, commandArgs, {
    cwd: appRoot,
    env: process.env,
    encoding: 'utf8',
  })
  if (result.status !== 0) return null
  return String(result.stdout || '').trim()
}

function assertPython(python) {
  const version = capture(python, ['-c', 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")'])
  if (!version) {
    fail(`Unable to run Windows PDF2ZH Python: ${python}`)
  }
  const [major, minor] = version.split('.').map((part) => Number(part))
  if (major !== 3 || minor < 11 || minor >= 13) {
    fail(`PDFMathTranslate-next requires Python 3.11 or 3.12, got ${version}`)
  }
  return version
}

function npmCommand() {
  return process.platform === 'win32' ? 'npm.cmd' : 'npm'
}

async function main() {
  if (process.platform !== 'win32') {
    const result = {
      ok: !requireWindows,
      skipped: true,
      reason: 'Windows frozen runtime build only runs on win32',
    }
    console.log(JSON.stringify(result, null, 2))
    if (requireWindows) process.exit(1)
    return
  }

  if (!existsSync(worker)) {
    fail(`Missing PDF2ZH worker script: ${worker}`)
  }

  const python = resolve(optionValue('--python', process.env.LUMENFOLIO_PDF2ZH_PYTHON || defaultPython))
  if (!existsSync(python)) {
    fail(`Missing Windows PDF2ZH Python: ${python}. Run npm run setup:pdf2zh first or pass --python=...`)
  }

  const pythonVersion = assertPython(python)
  mkdirSync(resourceRoot, { recursive: true })
  mkdirSync(buildRoot, { recursive: true })

  if (clean) {
    rmSync(outputExe, { force: true })
    rmSync(join(buildRoot, 'work'), { recursive: true, force: true })
    rmSync(join(buildRoot, 'spec'), { recursive: true, force: true })
  }

  if (!skipInstall) {
    run(python, ['-m', 'pip', 'install', '--upgrade', 'pyinstaller'])
  }

  run(python, [
    '-m',
    'PyInstaller',
    '--onefile',
    '--clean',
    '--name',
    'pdf2zh-sidecar',
    '--distpath',
    resourceRoot,
    '--workpath',
    join(buildRoot, 'work'),
    '--specpath',
    join(buildRoot, 'spec'),
    worker,
  ])

  if (!existsSync(outputExe)) {
    fail(`PyInstaller completed but did not create ${outputExe}`)
  }

  if (!skipReadiness) {
    run(npmCommand(), ['run', 'check:pdf2zh-windows-readiness'])
  }

  console.log(JSON.stringify({
    ok: true,
    python,
    pythonVersion,
    outputExe,
    readinessChecked: !skipReadiness,
  }, null, 2))
}

main().catch((error) => {
  fail(error.message)
})
