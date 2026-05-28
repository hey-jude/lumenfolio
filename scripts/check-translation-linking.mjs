import assert from 'node:assert/strict'
import {
  buildLinkedTranslationBlocks,
  buildLinkedBlocksByPage,
  canUseLinkedHover,
  createTranslatedBlockSelection,
  isLinkedBlockHovered,
  normalizeLinkedBlockHover,
} from '../src/translationLinking.js'

const blocks = [
  { blockId: 'b1', bboxList: [[0.1, 0.2, 0.3, 0.4]], translatedText: '一' },
  { blockId: 'b2', bboxList: [], translatedText: '二' },
  { bboxList: [[0, 0, 1, 1]], translatedText: 'missing id' },
]

assert.deepEqual(
  buildLinkedTranslationBlocks({
    viewMode: 'original',
    canOpenTranslationView: true,
    pageNo: 4,
    blocks,
  }),
  [],
)

assert.deepEqual(
  buildLinkedTranslationBlocks({
    viewMode: 'dual',
    canOpenTranslationView: false,
    pageNo: 4,
    blocks,
  }),
  [],
)

assert.deepEqual(
  buildLinkedTranslationBlocks({
    viewMode: 'dual',
    canOpenTranslationView: true,
    pageNo: 4,
    blocks,
  }),
  [
    {
      page: 4,
      blockId: 'b1',
      bboxList: [[0.1, 0.2, 0.3, 0.4]],
    },
  ],
)

assert.deepEqual(
  [...buildLinkedBlocksByPage([
    { page: 4, blockId: 'b1', bboxList: [[0.1, 0.2, 0.3, 0.4]] },
    { pageNo: 4, id: 'legacy-b2', bboxList: [[0.2, 0.3, 0.4, 0.5]] },
    { page: 5, blockId: 'empty-bbox', bboxList: [] },
    { page: 0, blockId: 'bad-page', bboxList: [[0, 0, 1, 1]] },
  ]).entries()],
  [
    [4, [
      { page: 4, blockId: 'b1', bboxList: [[0.1, 0.2, 0.3, 0.4]] },
      { page: 4, blockId: 'legacy-b2', bboxList: [[0.2, 0.3, 0.4, 0.5]] },
    ]],
  ],
)

assert.equal(canUseLinkedHover({ linkedHoverEnabled: true }), true)
assert.equal(canUseLinkedHover({ linkedHoverEnabled: false }), false)
assert.equal(canUseLinkedHover({ linkedHoverEnabled: true, selectionLocked: true }), false)
assert.equal(canUseLinkedHover({ linkedHoverEnabled: true, isSelectingText: true }), false)
assert.equal(canUseLinkedHover({ linkedHoverEnabled: true, hasSelectionText: true }), false)
assert.equal(canUseLinkedHover({ linkedHoverEnabled: true, translationPreviewOpen: true }), false)
assert.equal(canUseLinkedHover({ linkedHoverEnabled: true, hasActiveTranslation: true }), false)

assert.deepEqual(
  createTranslatedBlockSelection(4, blocks[0]),
  {
    page: 4,
    blockId: 'b1',
    source: {
      bboxList: [[0.1, 0.2, 0.3, 0.4]],
      clearHighlight: false,
    },
  },
)

assert.deepEqual(
  createTranslatedBlockSelection(4, blocks[1]),
  {
    page: 4,
    blockId: 'b2',
    source: {
      bboxList: [],
      clearHighlight: true,
    },
  },
)

assert.equal(isLinkedBlockHovered({ page: 4, blockId: 'b1' }, 4, 'b1'), true)
assert.equal(isLinkedBlockHovered({ page: 4, blockId: 'b1' }, 5, 'b1'), false)
assert.deepEqual(
  normalizeLinkedBlockHover({ page: 4, blockId: 'b1', bboxList: [[0, 0, 1, 1]] }),
  { page: 4, blockId: 'b1' },
)
assert.equal(normalizeLinkedBlockHover(null), null)

assert.deepEqual(
  createTranslatedBlockSelection(4, {
    blockId: 'pdf-b1',
    bboxList: [[0.1, 0.2, 0.3, 0.4]],
    renderedBboxList: [[0.5, 0.6, 0.7, 0.8]],
  }),
  {
    page: 4,
    blockId: 'pdf-b1',
    source: {
      bboxList: [[0.1, 0.2, 0.3, 0.4]],
      clearHighlight: false,
    },
  },
)
