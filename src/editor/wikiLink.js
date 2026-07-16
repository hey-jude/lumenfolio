// Milkdown/Crepe support for [[wikilinks]].
//
// Why this exists: remark-stringify escapes square brackets, so a wikilink left as
// plain text round-trips as `\[\[Title]]` — which silently breaks the backend's
// `[[...]]` link extraction the moment the user types anything in live mode. The fix
// is to make the pipeline aware of wikilinks end-to-end: parse them into a real node,
// and serialize that node back to verbatim `[[...]]`.
//
// Parsing deliberately avoids a micromark extension: remark-parse leaves `[[Title]]`
// alone as a plain `text` node, so a post-parse transformer can split those text
// nodes into `wikiLink` nodes. That keeps this small and fully under our control.

import { $node, $remark } from '@milkdown/kit/utils'

// [[Target]] or [[Target|Alias]] — no newlines, no nested brackets.
const WIKI_LINK_PATTERN = '\\[\\[([^\\]\\n]+)\\]\\]'

/** Split "Target|Alias" into its parts (alias is optional). */
export function parseWikiLinkValue(raw) {
  const value = String(raw || '')
  const separator = value.indexOf('|')
  if (separator === -1) return { target: value.trim(), alias: '' }
  return {
    target: value.slice(0, separator).trim(),
    alias: value.slice(separator + 1).trim(),
  }
}

/** Rebuild the raw `Target|Alias` payload from node attrs. */
function toWikiLinkValue(target, alias) {
  const cleanTarget = String(target || '').trim()
  const cleanAlias = String(alias || '').trim()
  return cleanAlias ? `${cleanTarget}|${cleanAlias}` : cleanTarget
}

/** Replace the `[[...]]` runs inside one text value with wikiLink mdast nodes. */
function splitTextNode(value) {
  const matcher = new RegExp(WIKI_LINK_PATTERN, 'g')
  const out = []
  let lastIndex = 0
  let match = matcher.exec(value)
  while (match) {
    if (match.index > lastIndex) {
      out.push({ type: 'text', value: value.slice(lastIndex, match.index) })
    }
    out.push({ type: 'wikiLink', value: match[1] })
    lastIndex = match.index + match[0].length
    match = matcher.exec(value)
  }
  if (!out.length) return null
  if (lastIndex < value.length) {
    out.push({ type: 'text', value: value.slice(lastIndex) })
  }
  return out
}

/** Depth-first walk replacing text children that contain wikilinks. */
function transformTree(node) {
  const children = node?.children
  if (!Array.isArray(children)) return
  let next = null
  for (let index = 0; index < children.length; index += 1) {
    const child = children[index]
    if (child.type === 'text') {
      const replacement = splitTextNode(child.value)
      if (replacement) {
        next = next || children.slice(0, index)
        next.push(...replacement)
        continue
      }
    } else {
      transformTree(child)
    }
    if (next) next.push(child)
  }
  if (next) node.children = next
}

// `function` (not an arrow) on purpose: remark calls plugins with `this` bound to the
// processor, and we need this.data() to register the stringify handler.
// Exported so the markdown round-trip can be tested without booting an editor.
export function remarkWikiLinkPlugin() {
  const data = this.data()
  const toMarkdownExtensions =
    data.toMarkdownExtensions || (data.toMarkdownExtensions = [])
  toMarkdownExtensions.push({
    handlers: {
      // Returned verbatim — mdast-util-to-markdown only escapes text it builds via
      // safe(), so this is exactly how we dodge the `\[\[` escaping.
      wikiLink: (node) => `[[${node.value}]]`,
    },
  })
  return (tree) => {
    transformTree(tree)
  }
}

export const remarkWikiLink = $remark('wikiLink', () => remarkWikiLinkPlugin)

export const wikiLinkNode = $node('wiki_link', () => ({
  group: 'inline',
  inline: true,
  // Atomic: the label is derived from attrs, so the user edits the link as one unit
  // instead of being able to break the `[[`/`]]` fences apart.
  atom: true,
  selectable: true,
  attrs: {
    target: { default: '' },
    alias: { default: '' },
  },
  parseDOM: [
    {
      tag: 'span[data-wiki-link]',
      getAttrs: (dom) => ({
        target: dom.getAttribute('data-target') || '',
        alias: dom.getAttribute('data-alias') || '',
      }),
    },
  ],
  toDOM: (node) => [
    'span',
    {
      'data-wiki-link': '',
      'data-target': node.attrs.target,
      'data-alias': node.attrs.alias,
      class: 'wiki-link',
      title: node.attrs.target,
    },
    node.attrs.alias || node.attrs.target,
  ],
  parseMarkdown: {
    match: (node) => node.type === 'wikiLink',
    runner: (state, node, type) => {
      const { target, alias } = parseWikiLinkValue(node.value)
      state.addNode(type, { target, alias })
    },
  },
  toMarkdown: {
    match: (node) => node.type.name === 'wiki_link',
    runner: (state, node) => {
      state.addNode(
        'wikiLink',
        undefined,
        toWikiLinkValue(node.attrs.target, node.attrs.alias),
      )
    },
  },
}))

/** Everything Crepe needs to round-trip and render wikilinks. */
export const wikiLinkPlugins = [remarkWikiLink, wikiLinkNode].flat()
