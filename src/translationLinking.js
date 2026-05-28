export function buildLinkedTranslationBlocks({
  viewMode,
  canOpenTranslationView,
  pageNo,
  blocks = [],
}) {
  if (!(viewMode === 'dual' && canOpenTranslationView)) return []
  return blocks
    .filter((block) => block?.blockId && block.bboxList?.length)
    .map((block) => ({
      page: pageNo,
      blockId: block.blockId,
      bboxList: block.bboxList,
    }))
}

export function buildLinkedBlocksByPage(blocks = []) {
  const next = new Map()
  for (const block of blocks || []) {
    const page = Number(block?.page || block?.pageNo)
    const blockId = String(block?.blockId || block?.id || '')
    if (!Number.isFinite(page) || page <= 0 || !blockId || !block?.bboxList?.length) continue
    const pageBlocks = next.get(page) || []
    pageBlocks.push({
      page,
      blockId,
      bboxList: block.bboxList,
    })
    next.set(page, pageBlocks)
  }
  return next
}

export function canUseLinkedHover({
  linkedHoverEnabled,
  selectionLocked,
  isSelectingText,
  hasSelectionText,
  translationPreviewOpen,
  hasActiveTranslation,
} = {}) {
  return Boolean(linkedHoverEnabled)
    && !selectionLocked
    && !isSelectingText
    && !hasSelectionText
    && !translationPreviewOpen
    && !hasActiveTranslation
}

export function createTranslatedBlockSelection(pageNo, block = {}) {
  const bboxList = block.bboxList || []
  return {
    page: pageNo,
    blockId: block.blockId || '',
    source: {
      bboxList,
      clearHighlight: !bboxList.length,
    },
  }
}

export function isLinkedBlockHovered(hoveredBlock, pageNo, blockId) {
  return hoveredBlock?.page === pageNo && hoveredBlock?.blockId === blockId
}

export function normalizeLinkedBlockHover(block = null) {
  return block?.blockId
    ? {
      page: block.page,
      blockId: block.blockId,
    }
    : null
}
