import net from 'node:net'
import { spawn } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const DEFAULT_HOST = '127.0.0.1'
const DEFAULT_PORT = 1420
const MAX_PORT_SCAN = 80

function parsePort(value, fallback) {
  const parsed = Number.parseInt(String(value || ''), 10)
  if (Number.isInteger(parsed) && parsed > 0 && parsed < 65536) return parsed
  return fallback
}

function isPortAvailable(host, port) {
  return new Promise((resolve) => {
    const server = net.createServer()
    server.unref()
    server.once('error', () => resolve(false))
    server.listen({ host, port }, () => {
      server.close(() => resolve(true))
    })
  })
}

async function findAvailablePort(host, startPort) {
  for (let offset = 0; offset <= MAX_PORT_SCAN; offset += 1) {
    const port = startPort + offset
    if (port >= 65536) break
    if (await isPortAvailable(host, port)) return port
  }
  throw new Error(`No available dev port found from ${startPort} to ${startPort + MAX_PORT_SCAN}`)
}

const host = process.env.LUMENFOLIO_DEV_HOST || DEFAULT_HOST
const startPort = parsePort(process.env.LUMENFOLIO_DEV_PORT, DEFAULT_PORT)
const port = await findAvailablePort(host, startPort)
const devUrl = `http://${host}:${port}`
const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const defaultTsrModel = path.join(rootDir, '.models', 'tsr', 'slanet_1m.onnx')
const tsrFeatureEnabled =
  process.env.LUMENFOLIO_TSR_ONNX === '1' ||
  (process.env.LUMENFOLIO_TSR_ONNX !== '0' && fs.existsSync(defaultTsrModel))

const defaultOcrModel = path.join(rootDir, '.models', 'ocr', 'ch_PP-OCRv4_det_infer.onnx')
const ocrFeatureEnabled =
  process.env.LUMENFOLIO_OCR_ONNX === '1' ||
  (process.env.LUMENFOLIO_OCR_ONNX !== '0' && fs.existsSync(defaultOcrModel))

const configOverride = {
  build: {
    devUrl,
    beforeDevCommand: `npm run dev -- --host ${host} --port ${port} --strictPort`,
  },
}

console.log(`[lumenfolio] using dev server ${devUrl}`)
console.log(`[lumenfolio] Rust TSR ONNX ${tsrFeatureEnabled ? 'enabled' : 'disabled'}`)
console.log(`[lumenfolio] Rust OCR ONNX ${ocrFeatureEnabled ? 'enabled' : 'disabled'}`)

const tauriArgs = ['tauri', 'dev']
const cargoFeatures = [
  ...(tsrFeatureEnabled ? ['tsr-onnx'] : []),
  ...(ocrFeatureEnabled ? ['ocr-onnx'] : []),
]
if (cargoFeatures.length) {
  tauriArgs.push('--features', cargoFeatures.join(','))
}
tauriArgs.push('--config', JSON.stringify(configOverride), ...process.argv.slice(2))

const child = spawn(
  'npx',
  tauriArgs,
  {
    stdio: 'inherit',
    env: {
      ...process.env,
      LUMENFOLIO_DEV_HOST: host,
      LUMENFOLIO_DEV_PORT: String(port),
      LUMENFOLIO_STRICT_PORT: '1',
    },
  },
)

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal)
    return
  }
  process.exit(code ?? 0)
})
