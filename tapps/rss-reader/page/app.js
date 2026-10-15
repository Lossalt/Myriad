const api = require('./api.js');
const { sanitizeContent } = require('./sanitize.js');

const PAGE_SIZE = 20;

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

function categoryTokens(value) {
  return String(value || '')
    .split(',')
    .map((part) => part.trim())
    .filter(Boolean);
}

function formatWhen(value) {
  if (value == null || value === '') return '';
  const numeric = typeof value === 'number' ? value : Number(value);
  const date = Number.isFinite(numeric)
    ? new Date(numeric > 1e12 ? numeric : numeric * 1000)
    : new Date(value);
  if (Number.isNaN(date.getTime())) return '';
  try {
    return new Intl.DateTimeFormat(Tapp.i18n.getLocale(), {
      month: 'short',
      day: 'numeric',
    }).format(date);
  } catch {
    return '';
  }
}

function el(tag, attrs, text) {
  const node = document.createElement(tag);
  if (attrs) {
    for (const [key, value] of Object.entries(attrs)) {
      if (value == null || value === false) continue;
      if (key === 'className') node.className = value;
      else node.setAttribute(key, String(value));
    }
  }
  if (text != null) node.textContent = text;
  return node;
}

function start(root) {
  const state = {
    sources: [],
    categories: [],
    stats: null,
    category: '',
    sourceId: null,
    filter: 'all',
    items: [],
    nextCursor: null,
    loading: false,
    article: null,
    reading: false,
    manage: false,
    pendingDelete: null,
    status: '',
    canWrite: granted('phantasi:write'),
    canManage: granted('phantasi:manage'),
  };

  root.replaceChildren();
  root.append(
    buildHeader(state),
    buildColumns(state),
    buildManage(state),
    el('p', { className: 'rss-status', 'data-role': 'status', role: 'status' }),
  );
  root.addEventListener('click', (event) => onClick(root, state, event));
  root.addEventListener('change', (event) => onChange(root, state, event));
  if (typeof Tapp.ui.onLocaleChange === 'function') {
    Tapp.ui.onLocaleChange(() => {
      applyStaticText(root, state);
      paint(root, state);
    });
  }
  applyStaticText(root, state);
  void Tapp.ui.setTitle(t('title')).catch(() => {});
  void reload(root, state);
}

function buildHeader(state) {
  const header = el('header', { className: 'rss-header' });
  header.append(
    el('h1', { 'data-i18n': 'title' }),
    el('p', { className: 'rss-stats', 'data-role': 'stats' }),
    el('div', { className: 'rss-filters', 'data-role': 'filters' }),
  );
  header.append(el('button', { type: 'button', 'data-action': 'export-opml', 'data-i18n': 'exportOpml' }));
  if (state.canWrite) {
    header.append(el('button', { type: 'button', 'data-action': 'mark-all', 'data-i18n': 'markAllRead' }));
  }
  if (state.canManage) {
    header.append(el('button', { type: 'button', 'data-action': 'toggle-manage', 'data-role': 'manage-toggle' }));
  }
  return header;
}

function buildColumns() {
  const columns = el('div', { className: 'rss-columns', 'data-role': 'columns' });
  const sources = el('aside', { className: 'rss-panel rss-sources', 'data-role': 'sources' });
  sources.append(el('h2', { 'data-i18n': 'sources' }), el('div', { 'data-role': 'source-list' }));
  const items = el('section', { className: 'rss-panel rss-items', 'data-role': 'items' });
  items.append(el('h2', { 'data-i18n': 'articles' }), el('div', { 'data-role': 'item-list' }));
  const reader = el('article', { className: 'rss-panel rss-reader', 'data-role': 'reader' });
  reader.append(el('h2', { 'data-i18n': 'reading' }), el('div', { 'data-role': 'reader-body' }));
  columns.append(sources, items, reader);
  return columns;
}

function buildManage(state) {
  const section = el('section', { className: 'rss-panel rss-manage', 'data-role': 'manage', hidden: true });
  if (!state.canManage) return section;
  section.append(
    el('h2', { 'data-i18n': 'addSource' }),
    formRow([
      el('input', { type: 'url', 'data-field': 'add-url', 'data-i18n-placeholder': 'feedUrl' }),
      el('button', { type: 'button', 'data-action': 'discover', 'data-i18n': 'discover' }),
      el('input', { type: 'text', 'data-field': 'add-name', 'data-i18n-placeholder': 'sourceName' }),
      el('input', { type: 'text', 'data-field': 'add-category', 'data-i18n-placeholder': 'category' }),
      el('button', { type: 'button', 'data-action': 'add-source', 'data-i18n': 'add' }),
    ]),
    el('h2', { 'data-i18n': 'sources' }),
    el('button', { type: 'button', 'data-action': 'refresh-all', 'data-i18n': 'refreshAll' }),
    el('div', { 'data-role': 'editor' }),
    el('h2', { 'data-i18n': 'categories' }),
    formRow([
      el('input', { type: 'text', 'data-field': 'new-category', 'data-i18n-placeholder': 'newCategory' }),
      el('button', { type: 'button', 'data-action': 'add-category', 'data-i18n': 'addCategory' }),
    ]),
    el('div', { className: 'rss-row', 'data-role': 'category-admin' }),
    el('label', { className: 'rss-row' }),
  );
  const importLabel = section.querySelector('label');
  importLabel.append(
    el('span', { 'data-i18n': 'importOpml' }),
    el('input', { type: 'file', accept: '.opml,.xml,text/xml,text/x-opml', 'data-action': 'import-opml' }),
  );
  return section;
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
  const toggle = root.querySelector('[data-role="manage-toggle"]');
  if (toggle) toggle.textContent = state.manage ? t('closeManage') : t('manage');
  const manage = root.querySelector('[data-role="manage"]');
  if (manage) manage.hidden = !state.manage;
  document.documentElement.lang = Tapp.i18n.getLocale() || '';
}

function paint(root, state) {
  const stats = root.querySelector('[data-role="stats"]');
  if (stats) {
    stats.textContent = state.stats
      ? t('stats', {
        unread: state.stats.total_unread ?? 0,
        sources: state.sources.length,
      })
      : '';
  }
  paintFilters(root, state);
  paintSources(root, state);
  paintItems(root, state);
  paintReader(root, state);
  paintEditor(root, state);
  paintCategoryAdmin(root, state);
  const status = root.querySelector('[data-role="status"]');
  if (status) status.textContent = state.status;
  const columns = root.querySelector('[data-role="columns"]');
  if (columns) columns.classList.toggle('is-reading', state.reading);
  applyStaticText(root, state);
}

function paintFilters(root, state) {
  const host = root.querySelector('[data-role="filters"]');
  if (!host) return;
  host.replaceChildren(
    filterButton('all', 'filterAll', state.filter),
    filterButton('unread', 'filterUnread', state.filter),
    filterButton('starred', 'filterStarred', state.filter),
  );
}

function filterButton(filter, key, current) {
  return el('button', {
    type: 'button',
    'data-action': 'filter',
    'data-filter': filter,
    'aria-pressed': current === filter ? 'true' : 'false',
  }, t(key));
}

function visibleSources(state) {
  return state.sources.filter((source) => {
    if (source.source_type === 'note' || source.source_type === 'link') return false;
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
    const tokens = categoryTokens(source.category);
    if (tokens.length === 0) hasEmpty = true;
    for (const token of tokens) names.add(token);
  }
  for (const category of state.categories) {
    if (category && category.name) names.add(category.name);
  }
  const chips = el('div', { className: 'rss-categories' });
  chips.append(categoryChip('', 'allSources', state.category));
  if (hasEmpty) chips.append(categoryChip('__none__', 'uncategorized', state.category));
  for (const name of [...names].sort((a, b) => a.localeCompare(b))) {
    chips.append(el('button', {
      type: 'button',
      'data-action': 'category',
      'data-category': name,
      'aria-pressed': state.category === name ? 'true' : 'false',
    }, name));
  }
  const list = el('div');
  const sources = visibleSources(state);
  if (sources.length === 0) {
    list.append(el('p', { className: 'rss-meta' }, t('emptySources')));
  }
  for (const source of sources) {
    const button = el('button', {
      type: 'button',
      className: 'rss-source',
      'data-action': 'source',
      'data-id': source.id,
      'aria-pressed': state.sourceId === source.id ? 'true' : 'false',
    });
    button.append(el('span', null, source.name || source.url || String(source.id)));
    if (source.unread_count) {
      button.append(el('span', { className: 'rss-badge' }, String(source.unread_count)));
    }
    list.append(button);
  }
  host.replaceChildren(chips, list);
}

function categoryChip(category, key, current) {
  return el('button', {
    type: 'button',
    'data-action': 'category',
    'data-category': category,
    'aria-pressed': current === category ? 'true' : 'false',
  }, t(key));
}

function paintItems(root, state) {
  const host = root.querySelector('[data-role="item-list"]');
  if (!host) return;
  const list = el('div');
  if (state.items.length === 0) {
    list.append(el('p', { className: 'rss-meta' }, state.loading ? t('loading') : t('emptyItems')));
  }
  for (const item of state.items) {
    const button = el('button', {
      type: 'button',
      className: item.is_read ? 'rss-item' : 'rss-item is-unread',
      'data-action': 'open',
      'data-id': item.id,
    });
    button.append(el('strong', null, item.title || t('emptyArticle')));
    const meta = [item.source_name, formatWhen(item.published_at)].filter(Boolean).join(' · ');
    if (meta) button.append(el('span', { className: 'rss-meta' }, meta));
    list.append(button);
  }
  if (state.nextCursor) {
    list.append(el('button', { type: 'button', 'data-action': 'more' }, t('loadMore')));
  }
  host.replaceChildren(list);
}

function paintReader(root, state) {
  const host = root.querySelector('[data-role="reader-body"]');
  if (!host) return;
  const article = state.article;
  if (!article) {
    host.replaceChildren(el('p', { className: 'rss-meta' }, t('emptyArticle')));
    return;
  }
  const body = el('div');
  body.append(el('button', { type: 'button', 'data-action': 'back', className: 'rss-back' }, t('back')));
  body.append(el('h3', null, article.title || ''));
  const meta = [article.author, article.source_name, formatWhen(article.published_at)].filter(Boolean).join(' · ');
  if (meta) body.append(el('p', { className: 'rss-meta' }, meta));
  if (article.link) {
    body.append(el('button', { type: 'button', 'data-action': 'open-original' }, t('openOriginal')));
    const url = el('p', { className: 'rss-url' });
    url.append(el('span', { className: 'rss-meta' }, `${t('originalUrl')} `), document.createTextNode(article.link));
    body.append(url);
  }
  if (state.canWrite) {
    const row = el('div', { className: 'rss-row' });
    row.append(
      el('button', { type: 'button', 'data-action': article.is_read ? 'unread' : 'read' }, article.is_read ? t('markUnread') : t('markRead')),
      el('button', { type: 'button', 'data-action': article.is_starred ? 'unstar' : 'star' }, article.is_starred ? t('unstar') : t('star')),
    );
    body.append(row);
  }
  const content = el('div', { className: 'rss-reader-body' });
  content.append(sanitizeContent(article.content || article.summary || ''));
  body.append(content);
  host.replaceChildren(body);
}

function paintEditor(root, state) {
  const host = root.querySelector('[data-role="editor"]');
  if (!host || !state.canManage) return;
  const source = state.sources.find((item) => item.id === state.sourceId);
  if (!source) {
    host.replaceChildren();
    return;
  }
  const form = el('div', { className: 'rss-form' });
  form.append(
    el('input', { type: 'text', 'data-field': 'edit-name', value: source.name || '' }),
    el('input', { type: 'url', 'data-field': 'edit-url', value: source.url || '' }),
    el('input', { type: 'text', 'data-field': 'edit-category', value: source.category || '' }),
    el('input', { type: 'number', 'data-field': 'edit-interval', min: '5', value: source.update_interval || 60 }),
    el('label', null, t('enabled')),
  );
  const enabled = el('input', { type: 'checkbox', 'data-field': 'edit-enabled' });
  if (source.enabled) enabled.checked = true;
  form.lastChild.append(enabled);
  form.append(
    el('button', { type: 'button', 'data-action': 'save-source' }, t('save')),
    el('button', { type: 'button', 'data-action': 'refresh-source' }, t('refresh')),
    el('button', { type: 'button', 'data-action': 'delete-source' }, state.pendingDelete === source.id ? t('confirmDelete') : t('delete')),
  );
  if (source.last_error) form.append(el('p', { className: 'rss-meta' }, source.last_error));
  host.replaceChildren(form);
}

function paintCategoryAdmin(root, state) {
  const host = root.querySelector('[data-role="category-admin"]');
  if (!host || !state.canManage) return;
  host.replaceChildren();
  for (const category of state.categories) {
    const row = el('div', { className: 'rss-row' });
    row.append(
      el('input', {
        type: 'text',
        'data-field': `rename-category-${category.id}`,
        value: category.name || '',
      }),
      el('button', {
        type: 'button',
        'data-action': 'rename-category',
        'data-id': category.id,
      }, t('save')),
      el('button', {
        type: 'button',
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
    state.loading = false;
    state.status = '';
  } catch (error) {
    state.loading = false;
    state.status = t('failed', { message: error.message || 'error' });
  }
  paint(root, state);
}

async function openArticle(root, state, id) {
  state.status = t('loading');
  paint(root, state);
  try {
    const article = await api.get(id);
    state.article = article;
    state.reading = true;
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

async function onClick(root, state, event) {
  const target = event.target.closest('[data-action]');
  if (!target || !root.contains(target)) return;
  const action = target.getAttribute('data-action');
  const id = Number(target.getAttribute('data-id'));
  try {
    if (action === 'filter') {
      state.filter = target.getAttribute('data-filter') || 'all';
      state.nextCursor = null;
      await loadItems(root, state, false);
      return;
    }
    if (action === 'category') {
      state.category = target.getAttribute('data-category') || '';
      state.sourceId = null;
      state.nextCursor = null;
      await loadItems(root, state, false);
      return;
    }
    if (action === 'source') {
      state.sourceId = state.sourceId === id ? null : id;
      state.reading = false;
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
      } catch (error) {
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
    if (action === 'toggle-manage') {
      state.manage = !state.manage;
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
        if (source.source_type === 'note' || source.source_type === 'link') continue;
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

module.exports = { start };
