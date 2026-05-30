// Lightweight localStorage-backed persistence for global UI preferences.
//
// Used for state that should survive an app restart but is purely front-end,
// non-sensitive, and changes often (locale, panel collapse/width, active
// settings section, last-selected document). localStorage is chosen over the
// SQLite app_settings table on purpose: it is synchronous, so the very first
// render already has the saved value and the UI never flashes the default
// layout before snapping to the remembered one.
//
// Per-document state (current page, chat model, translation language) does NOT
// belong here — it lives with the document in SQLite (see PR-2).

import { ref, watch } from 'vue'

const PREFIX = 'lumenfolio.'

/** Read and JSON-parse a persisted value, falling back on any failure. */
export function readPersisted(key, fallback) {
  try {
    const raw = window.localStorage?.getItem(PREFIX + key)
    if (raw == null) return fallback
    return JSON.parse(raw)
  } catch {
    return fallback
  }
}

/** JSON-serialize and store a value; storage failures are ignored. */
export function writePersisted(key, value) {
  try {
    window.localStorage?.setItem(PREFIX + key, JSON.stringify(value))
  } catch {
    // Quota/availability errors are non-fatal: in-session state still works.
  }
}

/**
 * A ref whose value is seeded from localStorage and written back whenever it
 * changes. Writes can be debounced for high-frequency sources (e.g. a width
 * being dragged) so we don't hammer storage on every pixel.
 *
 * @param {string} key - storage key (auto-prefixed with "lumenfolio.")
 * @param {*} defaultValue - used when nothing is stored or parsing fails
 * @param {{ debounceMs?: number }} [options]
 * @returns {import('vue').Ref}
 */
export function usePersistedRef(key, defaultValue, options = {}) {
  const { debounceMs = 0 } = options
  const stored = readPersisted(key, defaultValue)
  const state = ref(stored)

  let timer = 0
  const flush = (value) => writePersisted(key, value)

  watch(state, (value) => {
    if (debounceMs <= 0) {
      flush(value)
      return
    }
    if (timer) window.clearTimeout(timer)
    timer = window.setTimeout(() => {
      timer = 0
      flush(value)
    }, debounceMs)
  }, { deep: false })

  return state
}
