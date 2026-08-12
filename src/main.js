import { createApp } from 'vue'
import App from './App.vue'
import './styles/main.css'

performance.mark?.('lumenfolio:main-start')

const params = new URLSearchParams(window.location.search)
const harnessName = params.get('harness') || ''
const useE2eHarness = import.meta.env.DEV
  && import.meta.env.VITE_E2E === '1'
  && ['translation-linking', 'pdf-sidecar', 'pdf-annotation'].includes(harnessName)

async function resolveRootComponent() {
  if (!useE2eHarness) return App
  const harnesses = {
    'translation-linking': () => import('./e2e/TranslationLinkingHarness.vue'),
    'pdf-sidecar': () => import('./e2e/PdfSidecarHarness.vue'),
    'pdf-annotation': () => import('./e2e/PdfAnnotationHarness.vue'),
  }
  const { default: Harness } = await harnesses[harnessName]()
  return Harness
}

resolveRootComponent().then((Root) => {
  performance.mark?.('lumenfolio:root-resolved')
  createApp(Root).mount('#app')
  performance.mark?.('lumenfolio:vue-mounted')
  window.requestAnimationFrame(() => {
    performance.mark?.('lumenfolio:first-frame')
  })
})
