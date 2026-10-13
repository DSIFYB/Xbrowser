const { DatabaseSync } = require('node:sqlite');
const path = require('node:path');
const db = new DatabaseSync(path.join(process.env.LOCALAPPDATA, 'AxiomBrowser', 'Profile', 'browser.sqlite3'), { readOnly: true });
const tabs = db.prepare('SELECT position, url FROM sessions ORDER BY position').all();
console.log(JSON.stringify({ tabCount: tabs.length, active: db.prepare("SELECT value FROM meta WHERE key='active_index'").get()?.value, pages: tabs.map(tab => { try { const url = new URL(tab.url); return { position: tab.position, host: url.hostname, protocol: url.protocol }; } catch { return {position: tab.position, invalid: true}; } }), bookmarkCount: db.prepare('SELECT count(*) AS count FROM bookmarks').get().count }, null, 2));
db.close();
