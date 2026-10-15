// 宿主入库前已经消毒过正文。沙箱仍只保留白名单标签：阅读器渲染的是
// 它没有自己生成的 HTML，脚本和事件属性不能进这棵树。

const ALLOWED = new Set([
  'p', 'br', 'hr', 'blockquote', 'pre', 'code',
  'em', 'strong', 'b', 'i', 'u', 's', 'del', 'sub', 'sup',
  'span', 'div', 'figure', 'figcaption', 'img',
  'ul', 'ol', 'li', 'dl', 'dt', 'dd',
  'table', 'thead', 'tbody', 'tfoot', 'tr', 'th', 'td', 'caption',
  'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
  'small', 'mark', 'q', 'cite', 'abbr', 'time',
  'details', 'summary', 'section', 'article',
]);

const VOID_TAGS = new Set(['br', 'hr', 'img']);

function safeImageSrc(raw) {
  const value = String(raw || '').trim();
  if (!value || value.length > 200000) return '';
  if (value.startsWith('/api/phantasi/image-cache/')) {
    if (value.includes('..') || value.includes('\\') || value.includes('//')) return '';
    return value.split('?')[0].split('#')[0];
  }
  const lower = value.toLowerCase();
  if (!/^data:image\/(png|jpeg|gif|webp);base64,[a-z0-9+/=\s]+$/i.test(lower)) return '';
  return value;
}

function cleanNode(node) {
  if (node.nodeType === Node.TEXT_NODE) {
    return document.createTextNode(node.textContent || '');
  }
  if (node.nodeType !== Node.ELEMENT_NODE) return null;
  const tag = node.tagName.toLowerCase();
  if (!ALLOWED.has(tag)) {
    const fragment = document.createDocumentFragment();
    for (const child of node.childNodes) {
      const clean = cleanNode(child);
      if (clean) fragment.appendChild(clean);
    }
    return fragment;
  }
  const element = document.createElement(tag);
  if (tag === 'img') {
    const src = safeImageSrc(node.getAttribute('src'));
    if (!src) return null;
    element.setAttribute('src', src);
    const alt = node.getAttribute('alt');
    if (alt) element.setAttribute('alt', alt.slice(0, 300));
  } else if (tag === 'th' || tag === 'td') {
    const span = (name) => {
      const parsed = Number(node.getAttribute(name));
      if (Number.isInteger(parsed) && parsed > 0 && parsed < 50) {
        element.setAttribute(name, String(parsed));
      }
    };
    span('colspan');
    span('rowspan');
  } else if (tag === 'time') {
    const datetime = node.getAttribute('datetime');
    if (datetime && datetime.length < 80 && !/[\s<>"]/.test(datetime)) {
      element.setAttribute('datetime', datetime);
    }
  }
  if (!VOID_TAGS.has(tag)) {
    for (const child of node.childNodes) {
      const clean = cleanNode(child);
      if (clean) element.appendChild(clean);
    }
  }
  return element;
}

function sanitizeContent(unsafeHtml) {
  const parsed = new DOMParser().parseFromString(String(unsafeHtml || ''), 'text/html');
  const fragment = document.createDocumentFragment();
  for (const child of parsed.body.childNodes) {
    const clean = cleanNode(child);
    if (clean) fragment.appendChild(clean);
  }
  return fragment;
}

module.exports = { sanitizeContent };
