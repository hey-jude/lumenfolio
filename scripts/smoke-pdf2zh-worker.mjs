import { existsSync, mkdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawn } from 'node:child_process'

const __dirname = dirname(fileURLToPath(import.meta.url))
const appRoot = resolve(__dirname, '..')
const repoRoot = resolve(appRoot, '..')
const appSourceDir = join(appRoot, 'external', 'PDFMathTranslate', 'pdf2zh', 'kernel', 'PDFMathTranslate-next.git')
const parentSourceDir = join(repoRoot, 'external', 'PDFMathTranslate', 'pdf2zh', 'kernel', 'PDFMathTranslate-next.git')
const sourceDir = existsSync(appSourceDir) ? appSourceDir : parentSourceDir
const smokeRoot = process.env.LUMENFOLIO_PDF2ZH_SMOKE_DIR || '/tmp/lumenfolio-pdf2zh-worker-smoke'
const venvPython = process.platform === 'win32'
  ? join(appRoot, '.venv-pdf2zh', 'Scripts', 'python.exe')
  : join(appRoot, '.venv-pdf2zh', 'bin', 'python')
const python = process.env.LUMENFOLIO_PDF2ZH_PYTHON || venvPython
const explicitSidecar = process.env.LUMENFOLIO_PDF2ZH_SIDECAR || ''
const worker = join(appRoot, 'resources', 'pdf2zh-sidecar', 'worker', 'lumenfolio_pdf2zh_worker.py')
const smokeTimeoutMs = Number(process.env.LUMENFOLIO_PDF2ZH_SMOKE_TIMEOUT_MS || 300_000)
const probeTimeoutMs = Number(process.env.LUMENFOLIO_PDF2ZH_SMOKE_PROBE_TIMEOUT_MS || 180_000)
const quickErrorTimeoutMs = Number(process.env.LUMENFOLIO_PDF2ZH_SMOKE_ERROR_TIMEOUT_MS || 120_000)
const killTimeoutMs = Number(process.env.LUMENFOLIO_PDF2ZH_SMOKE_KILL_TIMEOUT_MS || 120_000)
const samplePdf = join(
  sourceDir,
  'test',
  'file',
  'translate.cli.plain.text.pdf',
)

function assert(condition, message) {
  if (!condition) {
    throw new Error(message)
  }
}

function prepareSmokeDir() {
  if (!process.env.LUMENFOLIO_PDF2ZH_KEEP_SMOKE_CACHE) {
    rmSync(smokeRoot, { recursive: true, force: true })
  }
  for (const part of ['logs', 'cache', 'artifacts', 'tmp', 'out']) {
    mkdirSync(join(smokeRoot, part), { recursive: true })
  }
}

function validateJsonLines(stdout) {
  const lines = stdout.split(/\n/).filter(Boolean)
  assert(lines.length > 0, 'worker did not emit any JSONL protocol events')
  return lines.map((line) => {
    try {
      return JSON.parse(line)
    } catch (error) {
      throw new Error(`worker stdout contained non-JSON protocol output: ${line}`)
    }
  })
}

function runtimeCommand() {
  if (explicitSidecar) {
    assert(existsSync(explicitSidecar), `PDFMathTranslate sidecar executable not found: ${explicitSidecar}`)
    return { program: explicitSidecar, args: [], kind: 'executable' }
  }
  assert(existsSync(python), `PDFMathTranslate Python not found: ${python}`)
  assert(existsSync(worker), `worker script not found: ${worker}`)
  return { program: python, args: ['-u', worker], kind: 'python' }
}

function runWorker(request, timeoutMs, options = {}) {
  return new Promise((resolve, reject) => {
    const command = runtimeCommand()
    const child = spawn(command.program, command.args, { cwd: appRoot, stdio: ['pipe', 'pipe', 'pipe'] })
    let stdout = ''
    let stderr = ''
    const timer = setTimeout(() => {
      child.kill()
      reject(new Error(`worker timed out after ${timeoutMs}ms\nSTDOUT:\n${stdout}\nSTDERR:\n${stderr}`))
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
      if (code !== 0 && !options.allowNonZero) {
        reject(new Error(`worker exited with code ${code}\nSTDOUT:\n${stdout}\nSTDERR:\n${stderr}`))
        return
      }
      resolve({ stdout, stderr, events: validateJsonLines(stdout) })
    })
    child.stdin.end(`${JSON.stringify(request)}\n`)
  })
}

function runWorkerAndKillAfterProgress(request, timeoutMs) {
  return new Promise((resolve, reject) => {
    const command = runtimeCommand()
    const child = spawn(command.program, command.args, { cwd: appRoot, stdio: ['pipe', 'pipe', 'pipe'] })
    let stdout = ''
    let stderr = ''
    let settled = false
    const timer = setTimeout(() => {
      if (!settled) {
        settled = true
        child.kill()
        reject(new Error(`worker did not emit progress before kill timeout\nSTDOUT:\n${stdout}\nSTDERR:\n${stderr}`))
      }
    }, timeoutMs)
    child.stdout.on('data', (chunk) => {
      stdout += chunk.toString()
      const lines = stdout.split(/\n/)
      if (!stdout.endsWith('\n')) lines.pop()
      const events = lines.filter(Boolean).map((line) => JSON.parse(line))
      if (!settled && events.some((event) => event.event === 'progress')) {
        child.kill()
      }
    })
    child.stderr.on('data', (chunk) => {
      stderr += chunk.toString()
    })
    child.on('error', (error) => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      reject(error)
    })
    child.on('close', (code, signal) => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      const events = validateJsonLines(stdout)
      assert(events.some((event) => event.event === 'progress'), 'killed worker did not emit progress before termination')
      assert(signal || code !== 0, 'killed worker exited as if it completed normally')
      resolve({ stdout, stderr, events, code, signal })
    })
    child.stdin.end(`${JSON.stringify(request)}\n`)
  })
}

function basePayload() {
  return {
    appDataDir: smokeRoot,
    logDir: join(smokeRoot, 'logs'),
    cacheDir: join(smokeRoot, 'cache'),
    artifactsDir: join(smokeRoot, 'artifacts'),
    tmpDir: join(smokeRoot, 'tmp'),
  }
}

async function main() {
  const command = runtimeCommand()
  assert(existsSync(samplePdf), `sample PDF not found: ${samplePdf}`)
  prepareSmokeDir()

  const probe = await runWorker({
    id: 'probe',
    protocolVersion: 'lumenfolio-pdf2zh-v1',
    kind: 'probe',
    payload: basePayload(),
  }, probeTimeoutMs)
  const probeResult = probe.events.at(-1)
  assert(probeResult?.event === 'probe_result', 'probe did not finish with probe_result')

  const preflight = await runWorker({
    id: 'preflight',
    protocolVersion: 'lumenfolio-pdf2zh-v1',
    kind: 'preflight',
    payload: {
      ...basePayload(),
      outputDir: join(smokeRoot, 'preflight'),
      sourceLang: 'en',
      targetLang: 'zh',
      engine: { provider: 'google-web' },
    },
  }, quickErrorTimeoutMs)
  const preflightResult = preflight.events.at(-1)
  assert(preflightResult?.event === 'preflight_result', 'preflight did not finish with preflight_result')

  const translate = await runWorker({
    id: 'tiny-translate',
    protocolVersion: 'lumenfolio-pdf2zh-v1',
    kind: 'translate',
    payload: {
      ...basePayload(),
      inputPdfPath: samplePdf,
      outputDir: join(smokeRoot, 'out'),
      sourceLang: 'en',
      targetLang: 'zh',
      artifactMode: 'both',
      pages: '1',
      onlyIncludeTranslatedPage: true,
      forceRefresh: true,
      engine: { provider: 'google-web' },
    },
  }, smokeTimeoutMs)
  const finish = translate.events.find((event) => event.event === 'finish')
  const artifactReady = translate.events.find((event) => event.event === 'artifact_ready')
  assert(artifactReady, 'tiny PDF translation did not emit artifact_ready for partial output')
  assert(artifactReady.artifactScope === 'partial', 'artifact_ready did not mark partial scope')
  assert(String(artifactReady.artifactPages || '') === '1', 'artifact_ready did not report requested page 1')
  assert(finish, 'tiny PDF translation did not emit finish')
  for (const key of ['monoPdfPath', 'dualPdfPath']) {
    const value = finish[key]
    assert(value && existsSync(value), `${key} was not created: ${value}`)
    assert(statSync(value).size > 0, `${key} is empty: ${value}`)
  }

  const invalidProvider = await runWorker({
    id: 'invalid-provider',
    protocolVersion: 'lumenfolio-pdf2zh-v1',
    kind: 'translate',
    payload: {
      ...basePayload(),
      inputPdfPath: samplePdf,
      outputDir: join(smokeRoot, 'out-invalid-provider'),
      sourceLang: 'en',
      targetLang: 'zh',
      artifactMode: 'both',
      pages: '1',
      onlyIncludeTranslatedPage: true,
      forceRefresh: true,
      engine: { provider: 'definitely-unsupported' },
    },
  }, quickErrorTimeoutMs, { allowNonZero: true })
  const invalidProviderError = invalidProvider.events.at(-1)
  assert(invalidProviderError?.event === 'error', 'invalid provider did not emit protocol error')
  assert(invalidProviderError.errorCode, 'invalid provider error did not include errorCode')

  const killed = await runWorkerAndKillAfterProgress({
    id: 'kill-after-progress',
    protocolVersion: 'lumenfolio-pdf2zh-v1',
    kind: 'translate',
    payload: {
      ...basePayload(),
      inputPdfPath: samplePdf,
      outputDir: join(smokeRoot, 'out-killed'),
      sourceLang: 'en',
      targetLang: 'zh',
      artifactMode: 'both',
      pages: '1',
      onlyIncludeTranslatedPage: true,
      forceRefresh: true,
      engine: { provider: 'google-web' },
    },
  }, killTimeoutMs)

  writeFileSync(join(smokeRoot, 'probe.stdout.jsonl'), probe.stdout)
  writeFileSync(join(smokeRoot, 'probe.stderr.log'), probe.stderr)
  writeFileSync(join(smokeRoot, 'translate.stdout.jsonl'), translate.stdout)
  writeFileSync(join(smokeRoot, 'translate.stderr.log'), translate.stderr)
  writeFileSync(join(smokeRoot, 'killed.stdout.jsonl'), killed.stdout)
  writeFileSync(join(smokeRoot, 'killed.stderr.log'), killed.stderr)
  console.log(JSON.stringify({
    ok: true,
    smokeRoot,
    runtimeKind: command.kind,
    runtimeProgram: command.program,
    pdf2zhVersion: probeResult.pdf2zhVersion,
    babeldocVersion: probeResult.babeldocVersion,
    monoPdfPath: finish.monoPdfPath,
    dualPdfPath: finish.dualPdfPath,
    partialMonoPdfPath: artifactReady.monoPdfPath,
    partialDualPdfPath: artifactReady.dualPdfPath,
    invalidProviderErrorCode: invalidProviderError.errorCode,
    killedSignal: killed.signal,
    events: translate.events.length,
  }, null, 2))
}

main().catch((error) => {
  console.error(error.message)
  process.exit(1)
})
