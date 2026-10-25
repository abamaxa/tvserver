#!/usr/bin/env node
// Verify the installed WebView, Tauri commands and SQLite migration together.
// Only a uniquely named temporary playlist is changed; library files are untouched.
const { execFileSync } = require('node:child_process');
const { randomBytes } = require('node:crypto');
const path = require('node:path');
const os = require('node:os');

const serial = process.argv[2] || 'emulator-5554';
const sdk = process.env.ANDROID_HOME || path.join(os.homedir(), 'Library/Android/sdk');
const adb = process.env.LOV_ADB || path.join(sdk, 'platform-tools/adb');
const run = (...args) => execFileSync(adb, ['-s', serial, ...args], { encoding: 'utf8' }).trim();
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const ports = [];

async function evaluate(url, expression) {
  const socket = new WebSocket(url);
  try {
    return await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error('Playlist smoke test timed out')), 45000);
      const finish = (callback, value) => { clearTimeout(timeout); callback(value); };
      socket.addEventListener('error', () => finish(reject, new Error('WebView connection failed')));
      socket.addEventListener('open', () => socket.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate',
        params: { expression, awaitPromise: true, returnByValue: true } })));
      socket.addEventListener('message', ({ data }) => {
        const response = JSON.parse(data);
        if (response.id !== 1) return;
        const error = response.error || response.result?.exceptionDetails;
        if (error) finish(reject, new Error(error.exception?.description || error.message || error.text));
        else finish(resolve, response.result.result.value);
      });
    });
  } finally { socket.close(); }
}

async function connect() {
  run('shell', 'am', 'start', '-W', '-n', 'com.abamaxa.lots_of_videos/.MainActivity');
  const pid = run('shell', 'pidof', 'com.abamaxa.lots_of_videos');
  if (!/^\d+$/.test(pid)) throw new Error('App process unavailable');
  const port = run('forward', 'tcp:0', `localabstract:webview_devtools_remote_${pid}`);
  ports.push(port);
  for (let attempt = 0; attempt < 50; attempt++) {
    try {
      const pages = await (await fetch(`http://127.0.0.1:${port}/json`, { signal: AbortSignal.timeout(2000) })).json();
      const page = pages.find((item) => item.type === 'page' && item.url.startsWith('http://tauri.localhost'));
      if (page && await evaluate(page.webSocketDebuggerUrl, 'Boolean(window.__TAURI_INTERNALS__?.invoke)')) return page.webSocketDebuggerUrl;
    } catch (_) {}
    await pause(200);
  }
  throw new Error('App WebView unavailable');
}

(async () => {
  const name = `Playlist regression ${randomBytes(8).toString('hex')}`;
  let url;
  let playlistId;
  try {
    url = await connect();
    const result = await evaluate(url, `(${async (name) => {
      const invoke = window.__TAURI_INTERNALS__.invoke;
      const playlist = await invoke('playlist_create', { name: name + ' original' });
      // The host retains this ID immediately, even if a later assertion fails.
      return { id: playlist.id };
    }})(${JSON.stringify(name)})`);
    playlistId = result.id;
    const checked = await evaluate(url, `(${async (playlistId, name) => {
      const invoke = window.__TAURI_INTERNALS__.invoke;
      const wait = async (condition) => {
        for (let i = 0; i < 100; i++) { if (condition()) return; await new Promise((resolve) => setTimeout(resolve, 100)); }
        throw new Error('Playlist UI did not update');
      };
      const videos = [];
      const pending = [''];
      const visited = new Set();
      while (pending.length && videos.length < 2 && visited.size < 50) {
        const collection = pending.shift();
        if (visited.has(collection)) continue;
        visited.add(collection);
        const result = await invoke('list_collection', { collection });
        const details = result.Collection;
        if (!details) continue;
        for (const item of details.videos || []) if (item.Video && !videos.some((video) => video.checksum === item.Video.checksum)) videos.push(item.Video);
        for (const child of details.child_collections || []) pending.push([collection, child.collection].filter(Boolean).join('/'));
      }
      const selected = videos.slice(0, 2);
      for (const video of selected) await invoke('playlist_add_video', { playlistId, videoId: video.checksum });
      if (selected.length) await invoke('playlist_add_video', { playlistId, videoId: selected[0].checksum });
      const videoIds = selected.map((video) => video.checksum).reverse();
      await invoke('playlist_reorder', { playlistId, videoIds });
      let details = await invoke('playlist_get', { playlistId });
      if (JSON.stringify(details.videos.map((video) => video.checksum)) !== JSON.stringify(videoIds)) throw new Error('Order or duplicate protection failed');
      if (selected.length) {
        const videoId = videoIds[videoIds.length - 1];
        await invoke('playlist_remove_video', { playlistId, videoId });
        await invoke('playlist_add_video', { playlistId, videoId });
      }
      await invoke('playlist_rename', { playlistId, name });
      await wait(() => document.querySelector('nav [aria-label="Playlists"]'));
      document.querySelector('nav [aria-label="Playlists"]').click();
      await wait(() => Array.from(document.querySelectorAll('button')).some((button) => button.getAttribute('aria-label') === 'Open playlist ' + name));
      Array.from(document.querySelectorAll('button')).find((button) => button.getAttribute('aria-label') === 'Open playlist ' + name).click();
      await wait(() => document.querySelector('h1')?.textContent === name);
      if (document.querySelectorAll('[aria-label="Playlist items"] > li').length !== selected.length) throw new Error('Playlist UI membership differs from SQLite');
      return { videoIds, membership: selected.length, ui: 'passed' };
    }})(${JSON.stringify(playlistId)}, ${JSON.stringify(name)})`);
    run('shell', 'am', 'force-stop', 'com.abamaxa.lots_of_videos');
    url = await connect();
    const restored = await evaluate(url, `window.__TAURI_INTERNALS__.invoke('playlist_get', { playlistId: ${JSON.stringify(playlistId)} })`);
    if (restored.name !== name || JSON.stringify(restored.videos.map((video) => video.checksum)) !== JSON.stringify(checked.videoIds)) {
      throw new Error('Saved playlist did not survive app restart');
    }
    console.log('PASS installed Android playlists:', JSON.stringify({ ...checked, restart: 'passed' }));
  } finally {
    if (playlistId && url) await evaluate(url, `window.__TAURI_INTERNALS__.invoke('playlist_delete', { playlistId: ${JSON.stringify(playlistId)} })`);
    for (const port of ports) run('forward', '--remove', `tcp:${port}`);
  }
})().catch((error) => { console.error(error.message); process.exitCode = 1; });
