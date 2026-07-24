import { reactive } from 'vue'

// Cross-component pointer drag for documents (sidebar → chat composer).
//
// HTML5 drag-and-drop is unusable in this Tauri webview: the native OS file-drop
// handler — which we need so files dragged from Finder import with a real path —
// swallows in-webview drags on macOS. The `drop` event never reaches the element
// and the cursor shows the OS "copy (+)" badge instead. That is why the composer's
// existing `@drop` doc branch had never once fired.
//
// So the sidebar drags with plain mouse events, and any component advertises
// itself as a drop zone by putting `data-doc-drop="<name>"` on an element and
// registering a handler for that name. Hit-testing is done with elementFromPoint
// at mouseup, which the native layer does not intercept.

export const docDrag = reactive({
  active: false,
  docId: '',
  label: '',
})

/** Drop-zone handlers keyed by their `data-doc-drop` value. */
const zoneHandlers = new Map()

/** Register a drop zone; returns an unregister function for onBeforeUnmount. */
export function registerDocDropZone(name, handler) {
  zoneHandlers.set(name, handler)
  return () => {
    if (zoneHandlers.get(name) === handler) zoneHandlers.delete(name)
  }
}

export function beginDocDrag(docId, label) {
  docDrag.active = true
  docDrag.docId = docId
  docDrag.label = label || ''
}

export function endDocDrag() {
  docDrag.active = false
  docDrag.docId = ''
  docDrag.label = ''
}

/** The registered drop zone under the pointer, if any. */
function zoneAt(x, y) {
  if (typeof document === 'undefined') return null
  return document.elementFromPoint(x, y)?.closest?.('[data-doc-drop]') || null
}

/**
 * Hand `docId` to whichever zone is under the pointer. Returns true when a zone
 * accepted it, so the caller can skip its own drop handling.
 */
export function deliverDocDrop(x, y, docId) {
  if (!docId) return false
  const zone = zoneAt(x, y)
  if (!zone) return false
  const handler = zoneHandlers.get(zone.getAttribute('data-doc-drop') || '')
  if (!handler) return false
  handler(docId)
  return true
}
