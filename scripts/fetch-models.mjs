// Fetch the native libraries / ONNX models bundled with the app into `.models/`.
//
// `.models/` is gitignored, so a fresh clone / CI machine has no models. Run
// `npm run fetch:models` to download them. Existing files of the right size are
// skipped, so re-running is cheap and idempotent.
//
// Contents:
//   OCR    — PP-OCRv4 mobile (RapidOCR / PaddleOCR), Apache-2.0
//   Pdfium — bblanchon/pdfium-binaries (per-platform native lib). The Rust
//            backend dlopen()s this at index time; without it every document
//            reindex fails and reading silently falls back to PDF.js. The lib is
//            placed at `.models/pdfium/lib/<platform lib name>`, which is exactly
//            where vision::pdfium_dir_from_env_or_default() looks.

import { copyFileSync, createWriteStream, existsSync, mkdirSync, rmSync, statSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { tmpdir } from 'node:os'
import { execFileSync } from 'node:child_process'
import { Readable } from 'node:stream'
import { pipeline } from 'node:stream/promises'

const __dirname = dirname(fileURLToPath(import.meta.url))
const appRoot = resolve(__dirname, '..')
const modelsRoot = join(appRoot, '.models')

// Each entry: destination (relative to .models/), URL, and the expected byte
// size (a loose floor — we re-download anything materially smaller, which
// catches truncated/HTML-error downloads).
const FILES = [
  {
    dest: 'ocr/ch_PP-OCRv4_det_infer.onnx',
    url: 'https://huggingface.co/SWHL/RapidOCR/resolve/main/PP-OCRv4/ch_PP-OCRv4_det_infer.onnx',
    minBytes: 4_500_000,
  },
  {
    dest: 'ocr/ch_PP-OCRv4_rec_infer.onnx',
    url: 'https://huggingface.co/SWHL/RapidOCR/resolve/main/PP-OCRv4/ch_PP-OCRv4_rec_infer.onnx',
    minBytes: 10_000_000,
  },
  {
    dest: 'ocr/ppocr_keys_v1.txt',
    url: 'https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/ppocr/utils/ppocr_keys_v1.txt',
    minBytes: 20_000,
  },
  {
    dest: 'ocr/LICENSE-Apache-2.0.txt',
    url: 'https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/LICENSE',
    minBytes: 10_000,
  },
]

// Pdfium ships as a per-platform tarball. Pick the asset + the library member to
// extract, and the canonical name pdfium-render expects inside `.models/pdfium/lib/`.
const PDFIUM_RELEASE =
  'https://github.com/bblanchon/pdfium-binaries/releases/latest/download'
const PDFIUM_TARGETS = {
  'darwin-arm64': { asset: 'pdfium-mac-arm64.tgz', member: 'lib/libpdfium.dylib', libName: 'libpdfium.dylib' },
  'darwin-x64': { asset: 'pdfium-mac-x64.tgz', member: 'lib/libpdfium.dylib', libName: 'libpdfium.dylib' },
  'linux-x64': { asset: 'pdfium-linux-x64.tgz', member: 'lib/libpdfium.so', libName: 'libpdfium.so' },
  'linux-arm64': { asset: 'pdfium-linux-arm64.tgz', member: 'lib/libpdfium.so', libName: 'libpdfium.so' },
  'win32-x64': { asset: 'pdfium-win-x64.tgz', member: 'bin/pdfium.dll', libName: 'pdfium.dll' },
  'win32-arm64': { asset: 'pdfium-win-arm64.tgz', member: 'bin/pdfium.dll', libName: 'pdfium.dll' },
}
const PDFIUM_MIN_BYTES = 2_000_000

function isPresent(absPath, minBytes) {
  if (!existsSync(absPath)) return false
  try {
    return statSync(absPath).size >= minBytes
  } catch {
    return false
  }
}

async function download(url, absPath) {
  mkdirSync(dirname(absPath), { recursive: true })
  const response = await fetch(url, { redirect: 'follow' })
  if (!response.ok || !response.body) {
    throw new Error(`HTTP ${response.status} for ${url}`)
  }
  await pipeline(Readable.fromWeb(response.body), createWriteStream(absPath))
}

// Download the platform pdfium tarball and extract just the native library into
// `.models/pdfium/lib/`. Returns 'downloaded' | 'skipped'.
async function fetchPdfium() {
  const key = `${process.platform}-${process.arch}`
  const target = PDFIUM_TARGETS[key]
  if (!target) {
    console.log(`  skip  pdfium (no prebuilt binary mapped for ${key})`)
    return 'skipped'
  }
  const destPath = join(modelsRoot, 'pdfium', 'lib', target.libName)
  if (isPresent(destPath, PDFIUM_MIN_BYTES)) {
    console.log(`  skip  pdfium/lib/${target.libName} (already present)`)
    return 'skipped'
  }
  process.stdout.write(`  get   pdfium/lib/${target.libName} ... `)
  const tmpDir = join(tmpdir(), `lumenfolio-pdfium-${process.pid}`)
  const tgzPath = join(tmpDir, target.asset)
  mkdirSync(tmpDir, { recursive: true })
  try {
    await download(`${PDFIUM_RELEASE}/${target.asset}`, tgzPath)
    // tar is available on macOS, Linux, and Windows 10+ (bsdtar).
    execFileSync('tar', ['-xzf', tgzPath, '-C', tmpDir, target.member])
    mkdirSync(dirname(destPath), { recursive: true })
    copyFileSync(join(tmpDir, target.member), destPath)
  } finally {
    rmSync(tmpDir, { recursive: true, force: true })
  }
  const size = statSync(destPath).size
  if (size < PDFIUM_MIN_BYTES) {
    throw new Error(
      `Extracted pdfium is only ${size} bytes (expected >= ${PDFIUM_MIN_BYTES}); the source may have moved.`,
    )
  }
  console.log(`${(size / 1_000_000).toFixed(1)} MB`)
  return 'downloaded'
}

async function main() {
  console.log(`Fetching bundled models into ${modelsRoot}`)
  let downloaded = 0
  let skipped = 0
  for (const file of FILES) {
    const absPath = join(modelsRoot, file.dest)
    if (isPresent(absPath, file.minBytes)) {
      console.log(`  skip  ${file.dest} (already present)`)
      skipped += 1
      continue
    }
    process.stdout.write(`  get   ${file.dest} ... `)
    await download(file.url, absPath)
    const size = statSync(absPath).size
    if (size < file.minBytes) {
      throw new Error(
        `Downloaded ${file.dest} is only ${size} bytes (expected >= ${file.minBytes}); the source may have moved.`,
      )
    }
    console.log(`${(size / 1_000_000).toFixed(1)} MB`)
    downloaded += 1
  }
  if ((await fetchPdfium()) === 'downloaded') downloaded += 1
  else skipped += 1
  console.log(`Done: ${downloaded} downloaded, ${skipped} skipped.`)
  console.log('Note: TSR is optional and is not bundled by this release model fetcher.')
}

main().catch((err) => {
  console.error(`fetch-models failed: ${err.message}`)
  process.exit(1)
})
