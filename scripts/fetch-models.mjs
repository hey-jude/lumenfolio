// Fetch the ONNX models bundled with the app into `.models/`.
//
// `.models/` is gitignored, so a fresh clone / CI machine has no models. Run
// `npm run fetch:models` to download them. Existing files of the right size are
// skipped, so re-running is cheap and idempotent.
//
// Models:
//   OCR — PP-OCRv4 mobile (RapidOCR / PaddleOCR), Apache-2.0

import { createWriteStream, existsSync, mkdirSync, statSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
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
  console.log(`Done: ${downloaded} downloaded, ${skipped} skipped.`)
  console.log('Note: TSR is optional and is not bundled by this release model fetcher.')
}

main().catch((err) => {
  console.error(`fetch-models failed: ${err.message}`)
  process.exit(1)
})
