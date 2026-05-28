import { getCurrentWindow } from '@tauri-apps/api/window'

const INTERACTIVE_SELECTOR = 'button,input,textarea,select,a,[role="button"]'

export function startWindowDrag(event) {
  if (event.button !== 0) return
  if (!window.__TAURI_INTERNALS__?.metadata?.currentWindow) return

  const target = event.target
  if (target?.closest?.(INTERACTIVE_SELECTOR)) return

  void getCurrentWindow().startDragging().catch(() => {})
}
