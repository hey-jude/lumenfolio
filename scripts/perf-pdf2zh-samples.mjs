import { existsSync, mkdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawn, spawnSync } from 'node:child_process'

const __dirname = dirname(fileURLToPath(import.meta.url))
const appRoot = resolve(__dirname, '..')
const perfRoot = process.env.LUMENFOLIO_PDF2ZH_PERF_DIR || join(appRoot, 'artifacts', 'pdf2zh-perf')
const worker = join(appRoot, 'resources', 'pdf2zh-sidecar', 'worker', 'lumenfolio_pdf2zh_worker.py')
const privatePython = join(appRoot, 'resources', 'pdf2zh-sidecar', 'python', 'bin', 'python3')
const venvPython = process.platform === 'win32'
  ? join(appRoot, '.venv-pdf2zh', 'Scripts', 'python.exe')
  : join(appRoot, '.venv-pdf2zh', 'bin', 'python')
const python = process.env.LUMENFOLIO_PDF2ZH_PYTHON
  || (existsSync(privatePython) ? privatePython : venvPython)

function assert(condition, message) {
  if (!condition) throw new Error(message)
}

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: appRoot,
    env: process.env,
    encoding: 'utf8',
  })
  if (result.status !== 0) {
    throw new Error(`command failed: ${command} ${args.join(' ')}\n${result.stdout || ''}\n${result.stderr || ''}`)
  }
  return String(result.stdout || '').trim()
}

function preparePerfDir() {
  if (!process.env.LUMENFOLIO_PDF2ZH_KEEP_PERF) {
    rmSync(perfRoot, { recursive: true, force: true })
  }
  for (const part of ['logs', 'cache', 'artifacts', 'tmp', 'outputs', 'samples']) {
    mkdirSync(join(perfRoot, part), { recursive: true })
  }
}

function generatePerfSample() {
  const script = [
    'import fitz, pathlib, sys',
    'root = pathlib.Path(sys.argv[1])',
    'root.mkdir(parents=True, exist_ok=True)',
    'doc = fitz.open()',
    'for page_no in range(1, 7):',
    '    page = doc.new_page(width=612, height=792)',
    '    page.insert_text((72, 72), f"PDF2ZH Performance Sample - Page {page_no}", fontsize=16)',
    '    y = 112',
    '    for para in range(5):',
    '        text = f"Page {page_no}, paragraph {para + 1}: this repeatable sample compares single page translation, multi page range translation, and full document translation for the Lumenfolio PDFMathTranslate sidecar."',
    '        page.insert_textbox((72, y, 540, y + 54), text, fontsize=10, align=0)',
    '        y += 66',
    'doc.save(root / "synthetic-perf-6-pages.pdf")',
  ].join('\n')
  run(python, ['-c', script, join(perfRoot, 'samples')])
  return join(perfRoot, 'samples', 'synthetic-perf-6-pages.pdf')
}

function basePayload() {
  return {
    appDataDir: perfRoot,
    logDir: join(perfRoot, 'logs'),
    cacheDir: join(perfRoot, 'cache'),
    artifactsDir: join(perfRoot, 'artifacts'),
    tmpDir: join(perfRoot, 'tmp'),
  }
}

function validateJsonLines(stdout) {
  const lines = stdout.split(/\n/).filter(Boolean)
  assert(lines.length > 0, 'worker did not emit JSONL events')
  return lines.map((line) => JSON.parse(line))
}

function runWorker(request, timeoutMs) {
  return new Promise((resolve, reject) => {
    const child = spawn(python, ['-u', worker], { cwd: appRoot, stdio: ['pipe', 'pipe', 'pipe'] })
    let stdout = ''
    let stderr = ''
    const startedAt = process.hrtime.bigint()
    const timer = setTimeout(() => {
      child.kill()
      reject(new Error(`worker timed out after ${timeoutMs}ms for ${request.id}\nSTDOUT:\n${stdout}\nSTDERR:\n${stderr}`))
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
      const durationMs = Number(process.hrtime.bigint() - startedAt) / 1_000_000
      if (code !== 0) {
        reject(new Error(`worker exited with code ${code} for ${request.id}\nSTDOUT:\n${stdout}\nSTDERR:\n${stderr}`))
        return
      }
      resolve({ stdout, stderr, events: validateJsonLines(stdout), durationMs })
    })
    child.stdin.end(`${JSON.stringify(request)}\n`)
  })
}

function analyzePdf(pdfPath) {
  const script = [
    'import fitz, json, pathlib, sys',
    'path = pathlib.Path(sys.argv[1])',
    'doc = fitz.open(path)',
    'print(json.dumps({"pageCount": doc.page_count, "fileSize": path.stat().st_size}))',
  ].join('\n')
  return JSON.parse(run(python, ['-c', script, pdfPath]))
}

async function translateScenario(samplePdf, scenario) {
  const outputDir = join(perfRoot, 'outputs', scenario.id)
  mkdirSync(outputDir, { recursive: true })
  const payload = {
    ...basePayload(),
    inputPdfPath: samplePdf,
    outputDir,
    sourceLang: 'en',
    targetLang: 'zh',
    artifactMode: 'both',
    forceRefresh: true,
    engine: { provider: 'google-web' },
  }
  if (scenario.pages) {
    payload.pages = scenario.pages
    payload.onlyIncludeTranslatedPage = true
  } else {
    payload.onlyIncludeTranslatedPage = false
  }
  const run = await runWorker({
    id: `perf-${scenario.id}`,
    protocolVersion: 'lumenfolio-pdf2zh-v1',
    kind: 'translate',
    payload,
  }, Number(process.env.LUMENFOLIO_PDF2ZH_PERF_TIMEOUT_MS || 600_000))
  writeFileSync(join(perfRoot, `${scenario.id}.stdout.jsonl`), run.stdout)
  writeFileSync(join(perfRoot, `${scenario.id}.stderr.log`), run.stderr)
  const finish = run.events.find((event) => event.event === 'finish')
  assert(finish, `${scenario.id} did not emit finish`)
  for (const key of ['monoPdfPath', 'dualPdfPath']) {
    assert(finish[key] && existsSync(finish[key]), `${scenario.id} missing ${key}`)
    assert(statSync(finish[key]).size > 0, `${scenario.id} empty ${key}`)
  }
  return {
    ...scenario,
    durationMs: Math.round(run.durationMs),
    events: run.events.length,
    monoPdfPath: finish.monoPdfPath,
    dualPdfPath: finish.dualPdfPath,
    mono: analyzePdf(finish.monoPdfPath),
    dual: analyzePdf(finish.dualPdfPath),
  }
}

function writeReports(samplePdf, results) {
  const report = {
    ok: true,
    generatedAt: new Date().toISOString(),
    python,
    perfRoot,
    samplePdf,
    sample: analyzePdf(samplePdf),
    results,
  }
  writeFileSync(join(perfRoot, 'report.json'), `${JSON.stringify(report, null, 2)}\n`)
  const lines = [
    '# PDF2ZH Sidecar Performance Report',
    '',
    `- Python: \`${python}\``,
    `- Perf root: \`${perfRoot}\``,
    `- Sample: \`${samplePdf}\``,
    `- Generated at: \`${report.generatedAt}\``,
    '',
    '| Scenario | Pages | Duration ms | Events | Mono pages | Dual pages |',
    '|---|---|---:|---:|---:|---:|',
  ]
  for (const result of results) {
    lines.push([
      result.id,
      result.pages || 'full',
      result.durationMs,
      result.events,
      result.mono.pageCount,
      result.dual.pageCount,
    ].join(' | ').replace(/^/, '| ').replace(/$/, ' |'))
  }
  lines.push('')
  lines.push('Notes:')
  lines.push('- This benchmark uses a controlled six-page synthetic PDF.')
  lines.push('- A one-page warm-up translation runs before measured scenarios unless `LUMENFOLIO_PDF2ZH_PERF_SKIP_WARMUP=1` is set.')
  lines.push('- `forceRefresh=true` is used for each scenario to avoid reporting a pure artifact cache hit.')
  lines.push('- The numbers include worker startup and warm-cache PDFMathTranslate runtime access for each measured scenario.')
  writeFileSync(join(perfRoot, 'report.md'), `${lines.join('\n')}\n`)
  return report
}

async function main() {
  assert(existsSync(python), `Python runtime missing: ${python}`)
  assert(existsSync(worker), `worker missing: ${worker}`)
  preparePerfDir()
  const samplePdf = generatePerfSample()
  if (process.env.LUMENFOLIO_PDF2ZH_PERF_SKIP_WARMUP !== '1') {
    await translateScenario(samplePdf, { id: 'warmup-single-page', pages: '1' })
  }
  const scenarios = [
    { id: 'single-page', pages: '1' },
    { id: 'range-1-3', pages: '1-3' },
    { id: 'full-6-pages', pages: '' },
  ]
  const results = []
  for (const scenario of scenarios) {
    results.push(await translateScenario(samplePdf, scenario))
  }
  const report = writeReports(samplePdf, results)
  console.log(JSON.stringify(report, null, 2))
}

main().catch((error) => {
  console.error(error.message)
  process.exit(1)
})
