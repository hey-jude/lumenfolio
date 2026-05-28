import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const appRoot = dirname(dirname(fileURLToPath(import.meta.url)))
const resourceRoot = join(appRoot, 'resources', 'pdf2zh-sidecar')
const manifestPath = join(resourceRoot, 'runtime-manifest.json')
const workerPath = join(resourceRoot, 'worker', 'lumenfolio_pdf2zh_worker.py')
const tauriConfigPath = join(appRoot, 'src-tauri', 'tauri.conf.json')

function fail(message) {
  console.error(message)
  process.exit(1)
}

function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, 'utf8'))
  } catch (error) {
    fail(`Failed to parse JSON ${path}: ${error.message}`)
  }
}

function assert(condition, message) {
  if (!condition) fail(message)
}

function bundledResources(config) {
  const resources = config?.bundle?.resources || []
  if (Array.isArray(resources)) return resources
  if (resources && typeof resources === 'object') return Object.keys(resources)
  return []
}

assert(existsSync(manifestPath), `Missing PDF2ZH runtime manifest: ${manifestPath}`)
assert(existsSync(workerPath), `Missing PDF2ZH worker script: ${workerPath}`)

const manifest = readJson(manifestPath)
const tauriConfig = readJson(tauriConfigPath)
const resources = bundledResources(tauriConfig)
const hasSidecarResourceDir = resources.includes('../resources/pdf2zh-sidecar')

assert(manifest.protocolVersion === 'lumenfolio-pdf2zh-v1', 'Unexpected PDF2ZH protocol version')
assert(manifest.worker === 'worker/lumenfolio_pdf2zh_worker.py', 'Runtime manifest worker path is out of sync')
assert(
  hasSidecarResourceDir || resources.includes('../resources/pdf2zh-sidecar/runtime-manifest.json'),
  'Tauri bundle resources omit runtime manifest',
)
assert(
  hasSidecarResourceDir || resources.includes('../resources/pdf2zh-sidecar/worker/lumenfolio_pdf2zh_worker.py'),
  'Tauri bundle resources omit worker script',
)

const macPython = join(resourceRoot, 'python', 'bin', 'python3')
const windowsPython = join(resourceRoot, 'python', 'python.exe')
const frozenMac = join(resourceRoot, 'pdf2zh-sidecar')
const frozenWindows = join(resourceRoot, 'pdf2zh-sidecar.exe')
const hasAnyReleaseRuntime = [macPython, windowsPython, frozenMac, frozenWindows].some((path) => existsSync(path))

console.log(JSON.stringify({
  ok: true,
  manifest: manifestPath,
  worker: workerPath,
  bundleResources: resources.filter((resource) => resource.includes('pdf2zh-sidecar')),
  releaseRuntimePresent: hasAnyReleaseRuntime,
  releaseRuntimeCandidates: {
    macPython,
    windowsPython,
    frozenMac,
    frozenWindows,
  },
}, null, 2))

if (process.env.LUMENFOLIO_PDF2ZH_REQUIRE_RELEASE_RUNTIME === '1') {
  assert(hasAnyReleaseRuntime, 'No bundled PDF2ZH release runtime found')
}
