const E2E_ENABLED = import.meta.env.DEV && import.meta.env.VITE_E2E === '1'

function toDataAttrName(key) {
  return `data-${String(key).replace(/[A-Z]/g, (match) => `-${match.toLowerCase()}`)}`
}

export function testAttrs(testId, extra = {}) {
  if (!E2E_ENABLED) return {}
  const attrs = { 'data-testid': testId }
  for (const [key, value] of Object.entries(extra)) {
    if (value === undefined || value === null || value === '') continue
    attrs[toDataAttrName(key)] = String(value)
  }
  return attrs
}
