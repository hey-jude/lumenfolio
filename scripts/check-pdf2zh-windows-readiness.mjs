import { existsSync, mkdirSync, writeFileSync, unlinkSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawn } from 'node:child_process'

const __dirname = dirname(fileURLToPath(import.meta.url))
const appRoot = resolve(__dirname, '..')
const resourceRoot = join(appRoot, 'resources', 'pdf2zh-sidecar')
const worker = join(resourceRoot, 'worker', 'lumenfolio_pdf2zh_worker.py')
const frozenExe = join(resourceRoot, 'pdf2zh-sidecar.exe')
const bundledPython = join(resourceRoot, 'python', 'python.exe')
const explicitPython = process.env.LUMENFOLIO_PDF2ZH_PYTHON || ''
const requireWindows = process.env.LUMENFOLIO_PDF2ZH_REQUIRE_WINDOWS === '1'
const timeoutMs = Number(process.env.LUMENFOLIO_PDF2ZH_WINDOWS_PROBE_TIMEOUT_MS || 180_000)

function fail(message) {
  console.error(message)
  process.exit(1)
}

function assert(condition, message) {
  if (!condition) fail(message)
}

function formatEvent(event) {
  return JSON.stringify(event || null, null, 2)
}

function runtimeCommand() {
  if (existsSync(frozenExe)) {
    return { program: frozenExe, args: [] }
  }
  const python = explicitPython || bundledPython
  if (python && existsSync(python)) {
    assert(existsSync(worker), `Missing PDF2ZH worker script: ${worker}`)
    return { program: python, args: ['-u', worker] }
  }
  fail(`No Windows PDF2ZH runtime found. Expected ${frozenExe} or ${bundledPython}`)
}

function prepareWritableDir(root) {
  const dirs = {
    appDataDir: root,
    logDir: join(root, 'logs'),
    cacheDir: join(root, 'cache'),
    artifactsDir: join(root, 'artifacts'),
    tmpDir: join(root, 'tmp'),
  }
  for (const dir of Object.values(dirs)) {
    mkdirSync(dir, { recursive: true })
    const probePath = join(dir, '.lumenfolio-pdf2zh-write-test')
    writeFileSync(probePath, 'ok')
    unlinkSync(probePath)
  }
  return dirs
}

function validateJsonLines(stdout) {
  const lines = stdout.split(/\n/).filter(Boolean)
  assert(lines.length > 0, 'worker did not emit JSONL events')
  return lines.map((line) => {
    try {
      return JSON.parse(line)
    } catch {
      fail(`worker stdout contained non-JSON protocol output: ${line}`)
    }
  })
}

function runProbe(command, payload) {
  return new Promise((resolve, reject) => {
    const child = spawn(command.program, command.args, { cwd: appRoot, stdio: ['pipe', 'pipe', 'pipe'] })
    let stdout = ''
    let stderr = ''
    const timer = setTimeout(() => {
      child.kill()
      reject(new Error(`Windows PDF2ZH probe timed out after ${timeoutMs}ms\nSTDOUT:\n${stdout}\nSTDERR:\n${stderr}`))
    }, timeoutMs)
    child.stdout.on('data', (chunk) => {
      stdout += chunk.toString()
    })
    child.stderr.on('data', (chunk) => {
      stderr += chunk.toString()
    })
    child.on('error', (error) => {
      clearTimeout(timer)
      reject(error)
    })
    child.on('close', (code) => {
      clearTimeout(timer)
      if (code !== 0) {
        reject(new Error(`Windows PDF2ZH probe exited with code ${code}\nSTDOUT:\n${stdout}\nSTDERR:\n${stderr}`))
        return
      }
      resolve({ stdout, stderr, events: validateJsonLines(stdout) })
    })
    child.stdin.end(`${JSON.stringify({
      id: 'windows-readiness-probe',
      protocolVersion: 'lumenfolio-pdf2zh-v1',
      kind: 'probe',
      payload,
    })}\n`)
  })
}

async function main() {
  if (process.platform !== 'win32') {
    const result = {
      ok: !requireWindows,
      skipped: true,
      reason: 'Windows app-data verification only runs on win32',
    }
    console.log(JSON.stringify(result, null, 2))
    if (requireWindows) process.exit(1)
    return
  }

  const localAppData = process.env.LOCALAPPDATA
  assert(localAppData, 'LOCALAPPDATA is not set')
  const appDataRoot = join(localAppData, 'Lumenfolio', 'pdf2zh')
  const payload = prepareWritableDir(appDataRoot)
  const command = runtimeCommand()
  const probe = await runProbe(command, payload)
  const probeResult = probe.events.at(-1)
  assert(
    probeResult?.event === 'probe_result',
    `probe did not finish with probe_result. Last event:\n${formatEvent(probeResult)}`,
  )
  assert(
    probeResult.status === 'succeeded',
    `probe did not report succeeded status. Last event:\n${formatEvent(probeResult)}`,
  )
  console.log(JSON.stringify({
    ok: true,
    appDataRoot,
    runtimeProgram: command.program,
    pdf2zhVersion: probeResult.pdf2zhVersion,
    babeldocVersion: probeResult.babeldocVersion,
    python: probeResult.python,
  }, null, 2))
}

main().catch((error) => {
  fail(error.message)
})
