const api = require('./api.js');
const { sanitizeContent } = require('./sanitize.js');

const PAGE_SIZE = 24;
const PREFS_KEY = 'rss.reader.prefs';

function t(key, variables) {
  return Tapp.i18n.t(key, variables || {});
}

function granted(name) {
  const list = Array.isArray(Tapp.permissions) ? Tapp.permissions : [];
  if (list.includes(name)) return true;
  try {
    const info = Tapp.lifecycle.getInfo();
    return Array.isArray(info.permissions) && info.permissions.includes(name);
  } catch {
    return false;
  }
}

function canStore() {
  return granted('storage:read') || granted('storage:write');
}

function categoryTokens(value) {
  return String(value || '')
    .split(',')
    .map((part) => part.trim())
    .filter(Boolean);
}

function isFeed(source) {
  return source && source.source_type !== 'note' && source.source_type !== 'link';
}

function formatWhen(value, style) {
  if (value == null || value === '') return '';
  const numeric = typeof value === 'number' ? value : Number(value);
  const date = Number.isFinite(numeric)
    ? new Date(numeric > 1e12 ? numeric : numeric * 1000)
    : new Date(value);
  if (Number.isNaN(date.getTime())) return '';
  try {
    const locale = Tapp.i18n.getLocale();
    if (style === 'long') {
      return new Intl.DateTimeFormat(locale, {
        year: 'numeric',
        month: 'short',
        day: 'numeric',
        hour: 'numeric',
        minute: '2-digit',
      }).format(date);
    }
    const delta = Date.now() - date.getTime();
    const minutes = Math.round(delta / 60000);
    if (Math.abs(minutes) < 1) return t('justNow');
    const rel = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
    if (Math.abs(minutes) < 60) return rel.format(-minutes, 'minute');
    const hours = Math.round(minutes / 60);
    if (Math.abs(hours) < 24) return rel.format(-hours, 'hour');
    const days = Math.round(hours / 24);
    if (Math.abs(days) < 14) return rel.format(-days, 'day');
    return new Intl.DateTimeFormat(locale, { month: 'short', day: 'numeric' }).format(date);
  } catch {
    return '';
  }
}

function localMedia(url) {
  const value = String(url || '').trim();
  if (!value) return '';
  if (value.startsWith('/api/') || value.startsWith('/media/')) {
    if (value.includes('..') || value.includes('\\') || value.includes('//')) return '';
    return value.split('?')[0].split('#')[0];
  }
  if (/^data:image\/(png|jpeg|gif|webp);base64,/i.test(value)) return value;
  return '';
}

function monogram(name) {
  const text = String(name || '').trim();
  if (!text) return '·';
  return Array.from(text)[0].toUpperCase();
}

function excerptOf(item) {
  return String(item && (item.summary || item.title) || '')
    .replace(/<[^>]+>/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, 160);
}

function el(tag, attrs, text) {
  const node = document.createElement(tag);
  if (attrs) {
    for (const [key, value] of Object.entries(attrs)) {
      if (value == null || value === false) continue;
      if (key === 'className') node.className = value;
      else if (key === 'hidden') node.hidden = true;
      else node.setAttribute(key, String(value));
    }
  }
  if (text != null) node.textContent = text;
  return node;
}

function icon(path) {
  const svg = document.createElementNS
    ? document.createElementNS('http://www.w3.org/2000/svg', 'svg')
    : el('span');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('aria-hidden', 'true');
  if (svg.tagName && svg.tagName.toLowerCase() === 'svg') {
    const drawn = document.createElementNS('http://www.w3.org/2000/svg', 'path');
    drawn.setAttribute('fill', 'none');
    drawn.setAttribute('stroke', 'currentColor');
    drawn.setAttribute('stroke-width', '1.8');
    drawn.setAttribute('stroke-linecap', 'round');
    drawn.setAttribute('stroke-linejoin', 'round');
    drawn.setAttribute('d', path);
    svg.appendChild(drawn);
  }
  return svg;
}

function flag(node, name, on) {
  if (!node) return;
  const current = String(node.className || '').split(/\s+/).filter(Boolean);
  const next = current.filter((item) => item !== name);
  if (on) next.push(name);
  node.className = next.join(' ');
}

function start(root) {
  const state = {
    sources: [],
    categories: [],
    stats: null,
    category: '',
    sourceId: null,
    filter: 'unread',
    query: '',
    items: [],
    nextCursor: null,
    loading: false,
    article: null,
    reading: false,
    navOpen: false,
    manage: false,
    pendingDelete: null,
    status: '',
    focusId: null,
    canWrite: granted('phantasi:write'),
    canManage: granted('phantasi:manage'),
  };

  root.replaceChildren();
  root.append(
    buildBar(state),
    buildShell(state),
    buildSheet(state),
    el('button', { type: 'button', className: 'rss-veil', 'data-action': 'close-manage', hidden: true, 'aria-label': 'close' }),
    el('p', { className: 'rss-status', 'data-role': 'status', role: 'status' }),
  );
  root.addEventListener('click', (event) => onClick(root, state, event));
  root.addEventListener('change', (event) => onChange(root, state, event));
  root.addEventListener('input', (event) => onInput(root, state, event));
  if (typeof document.addEventListener === 'function') {
    document.addEventListener('keydown', (event) => onKey(root, state, event));
  }
  if (typeof Tapp.ui.onLocaleChange === 'function') {
    Tapp.ui.onLocaleChange(() => {
      applyStaticText(root, state);
      paint(root, state);
    });
  }
  applyStaticText(root, state);
  void Tapp.ui.setTitle(t('title')).catch(() => {});
  void boot(root, state);
}

function buildBar(state) {
  const bar = el('header', { className: 'rss-bar' });
  const brand = el('div', { className: 'rss-brand' });
  const navBtn = el('button', {
    type: 'button',
    className: 'rss-iconbtn',
    'data-action': 'toggle-nav',
    'data-i18n-aria': 'feeds',
  });
  navBtn.appendChild(icon('M4 7h16M4 12h16M4 17h16'));
  brand.append(
    navBtn,
    el('h1', { 'data-i18n': 'title' }),
    el('p', { className: 'rss-stats', 'data-role': 'stats' }),
  );
  const search = el('input', {
    type: 'search',
    className: 'rss-search',
    'data-field': 'search',
    'data-i18n-placeholder': 'search',
    autocomplete: 'off',
  });
  const actions = el('div', { className: 'rss-actions' });
  if (state.canWrite) {
    actions.append(textBtn('mark-all', 'markAllRead'));
  }
  actions.append(textBtn('export-opml', 'exportOpml'));
  if (state.canManage) {
    actions.append(textBtn('toggle-manage', 'manage', 'manage-toggle'));
  }
  bar.append(brand, search, actions);
  return bar;
}

function buildShell() {
  const shell = el('div', { className: 'rss-shell', 'data-role': 'columns' });
  const nav = el('nav', { className: 'rss-nav', 'data-role': 'sources', 'aria-label': 'feeds' });
  nav.append(el('div', { 'data-role': 'source-list' }));
  const stream = el('section', { className: 'rss-stream', 'data-role': 'items' });
  const head = el('div', { className: 'rss-stream__head' });
  head.append(el('h2', { 'data-role': 'stream-title' }), el('p', { 'data-role': 'stream-hint' }));
  stream.append(head, el('div', { 'data-role': 'item-list' }));
  const reader = el('article', { className: 'rss-read', 'data-role': 'reader' });
  reader.append(el('div', { 'data-role': 'reader-body' }));
  shell.append(nav, stream, reader);
  return shell;
}

function buildSheet(state) {
  const sheet = el('section', { className: 'rss-sheet', 'data-role': 'manage', hidden: true });
  if (!state.canManage) return sheet;
  const head = el('div', { className: 'rss-row' });
  const close = el('button', { type: 'button', className: 'rss-iconbtn', 'data-action': 'toggle-manage', 'data-i18n-aria': 'closeManage' });
  close.appendChild(icon('M6 6l12 12M18 6L6 18'));
  head.append(el('h2', { 'data-i18n': 'manage' }), close);
  sheet.append(
    head,
    el('h2', { 'data-i18n': 'addSource' }),
    formRow([
      el('input', { type: 'url', 'data-field': 'add-url', 'data-i18n-placeholder': 'feedUrl' }),
      textBtn('discover', 'discover'),
    ]),
    formRow([
      el('input', { type: 'text', 'data-field': 'add-name', 'data-i18n-placeholder': 'sourceName' }),
      el('input', { type: 'text', 'data-field': 'add-category', 'data-i18n-placeholder': 'category' }),
      textBtn('add-source', 'add', null, true),
    ]),
    el('h2', { 'data-i18n': 'sources' }),
    textBtn('refresh-all', 'refreshAll'),
    el('div', { 'data-role': 'editor' }),
    el('h2', { 'data-i18n': 'categories' }),
    formRow([
      el('input', { type: 'text', 'data-field': 'new-category', 'data-i18n-placeholder': 'newCategory' }),
      textBtn('add-category', 'addCategory'),
    ]),
    el('div', { 'data-role': 'category-admin' }),
  );
  const importRow = el('label', { className: 'rss-row' });
  importRow.append(
    el('span', { 'data-i18n': 'importOpml' }),
    el('input', { type: 'file', accept: '.opml,.xml,text/xml,text/x-opml', 'data-action': 'import-opml' }),
  );
  sheet.append(importRow);
  return sheet;
}

function textBtn(action, key, role, accent) {
  return el('button', {
    type: 'button',
    className: accent ? 'rss-textbtn is-accent' : 'rss-textbtn',
    'data-action': action,
    'data-role': role || undefined,
    'data-i18n': key,
  });
}

function formRow(children) {
  const row = el('div', { className: 'rss-form' });
  row.append(...children.filter(Boolean));
  return row;
}

function applyStaticText(root, state) {
  root.querySelectorAll('[data-i18n]').forEach((node) => {
    node.textContent = t(node.getAttribute('data-i18n'));
  });
  root.querySelectorAll('[data-i18n-placeholder]').forEach((node) => {
    node.setAttribute('placeholder', t(node.getAttribute('data-i18n-placeholder')));
  });
  root.querySelectorAll('[data-i18n-aria]').forEach((node) => {
    node.setAttribute('aria-label', t(node.getAttribute('data-i18n-aria')));
  });
  const toggle = root.querySelector('[data-action="toggle-manage"]');
  if (toggle && toggle.getAttribute('data-role') === 'manage-toggle') {
    toggle.textContent = state.manage ? t('closeManage') : t('manage');
  }
  const manage = root.querySelector('[data-role="manage"]');
  if (manage) manage.hidden = !state.manage;
  const veil = root.querySelector('.rss-veil');
  if (veil) veil.hidden = !state.manage;
  if (document.documentElement) document.documentElement.lang = Tapp.i18n.getLocale() || '';
}

function paint(root, state) {
  const stats = root.querySelector('[data-role="stats"]');
  if (stats) {
    stats.textContent = state.stats
      ? t('stats', {
        unread: state.stats.total_unread ?? 0,
        sources: visibleSources(state, true).length,
      })
      : '';
  }
  paintSources(root, state);
  paintItems(root, state);
  paintReader(root, state);
  paintEditor(root, state);
  paintCategoryAdmin(root, state);
  const status = root.querySelector('[data-role="status"]');
  if (status) status.textContent = state.status;
  flag(root, 'is-reading', state.reading);
  flag(root, 'is-nav', state.navOpen && !state.reading);
  applyStaticText(root, state);
}

function visibleSources(state, all) {
  return state.sources.filter((source) => {
    if (!isFeed(source)) return false;
    if (all) return true;
    const tokens = categoryTokens(source.category);
    if (state.category === '__none__') return tokens.length === 0;
    if (!state.category) return true;
    return tokens.includes(state.category);
  });
}

function paintSources(root, state) {
  const host = root.querySelector('[data-role="source-list"]');
  if (!host) return;
  const names = new Set();
  let hasEmpty = false;
  for (const source of state.sources) {
    if (!isFeed(source)) continue;
    const tokens = categoryTokens(source.category);
    if (tokens.length === 0) hasEmpty = true;
    for (const token of tokens) names.add(token);
  }
  for (const category of state.categories) {
    if (category && category.name) names.add(category.name);
  }

  const list = el('div');
  list.append(el('p', { className: 'rss-label' }, t('library')));
  list.append(folderBtn('all', '', null, t('filterAll'), state));
  list.append(folderBtn('unread', '', null, t('filterUnread'), state, state.stats && state.stats.total_unread));
  list.append(folderBtn('starred', '', null, t('filterStarred'), state, state.stats && state.stats.total_starred));
  list.append(el('p', { className: 'rss-label' }, t('sources')));
  list.append(folderBtn(state.filter, '', null, t('allSources'), state));
  if (hasEmpty) list.append(folderBtn(state.filter, '__none__', null, t('uncategorized'), state));
  for (const name of [...names].sort((a, b) => a.localeCompare(b))) {
    list.append(folderBtn(state.filter, name, null, name, state));
    for (const source of state.sources.filter((item) => isFeed(item) && categoryTokens(item.category).includes(name))) {
      list.append(sourceBtn(source, state));
    }
  }
  const leftover = visibleSources(state).filter((source) => categoryTokens(source.category).length === 0);
  if (leftover.length && (state.category === '' || state.category === '__none__')) {
    for (const source of leftover) list.append(sourceBtn(source, state));
  }
  if (visibleSources(state, true).length === 0) {
    list.append(el('p', { className: 'rss-empty' }, t('emptySources')));
  }
  host.replaceChildren(list);
}

function folderBtn(filter, category, sourceId, label, state, count) {
  const on = state.filter === filter && state.category === category && state.sourceId === sourceId;
  const button = el('button', {
    type: 'button',
    className: 'rss-navbtn',
    'data-action': sourceId ? 'source' : category ? 'category' : 'folder',
    'data-filter': filter,
    'data-category': category,
    'data-id': sourceId || undefined,
    'aria-pressed': on ? 'true' : 'false',
  });
  button.append(el('span', { className: 'rss-navbtn__name' }, label));
  if (count) button.append(el('span', { className: 'rss-count' }, String(count)));
  return button;
}

function sourceBtn(source, state) {
  const button = el('button', {
    type: 'button',
    className: 'rss-navbtn',
    'data-action': 'source',
    'data-id': source.id,
    'aria-pressed': state.sourceId === source.id ? 'true' : 'false',
  });
  const iconSrc = localMedia(source.icon);
  if (iconSrc) button.append(el('img', { className: 'rss-thumb', src: iconSrc, alt: '' }));
  else button.append(el('span', { className: 'rss-mono' }, monogram(source.name || source.url)));
  button.append(el('span', { className: 'rss-navbtn__name' }, source.name || source.url || String(source.id)));
  if (source.unread_count) {
    button.append(el('span', { className: 'rss-count' }, String(source.unread_count)));
  }
  return button;
}

function streamTitle(state) {
  if (state.filter === 'unread') return t('filterUnread');
  if (state.filter === 'starred') return t('filterStarred');
  if (state.sourceId) {
    const source = state.sources.find((item) => item.id === state.sourceId);
    return (source && (source.name || source.url)) || t('articles');
  }
  if (state.category === '__none__') return t('uncategorized');
  if (state.category) return state.category;
  return t('filterAll');
}

function paintItems(root, state) {
  const host = root.querySelector('[data-role="item-list"]');
  const title = root.querySelector('[data-role="stream-title"]');
  const hint = root.querySelector('[data-role="stream-hint"]');
  if (title) title.textContent = streamTitle(state);
  if (hint) {
    hint.textContent = state.loading
      ? t('loading')
      : t('streamHint', { count: shownItems(state).length });
  }
  if (!host) return;
  const list = el('div');
  const items = shownItems(state);
  if (items.length === 0) {
    list.append(emptyState(
      state.loading ? t('loading') : t('emptyItems'),
      state.canManage ? t('emptyItemsHint') : t('emptyArticleHint'),
    ));
  }
  for (const item of items) {
    const button = el('button', {
      type: 'button',
      className: item.is_read ? 'rss-item' : 'rss-item is-unread',
      'data-action': 'open',
      'data-id': item.id,
    });
    if (state.article && state.article.id === item.id) flag(button, 'is-open', true);
    const meta = el('div', { className: 'rss-item__meta' });
    if (!item.is_read) meta.append(el('span', { className: 'rss-dot' }));
    if (item.source_name) meta.append(el('span', null, item.source_name));
    const when = formatWhen(item.published_at);
    if (when) meta.append(el('span', null, when));
    if (item.is_starred) meta.append(el('span', null, '★'));
    button.append(el('strong', { className: 'rss-item__title' }, item.title || t('untitled')));
    button.append(meta);
    const excerpt = excerptOf(item);
    if (excerpt && excerpt !== item.title) {
      button.append(el('span', { className: 'rss-excerpt' }, excerpt));
    }
    list.append(button);
  }
  if (state.nextCursor) {
    const more = el('button', { type: 'button', className: 'rss-textbtn rss-more', 'data-action': 'more' }, t('loadMore'));
    list.append(more);
  }
  host.replaceChildren(list);
}

function shownItems(state) {
  const query = state.query.trim().toLowerCase();
  if (!query) return state.items;
  return state.items.filter((item) => {
    const hay = `${item.title || ''} ${item.source_name || ''} ${item.summary || ''}`.toLowerCase();
    return hay.includes(query);
  });
}

function emptyState(title, hint) {
  const box = el('div', { className: 'rss-empty' });
  box.append(el('strong', null, title));
  if (hint) box.append(el('span', null, hint));
  return box;
}

function paintReader(root, state) {
  const host = root.querySelector('[data-role="reader-body"]');
  if (!host) return;
  const article = state.article;
  if (!article) {
    const box = emptyState(t('emptyArticle'), t('emptyArticleHint'));
    const keys = el('p');
    keys.append(
      el('kbd', null, 'J'),
      document.createTextNode(' / '),
      el('kbd', null, 'K'),
      document.createTextNode(` ${t('shortcutMove')} · `),
      el('kbd', null, 'O'),
      document.createTextNode(` ${t('openOriginal')}`),
    );
    box.append(keys);
    host.replaceChildren(box);
    return;
  }
  const body = el('div');
  const toolbar = el('div', { className: 'rss-read__toolbar' });
  toolbar.append(iconBtn('back', 'back', 'M15 6l-6 6 6 6'));
  if (article.link) toolbar.append(textBtn('open-original', 'openOriginal'));
  if (state.canWrite) {
    toolbar.append(
      textBtn(article.is_read ? 'unread' : 'read', article.is_read ? 'markUnread' : 'markRead'),
      textBtn(article.is_starred ? 'unstar' : 'star', article.is_starred ? 'unstar' : 'star'),
    );
  }
  const page = el('div', { className: 'rss-article' });
  if (article.source_name) page.append(el('p', { className: 'rss-kicker' }, article.source_name));
  page.append(el('h2', null, article.title || t('untitled')));
  const by = [article.author, formatWhen(article.published_at, 'long')].filter(Boolean).join(' · ');
  if (by) page.append(el('p', { className: 'rss-byline' }, by));
  if (article.link) {
    const url = el('p', { className: 'rss-url' });
    url.append(el('span', null, `${t('originalUrl')} `), document.createTextNode(article.link));
    page.append(url);
  }
  const content = el('div', { className: 'rss-body' });
  content.append(sanitizeContent(article.content || article.summary || ''));
  page.append(content);
  body.append(toolbar, page);
  host.replaceChildren(body);
}

function iconBtn(action, labelKey, path) {
  const button = el('button', {
    type: 'button',
    className: 'rss-iconbtn',
    'data-action': action,
    'aria-label': t(labelKey),
  });
  button.appendChild(icon(path));
  return button;
}

function paintEditor(root, state) {
  const host = root.querySelector('[data-role="editor"]');
  if (!host || !state.canManage) return;
  const source = state.sources.find((item) => item.id === state.sourceId);
  if (!source) {
    host.replaceChildren(el('p', { className: 'rss-empty' }, t('pickFeedToEdit')));
    return;
  }
  const form = el('div', { className: 'rss-form' });
  form.append(
    el('input', { type: 'text', 'data-field': 'edit-name', value: source.name || '' }),
    el('input', { type: 'url', 'data-field': 'edit-url', value: source.url || '' }),
    el('input', { type: 'text', 'data-field': 'edit-category', value: source.category || '' }),
    el('input', { type: 'number', 'data-field': 'edit-interval', min: '5', value: source.update_interval || 60 }),
  );
  const enabled = el('label', { className: 'rss-check' }, t('enabled'));
  const box = el('input', { type: 'checkbox', 'data-field': 'edit-enabled' });
  if (source.enabled) box.checked = true;
  enabled.append(box);
  form.append(enabled);
  form.append(
    textBtn('save-source', 'save', null, true),
    textBtn('refresh-source', 'refresh'),
    el('button', {
      type: 'button',
      className: 'rss-textbtn is-danger',
      'data-action': 'delete-source',
    }, state.pendingDelete === source.id ? t('confirmDelete') : t('delete')),
  );
  if (source.last_error) form.append(el('p', { className: 'rss-status' }, source.last_error));
  host.replaceChildren(form);
}

function paintCategoryAdmin(root, state) {
  const host = root.querySelector('[data-role="category-admin"]');
  if (!host || !state.canManage) return;
  host.replaceChildren();
  for (const category of state.categories) {
    const row = el('div', { className: 'rss-row' });
    const save = textBtn('rename-category', 'save');
    save.setAttribute('data-id', String(category.id));
    row.append(
      el('input', {
        type: 'text',
        'data-field': `rename-category-${category.id}`,
        value: category.name || '',
      }),
      save,
      el('button', {
        type: 'button',
        className: 'rss-textbtn is-danger',
        'data-action': 'delete-category',
        'data-id': category.id,
      }, state.pendingDelete === `cat:${category.id}` ? t('confirmDelete') : t('delete')),
    );
    host.append(row);
  }
}

function field(root, name) {
  const node = root.querySelector(`[data-field="${name}"]`);
  if (!node) return '';
  if (node.type === 'checkbox') return node.checked;
  return node.value.trim();
}

async function readPrefs() {
  if (!canStore() || !Tapp.storage || typeof Tapp.storage.get !== 'function') return {};
  try {
    const value = await Tapp.storage.get(PREFS_KEY);
    return value && typeof value === 'object' ? value : {};
  } catch {
    return {};
  }
}

async function writePrefs(state) {
  if (!granted('storage:write') || !Tapp.storage || typeof Tapp.storage.set !== 'function') return;
  try {
    await Tapp.storage.set(PREFS_KEY, {
      filter: state.filter,
      category: state.category,
      sourceId: state.sourceId,
    });
  } catch {
    // 未授予存储时就当次有效。
  }
}

async function boot(root, state) {
  const prefs = await readPrefs();
  if (prefs.filter === 'all' || prefs.filter === 'unread' || prefs.filter === 'starred') {
    state.filter = prefs.filter;
  }
  if (typeof prefs.category === 'string') state.category = prefs.category;
  if (Number.isInteger(prefs.sourceId)) state.sourceId = prefs.sourceId;
  await reload(root, state);
}

async function reload(root, state) {
  state.loading = true;
  state.status = t('loading');
  paint(root, state);
  try {
    const [sources, categories, stats] = await Promise.all([
      api.sources(),
      api.categories().catch(() => []),
      api.stats().catch(() => null),
    ]);
    state.sources = Array.isArray(sources) ? sources : [];
    state.categories = Array.isArray(categories) ? categories : [];
    state.stats = stats;
    if (state.sourceId && !state.sources.some((item) => item.id === state.sourceId)) {
      state.sourceId = null;
    }
    state.status = '';
    await loadItems(root, state, false);
  } catch (error) {
    state.loading = false;
    state.status = t('failed', { message: error.message || 'error' });
    paint(root, state);
  }
}

async function loadItems(root, state, append) {
  state.loading = true;
  if (!append) state.status = t('loading');
  paint(root, state);
  try {
    const query = { filter: state.filter, limit: PAGE_SIZE };
    if (state.sourceId) query.source_id = state.sourceId;
    else if (state.category && state.category !== '__none__') query.category = state.category;
    if (append && state.nextCursor) query.cursor = state.nextCursor;
    const page = await api.list(query);
    const incoming = page && Array.isArray(page.items) ? page.items : [];
    state.items = append ? state.items.concat(incoming) : incoming;
    state.nextCursor = page && page.next_cursor ? page.next_cursor : null;
    if (!state.focusId && state.items[0]) state.focusId = state.items[0].id;
    state.loading = false;
    state.status = '';
  } catch (error) {
    state.loading = false;
    state.status = t('failed', { message: error.message || 'error' });
  }
  paint(root, state);
  void writePrefs(state);
}

async function openArticle(root, state, id) {
  state.status = t('loading');
  paint(root, state);
  try {
    const article = await api.get(id);
    state.article = article;
    state.reading = true;
    state.navOpen = false;
    state.focusId = id;
    state.status = '';
    const listed = state.items.find((item) => item.id === id);
    if (state.canWrite && article && !article.is_read) {
      try {
        await api.markRead(id);
        article.is_read = true;
        if (listed) listed.is_read = true;
        adjustUnread(state, article.source_id || (listed && listed.source_id), -1);
      } catch {
        state.status = t('noWrite');
      }
    }
  } catch (error) {
    state.status = t('failed', { message: error.message || 'error' });
  }
  paint(root, state);
}

function adjustUnread(state, sourceId, delta) {
  const source = state.sources.find((item) => item.id === sourceId);
  if (source) source.unread_count = Math.max(0, (source.unread_count || 0) + delta);
  if (state.stats && typeof state.stats.total_unread === 'number') {
    state.stats.total_unread = Math.max(0, state.stats.total_unread + delta);
  }
}

function stepArticle(state, dir) {
  const items = shownItems(state);
  if (items.length === 0) return null;
  const current = state.article ? state.article.id : state.focusId;
  const index = items.findIndex((item) => item.id === current);
  const next = items[Math.max(0, Math.min(items.length - 1, (index < 0 ? 0 : index) + dir))];
  return next ? next.id : null;
}

async function onClick(root, state, event) {
  const target = event.target.closest ? event.target.closest('[data-action]') : null;
  if (!target || !root.contains(target)) return;
  const action = target.getAttribute('data-action');
  const id = Number(target.getAttribute('data-id'));
  try {
    if (action === 'toggle-nav') {
      state.navOpen = !state.navOpen;
      if (state.navOpen) state.reading = false;
      paint(root, state);
      return;
    }
    if (action === 'folder') {
      state.filter = target.getAttribute('data-filter') || 'all';
      state.category = '';
      state.sourceId = null;
      state.reading = false;
      state.navOpen = false;
      state.nextCursor = null;
      await loadItems(root, state, false);
      return;
    }
    if (action === 'filter' || action === 'category') {
      if (target.getAttribute('data-filter')) state.filter = target.getAttribute('data-filter') || state.filter;
      state.category = target.getAttribute('data-category') || '';
      state.sourceId = null;
      state.reading = false;
      state.navOpen = false;
      state.nextCursor = null;
      await loadItems(root, state, false);
      return;
    }
    if (action === 'source') {
      state.sourceId = state.sourceId === id ? null : id;
      state.reading = false;
      state.navOpen = false;
      state.nextCursor = null;
      await loadItems(root, state, false);
      return;
    }
    if (action === 'open') {
      await openArticle(root, state, id);
      return;
    }
    if (action === 'more') {
      await loadItems(root, state, true);
      return;
    }
    if (action === 'back') {
      state.reading = false;
      paint(root, state);
      return;
    }
    if (action === 'open-original' && state.article) {
      try {
        await api.openOriginal(state.article.id);
        state.status = '';
      } catch {
        state.status = t('openFailed');
      }
      paint(root, state);
      return;
    }
    if (action === 'mark-all') {
      if (!state.canWrite) {
        state.status = t('noWrite');
        paint(root, state);
        return;
      }
      const options = {};
      if (state.sourceId) options.source_id = state.sourceId;
      else if (state.category && state.category !== '__none__') options.category = state.category;
      await api.markAllRead(options);
      await reload(root, state);
      return;
    }
    if (action === 'read' || action === 'unread' || action === 'star' || action === 'unstar') {
      await toggleItem(root, state, action);
      return;
    }
    if (action === 'toggle-manage' || action === 'close-manage') {
      if (!state.canManage && action === 'toggle-manage') return;
      state.manage = action === 'close-manage' ? false : !state.manage;
      paint(root, state);
      return;
    }
    if (action === 'discover') {
      const found = await api.discover(field(root, 'add-url'));
      const name = root.querySelector('[data-field="add-name"]');
      if (name && found && found.title && !name.value) name.value = found.title;
      state.status = t('discovered', { title: (found && found.title) || '' });
      paint(root, state);
      return;
    }
    if (action === 'add-source') {
      await api.addSource({
        url: field(root, 'add-url'),
        name: field(root, 'add-name') || undefined,
        category: field(root, 'add-category') || undefined,
      });
      state.status = t('saved');
      await reload(root, state);
      return;
    }
    if (action === 'save-source' && state.sourceId) {
      const enabled = root.querySelector('[data-field="edit-enabled"]');
      await api.updateSource(state.sourceId, {
        name: field(root, 'edit-name'),
        url: field(root, 'edit-url') || undefined,
        category: field(root, 'edit-category'),
        update_interval: Number(field(root, 'edit-interval')) || undefined,
        enabled: enabled ? enabled.checked : undefined,
      });
      state.status = t('saved');
      await reload(root, state);
      return;
    }
    if (action === 'refresh-all') {
      if (!state.canManage) return;
      let count = 0;
      for (const source of state.sources) {
        if (!isFeed(source)) continue;
        const result = await api.refreshSource(source.id);
        count += (result && result.new_items) || 0;
      }
      state.status = t('refreshDone', { count });
      await reload(root, state);
      return;
    }
    if (action === 'refresh-source' && state.sourceId) {
      const result = await api.refreshSource(state.sourceId);
      state.status = t('refreshDone', { count: (result && result.new_items) || 0 });
      await reload(root, state);
      return;
    }
    if (action === 'delete-source' && state.sourceId) {
      if (state.pendingDelete !== state.sourceId) {
        state.pendingDelete = state.sourceId;
        paint(root, state);
        return;
      }
      await api.deleteSource(state.sourceId);
      state.sourceId = null;
      state.pendingDelete = null;
      state.status = t('deleted');
      await reload(root, state);
      return;
    }
    if (action === 'rename-category') {
      if (!state.canManage) return;
      await api.updateCategory(id, { name: field(root, `rename-category-${id}`) });
      state.status = t('saved');
      await reload(root, state);
      return;
    }
    if (action === 'add-category') {
      await api.createCategory(field(root, 'new-category'));
      state.status = t('saved');
      await reload(root, state);
      return;
    }
    if (action === 'delete-category') {
      const key = `cat:${id}`;
      if (state.pendingDelete !== key) {
        state.pendingDelete = key;
        paint(root, state);
        return;
      }
      await api.deleteCategory(id);
      state.pendingDelete = null;
      state.status = t('deleted');
      await reload(root, state);
      return;
    }
    if (action === 'export-opml') {
      const opml = await api.exportOpml();
      await Tapp.file.download(String(opml || ''), 'subscriptions.opml', 'text/x-opml;charset=utf-8');
      return;
    }
  } catch (error) {
    state.status = t('failed', { message: error.message || 'error' });
    paint(root, state);
  }
}

async function toggleItem(root, state, action) {
  if (!state.canWrite || !state.article) {
    state.status = t('noWrite');
    paint(root, state);
    return;
  }
  const id = state.article.id;
  if (action === 'read') await api.markRead(id);
  if (action === 'unread') await api.markUnread(id);
  if (action === 'star') await api.star(id);
  if (action === 'unstar') await api.unstar(id);
  state.article.is_read = action === 'unread' ? false : action === 'read' ? true : state.article.is_read;
  state.article.is_starred = action === 'unstar' ? false : action === 'star' ? true : state.article.is_starred;
  const listed = state.items.find((item) => item.id === id);
  if (listed) {
    listed.is_read = state.article.is_read;
    listed.is_starred = state.article.is_starred;
  }
  if (action === 'read') adjustUnread(state, state.article.source_id, -1);
  if (action === 'unread') adjustUnread(state, state.article.source_id, 1);
  state.status = t('saved');
  paint(root, state);
}

function onInput(root, state, event) {
  const input = event.target;
  if (!input || input.getAttribute('data-field') !== 'search') return;
  state.query = input.value || '';
  paintItems(root, state);
}

function onChange(root, state, event) {
  const input = event.target;
  if (!input || input.getAttribute('data-action') !== 'import-opml' || !state.canManage) return;
  const file = input.files && input.files[0];
  input.value = '';
  if (!file) return;
  const reader = new FileReader();
  reader.onload = () => {
    void api.importOpml(String(reader.result || '')).then((result) => {
      state.status = t('imported', {
        imported: (result && result.imported) || 0,
        skipped: (result && result.skipped) || 0,
      });
      return reload(root, state);
    }).catch((error) => {
      state.status = t('failed', { message: error.message || 'error' });
      paint(root, state);
    });
  };
  reader.readAsText(file);
}

function onKey(root, state, event) {
  const tag = event.target && event.target.tagName;
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') {
    if (event.key === 'Escape') event.target.blur();
    return;
  }
  if (event.key === 'Escape') {
    if (state.manage) {
      state.manage = false;
      paint(root, state);
      return;
    }
    if (state.reading) {
      state.reading = false;
      paint(root, state);
    }
    return;
  }
  if (event.key === 'j' || event.key === 'J') {
    const id = stepArticle(state, 1);
    if (id) void openArticle(root, state, id);
    return;
  }
  if (event.key === 'k' || event.key === 'K') {
    const id = stepArticle(state, -1);
    if (id) void openArticle(root, state, id);
    return;
  }
  if (event.key === 'o' || event.key === 'O') {
    if (state.article) void api.openOriginal(state.article.id).catch(() => {});
    return;
  }
  if ((event.key === 's' || event.key === 'S') && state.article) {
    void toggleItem(root, state, state.article.is_starred ? 'unstar' : 'star');
    return;
  }
  if ((event.key === 'u' || event.key === 'U') && state.article) {
    void toggleItem(root, state, state.article.is_read ? 'unread' : 'read');
  }
}

module.exports = { start };
