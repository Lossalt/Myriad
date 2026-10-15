// 沙箱 SDK 已经把桥的 { success, data } 解成返回值，失败则 reject。
// 这里不要再读 .data，否则列表会变成 undefined。

function list(options) {
  return Tapp.phantasiList.list(options || {});
}

function get(id) {
  return Tapp.phantasiList.get(id);
}

function openOriginal(id) {
  return Tapp.phantasiList.openOriginal(id);
}

function sources() {
  return Tapp.phantasiList.sources({ board: 'feeds' });
}

function categories() {
  return Tapp.phantasiList.categories();
}

function stats() {
  return Tapp.phantasiList.stats();
}

function markRead(id) {
  return Tapp.phantasiList.markRead(id);
}

function markUnread(id) {
  return Tapp.phantasiList.markUnread(id);
}

function star(id) {
  return Tapp.phantasiList.star(id);
}

function unstar(id) {
  return Tapp.phantasiList.unstar(id);
}

function markAllRead(options) {
  return Tapp.phantasiList.markAllRead(options || {});
}

function discover(url) {
  return Tapp.phantasiList.discover(url);
}

function addSource(req) {
  return Tapp.phantasiList.addSource(req);
}

function updateSource(id, req) {
  return Tapp.phantasiList.updateSource(id, req);
}

function deleteSource(id) {
  return Tapp.phantasiList.deleteSource(id);
}

function refreshSource(id) {
  return Tapp.phantasiList.refreshSource(id);
}

function importOpml(opml) {
  return Tapp.phantasiList.importOpml(opml);
}

function exportOpml() {
  return Tapp.phantasiList.exportOpml();
}

function createCategory(name) {
  return Tapp.phantasiList.createCategory({ name });
}

function updateCategory(id, req) {
  return Tapp.phantasiList.updateCategory(id, req);
}

function deleteCategory(id) {
  return Tapp.phantasiList.deleteCategory(id);
}

module.exports = {
  list,
  get,
  openOriginal,
  sources,
  categories,
  stats,
  markRead,
  markUnread,
  star,
  unstar,
  markAllRead,
  discover,
  addSource,
  updateSource,
  deleteSource,
  refreshSource,
  importOpml,
  exportOpml,
  createCategory,
  updateCategory,
  deleteCategory,
};
