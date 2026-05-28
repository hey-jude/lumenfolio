import { existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawn, spawnSync } from 'node:child_process'

const __dirname = dirname(fileURLToPath(import.meta.url))
const appRoot = resolve(__dirname, '..')
const repoRoot = resolve(appRoot, '..')
const qaRoot = process.env.LUMENFOLIO_PDF2ZH_QA_DIR || join(appRoot, 'artifacts', 'pdf2zh-qa')
const worker = join(appRoot, 'resources', 'pdf2zh-sidecar', 'worker', 'lumenfolio_pdf2zh_worker.py')
const privatePython = join(appRoot, 'resources', 'pdf2zh-sidecar', 'python', 'bin', 'python3')
const venvPython = process.platform === 'win32'
  ? join(appRoot, '.venv-pdf2zh', 'Scripts', 'python.exe')
  : join(appRoot, '.venv-pdf2zh', 'bin', 'python')
const python = process.env.LUMENFOLIO_PDF2ZH_PYTHON
  || (existsSync(privatePython) ? privatePython : venvPython)
const sampleRoot = join(repoRoot, 'external', 'PDFMathTranslate', 'pdf2zh', 'kernel', 'PDFMathTranslate-next.git', 'test', 'file')
const sampleDefinitions = [
  {
    id: 'single-column',
    requirement: 'PDFTR-071',
    inputPdfPath: join(sampleRoot, 'translate.cli.plain.text.pdf'),
    pages: '1',
  },
  {
    id: 'figure',
    requirement: 'PDFTR-073',
    inputPdfPath: join(sampleRoot, 'translate.cli.text.with.figure.pdf'),
    pages: '1',
  },
  {
    id: 'formula-synthetic',
    requirement: 'PDFTR-074',
    inputPdfPath: join(qaRoot, 'samples', 'synthetic-formula.pdf'),
    pages: '1',
  },
  {
    id: 'table-synthetic',
    requirement: 'PDFTR-075',
    inputPdfPath: join(qaRoot, 'samples', 'synthetic-table.pdf'),
    pages: '1',
  },
  {
    id: 'stress-55-pages',
    requirement: 'PDFTR-076',
    inputPdfPath: join(qaRoot, 'samples', 'synthetic-55-pages.pdf'),
    pages: process.env.LUMENFOLIO_PDF2ZH_QA_FULL_STRESS === '1' ? '1-55' : '1',
  },
]
const knownRiskSampleDefinitions = [
  {
    id: 'font-unknown',
    requirement: 'PDFTR-074-risk',
    inputPdfPath: join(sampleRoot, 'translate.cli.font.unknown.pdf'),
    pages: '1',
  },
]

function assert(condition, message) {
  if (!condition) throw new Error(message)
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: appRoot,
    env: process.env,
    encoding: 'utf8',
    ...options,
  })
  if (result.status !== 0) {
    throw new Error(`command failed: ${command} ${args.join(' ')}\n${result.stdout || ''}\n${result.stderr || ''}`)
  }
  return String(result.stdout || '').trim()
}

function prepareQaDir() {
  if (!process.env.LUMENFOLIO_PDF2ZH_KEEP_QA) {
    rmSync(qaRoot, { recursive: true, force: true })
  }
  for (const part of ['logs', 'cache', 'artifacts', 'tmp', 'outputs', 'renders', 'samples']) {
    mkdirSync(join(qaRoot, part), { recursive: true })
  }
}

function generateSyntheticSamples() {
  const script = [
    'import fitz, pathlib, sys',
    'root = pathlib.Path(sys.argv[1])',
    'root.mkdir(parents=True, exist_ok=True)',
    'def add_wrapped_text(page, text, x, y, width, size=10, line_height=14):',
    '    words = text.split()',
    '    line = ""',
    '    for word in words:',
    '        candidate = (line + " " + word).strip()',
    '        if len(candidate) > 84:',
    '            page.insert_text((x, y), line, fontsize=size)',
    '            y += line_height',
    '            line = word',
    '        else:',
    '            line = candidate',
    '    if line:',
    '        page.insert_text((x, y), line, fontsize=size)',
    '        y += line_height',
    '    return y',
    'table_doc = fitz.open()',
    'page = table_doc.new_page(width=612, height=792)',
    'page.insert_text((72, 72), "Synthetic Table Layout QA", fontsize=18)',
    'y = add_wrapped_text(page, "This synthetic sample verifies that the PDF translation sidecar can process a document containing paragraphs, ruled table cells, numeric values, and captions without dropping the table area.", 72, 108, 468, 10, 14)',
    'x0, y0, cell_w, cell_h = 72, y + 24, 92, 32',
    'headers = ["Metric", "Baseline", "Experiment A", "Experiment B", "Notes"]',
    'rows = [',
    '    ["Accuracy", "91.4%", "94.8%", "95.1%", "stable"],',
    '    ["Latency", "182 ms", "149 ms", "146 ms", "lower"],',
    '    ["Memory", "1.8 GB", "2.1 GB", "2.0 GB", "bounded"],',
    '    ["Failures", "12", "4", "3", "retryable"],',
    ']',
    'for r in range(len(rows) + 1):',
    '    yline = y0 + r * cell_h',
    '    page.draw_line((x0, yline), (x0 + len(headers) * cell_w, yline), color=(0, 0, 0), width=0.6)',
    'page.draw_line((x0, y0 + (len(rows) + 1) * cell_h), (x0 + len(headers) * cell_w, y0 + (len(rows) + 1) * cell_h), color=(0, 0, 0), width=0.6)',
    'for c in range(len(headers) + 1):',
    '    xline = x0 + c * cell_w',
    '    page.draw_line((xline, y0), (xline, y0 + (len(rows) + 1) * cell_h), color=(0, 0, 0), width=0.6)',
    'for c, value in enumerate(headers):',
    '    page.insert_text((x0 + c * cell_w + 6, y0 + 21), value, fontsize=8.5)',
    'for r, row in enumerate(rows, start=1):',
    '    for c, value in enumerate(row):',
    '        page.insert_text((x0 + c * cell_w + 6, y0 + r * cell_h + 21), value, fontsize=8.5)',
    'page.insert_text((72, y0 + (len(rows) + 1) * cell_h + 28), "Table 1. Synthetic benchmark rows for translation QA.", fontsize=10)',
    'table_doc.save(root / "synthetic-table.pdf")',
    'formula_doc = fitz.open()',
    'page = formula_doc.new_page(width=612, height=792)',
    'page.insert_text((72, 72), "Synthetic Formula Layout QA", fontsize=18)',
    'page.insert_text((72, 120), "This page contains mathematical expressions, inline variables, superscripts, Greek letters, and a displayed equation block.", fontsize=10)',
    'page.insert_text((72, 164), "The energy equation E = m c^2 is used as a compact inline expression.", fontsize=11)',
    'page.insert_text((72, 214), "For a differentiable function f(x), the update rule is:", fontsize=11)',
    'page.draw_rect((108, 246, 504, 330), color=(0, 0, 0), width=0.8)',
    'page.insert_text((150, 286), "theta_{t+1} = theta_t - eta * gradient L(theta_t)", fontsize=14)',
    'page.insert_text((72, 372), "The normalized score is sigma = (x - mu) / sqrt(variance + epsilon).", fontsize=11)',
    'page.insert_text((72, 418), "Equation 1. Synthetic formula content for sidecar visual QA.", fontsize=10)',
    'formula_doc.save(root / "synthetic-formula.pdf")',
    'stress_doc = fitz.open()',
    'for index in range(55):',
    '    page = stress_doc.new_page(width=612, height=792)',
    '    page.insert_text((72, 72), f"Synthetic Stress Document - Page {index + 1}", fontsize=16)',
    '    y = 110',
    '    for para in range(7):',
    '        text = f"Page {index + 1}, paragraph {para + 1}: this controlled long document is used to validate that the sidecar can accept a fifty five page PDF and produce priority partial output while preserving page metadata and progress events."',
    '        y = add_wrapped_text(page, text, 72, y, 468, 10, 14) + 8',
    'stress_doc.save(root / "synthetic-55-pages.pdf")',
  ].join('\n')
  run(python, ['-c', script, join(qaRoot, 'samples')])
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
      if (code !== 0) {
        reject(new Error(`worker exited with code ${code} for ${request.id}\nSTDOUT:\n${stdout}\nSTDERR:\n${stderr}`))
        return
      }
      resolve({ stdout, stderr, events: validateJsonLines(stdout) })
    })
    child.stdin.end(`${JSON.stringify(request)}\n`)
  })
}

function basePayload() {
  return {
    appDataDir: qaRoot,
    logDir: join(qaRoot, 'logs'),
    cacheDir: join(qaRoot, 'cache'),
    artifactsDir: join(qaRoot, 'artifacts'),
    tmpDir: join(qaRoot, 'tmp'),
  }
}

function analyzePdf(pdfPath) {
  const script = [
    'import fitz, json, pathlib, sys',
    'path = pathlib.Path(sys.argv[1])',
    'doc = fitz.open(path)',
    'first = doc[0] if doc.page_count else None',
    'text = first.get_text("text") if first else ""',
    'images = len(first.get_images(full=True)) if first else 0',
    'rect = first.rect if first else None',
    'print(json.dumps({',
    '  "path": str(path),',
    '  "pageCount": doc.page_count,',
    '  "fileSize": path.stat().st_size,',
    '  "firstPageTextLength": len(text.strip()),',
    '  "firstPageImages": images,',
    '  "firstPageWidth": rect.width if rect else 0,',
    '  "firstPageHeight": rect.height if rect else 0,',
    '}))',
  ].join('\n')
  return JSON.parse(run(python, ['-c', script, pdfPath]))
}

function renderFirstPage(pdfPath, pngPath) {
  const script = [
    'import fitz, pathlib, sys',
    'doc = fitz.open(sys.argv[1])',
    'page = doc[0]',
    'pix = page.get_pixmap(matrix=fitz.Matrix(1.5, 1.5), alpha=False)',
    'pathlib.Path(sys.argv[2]).parent.mkdir(parents=True, exist_ok=True)',
    'pix.save(sys.argv[2])',
  ].join('\n')
  run(python, ['-c', script, pdfPath, pngPath])
  assert(existsSync(pngPath), `rendered PNG missing: ${pngPath}`)
  assert(statSync(pngPath).size > 0, `rendered PNG is empty: ${pngPath}`)
}

function writeReport(results) {
  const reportPath = join(qaRoot, 'report.md')
  const lines = [
    '# PDF2ZH Sidecar Sample QA Report',
    '',
    `- Python: \`${python}\``,
    `- QA root: \`${qaRoot}\``,
    `- Generated at: \`${new Date().toISOString()}\``,
    '',
    '| Requirement | Sample | Status | Input pages | Mono pages | Dual pages | Mono render | Dual render | Error |',
    '|---|---|---|---:|---:|---:|---|---|---|',
  ]
  for (const result of results) {
    lines.push([
      result.requirement,
      result.id,
      result.status,
      result.input?.pageCount ?? '',
      result.mono?.pageCount ?? '',
      result.dual?.pageCount ?? '',
      result.monoRender || '',
      result.dualRender || '',
      result.error || '',
    ].join(' | ').replace(/^/, '| ').replace(/$/, ' |'))
  }
  lines.push('')
  lines.push('Notes:')
  lines.push('- This QA verifies translation completion, non-empty artifacts, page count, first-page text/images metadata, and renderability.')
  lines.push('- The 55-page stress sample defaults to priority-page translation. Set `LUMENFOLIO_PDF2ZH_QA_FULL_STRESS=1` to translate all 55 pages as a heavier gate.')
  lines.push('- Human visual acceptance is still required before marking layout-sensitive samples fully DONE.')
  writeFileSync(reportPath, `${lines.join('\n')}\n`)
  return reportPath
}

async function translateSample(sample) {
  assert(existsSync(sample.inputPdfPath), `sample PDF missing: ${sample.inputPdfPath}`)
  const outputDir = join(qaRoot, 'outputs', sample.id)
  mkdirSync(outputDir, { recursive: true })
  const request = {
    id: `qa-${sample.id}`,
    protocolVersion: 'lumenfolio-pdf2zh-v1',
    kind: 'translate',
    payload: {
      ...basePayload(),
      inputPdfPath: sample.inputPdfPath,
      outputDir,
      sourceLang: 'en',
      targetLang: 'zh',
      artifactMode: 'both',
      pages: sample.pages,
      onlyIncludeTranslatedPage: true,
      forceRefresh: true,
      engine: { provider: 'google-web' },
    },
  }
  const run = await runWorker(request, Number(process.env.LUMENFOLIO_PDF2ZH_QA_TIMEOUT_MS || 360_000))
  writeFileSync(join(qaRoot, `${sample.id}.stdout.jsonl`), run.stdout)
  writeFileSync(join(qaRoot, `${sample.id}.stderr.log`), run.stderr)
  const finish = run.events.find((event) => event.event === 'finish')
  const error = run.events.find((event) => event.event === 'error')
  if (!finish) {
    return {
      id: sample.id,
      requirement: sample.requirement,
      status: 'failed',
      error: error?.message || `${sample.id} did not emit finish`,
      errorCode: error?.errorCode || '',
      inputPdfPath: sample.inputPdfPath,
      events: run.events.length,
      input: analyzePdf(sample.inputPdfPath),
    }
  }
  for (const key of ['monoPdfPath', 'dualPdfPath']) {
    assert(finish[key] && existsSync(finish[key]), `${sample.id} missing ${key}`)
    assert(statSync(finish[key]).size > 0, `${sample.id} empty ${key}`)
  }
  const monoRender = join(qaRoot, 'renders', `${sample.id}.mono.png`)
  const dualRender = join(qaRoot, 'renders', `${sample.id}.dual.png`)
  renderFirstPage(finish.monoPdfPath, monoRender)
  renderFirstPage(finish.dualPdfPath, dualRender)
  return {
    id: sample.id,
    requirement: sample.requirement,
    status: 'passed',
    inputPdfPath: sample.inputPdfPath,
    monoPdfPath: finish.monoPdfPath,
    dualPdfPath: finish.dualPdfPath,
    monoRender,
    dualRender,
    events: run.events.length,
    input: analyzePdf(sample.inputPdfPath),
    mono: analyzePdf(finish.monoPdfPath),
    dual: analyzePdf(finish.dualPdfPath),
  }
}

async function main() {
  assert(existsSync(python), `Python runtime missing: ${python}`)
  assert(existsSync(worker), `worker missing: ${worker}`)
  prepareQaDir()
  generateSyntheticSamples()
  const selected = process.env.LUMENFOLIO_PDF2ZH_QA_SAMPLES
    ? new Set(process.env.LUMENFOLIO_PDF2ZH_QA_SAMPLES.split(',').map((item) => item.trim()).filter(Boolean))
    : null
  const availableSamples = selected
    ? [...sampleDefinitions, ...knownRiskSampleDefinitions]
    : sampleDefinitions
  const samples = selected
    ? availableSamples.filter((sample) => selected.has(sample.id))
    : availableSamples
  assert(samples.length > 0, 'no QA samples selected')
  const results = []
  for (const sample of samples) {
    results.push(await translateSample(sample))
  }
  const reportPath = writeReport(results)
  const failures = results.filter((result) => result.status !== 'passed')
  console.log(JSON.stringify({ ok: true, qaRoot, reportPath, samples: results }, null, 2))
  if (failures.length > 0 && process.env.LUMENFOLIO_PDF2ZH_QA_ALLOW_FAILURES !== '1') {
    console.error(`PDF2ZH QA had ${failures.length} failing sample(s). See ${reportPath}`)
    process.exit(1)
  }
}

main().catch((error) => {
  console.error(error.message)
  process.exit(1)
})
