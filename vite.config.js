import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { cpSync, createReadStream, existsSync, statSync } from 'node:fs'
import { dirname, join, normalize, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const devHost = process.env.LUMENFOLIO_DEV_HOST || '127.0.0.1'
const devPort = Number.parseInt(process.env.LUMENFOLIO_DEV_PORT || '1420', 10)
const e2eEnabled = process.env.VITE_E2E === '1'
const pdfjsRoot = dirname(fileURLToPath(import.meta.resolve('pdfjs-dist/package.json')))
const pdfjsAssetBase = '/pdfjs'
const ignoredGeneratedRuntimeGlobs = [
  '**/.venv-pdf2zh/**',
  '**/artifacts/**',
  '**/resources/pdf2zh-sidecar/python/**',
  '**/src-tauri/target/**',
]

function pdfjsStaticAssets() {
  const assetDirs = [
    ['cmaps', 'application/octet-stream'],
    ['standard_fonts', 'font/ttf'],
  ]

  function copyAssets(outDir) {
    for (const [name] of assetDirs) {
      cpSync(join(pdfjsRoot, name), join(outDir, pdfjsAssetBase, name), {
        recursive: true,
        force: true,
      })
    }
  }

  function serveAssetDir(root, defaultType) {
    return (req, res, next) => {
      const rawPath = decodeURIComponent((req.url || '/').split('?')[0])
      const relativePath = normalize(rawPath.replace(/^\/+/, ''))
      const filePath = resolve(root, relativePath)
      const rootPath = resolve(root)
      if (!filePath.startsWith(rootPath + sep) && filePath !== rootPath) {
        res.statusCode = 403
        res.end('Forbidden')
        return
      }
      if (!existsSync(filePath) || !statSync(filePath).isFile()) {
        next()
        return
      }
      res.setHeader('Content-Type', defaultType)
      createReadStream(filePath).pipe(res)
    }
  }

  return {
    name: 'lumenfolio-pdfjs-static-assets',
    configureServer(server) {
      for (const [name, type] of assetDirs) {
        server.middlewares.use(
          `${pdfjsAssetBase}/${name}`,
          serveAssetDir(join(pdfjsRoot, name), type),
        )
      }
    },
    closeBundle() {
      copyAssets(resolve('dist'))
    },
  }
}

export default defineConfig({
  plugins: [vue(), pdfjsStaticAssets()],
  optimizeDeps: {
    entries: ['index.html'],
  },
  resolve: {
    alias: e2eEnabled
      ? {
          '@tauri-apps/api/core': '/src/e2e/tauriCoreMock.js',
        }
      : {},
  },
  server: {
    host: devHost,
    port: Number.isInteger(devPort) ? devPort : 1420,
    strictPort: process.env.LUMENFOLIO_STRICT_PORT === '1',
    watch: {
      ignored: ignoredGeneratedRuntimeGlobs,
    },
  },
  clearScreen: false,
})
