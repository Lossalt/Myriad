const { start } = require('./app.js');

Tapp.lifecycle.onReady(function () {
  const root = document.getElementById('rss-app');
  if (!root) return;
  start(root);
});
