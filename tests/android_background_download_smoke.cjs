#!/usr/bin/env node
// Requires an otherwise idle debug app, adb and Node 22+. Uses only the emulator by default.
const { execFileSync, spawn } = require('node:child_process');
const { randomBytes } = require('node:crypto');
const path = require('node:path');
const os = require('node:os');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const http = require('node:http');
const readline = require('node:readline');

const serial = process.argv[2] || 'emulator-5554';
const sdk = process.env.ANDROID_HOME || path.join(os.homedir(), 'Library/Android/sdk');
const adb = process.env.LOV_ADB || path.join(sdk, 'platform-tools/adb');
const app = 'com.abamaxa.lots_of_videos';
const run = (...args) => execFileSync(adb, ['-s', serial, ...args], { encoding: 'utf8' }).trim();
const delay = (millis) => new Promise((resolve) => setTimeout(resolve, millis));

async function connect() {
  for (let attempt = 0; attempt < 30; attempt++) {
    let pid;
    try { pid = run('shell', 'pidof', app); } catch { await delay(250); continue; }
    const port = run('forward', 'tcp:0', `localabstract:webview_devtools_remote_${pid}`);
    try {
      const pages = await (await fetch(`http://127.0.0.1:${port}/json`, { signal: AbortSignal.timeout(1000) })).json();
      const page = pages.find((item) => item.type === 'page');
      if (page) return { pid, port, url: page.webSocketDebuggerUrl };
    } catch {}
    run('forward', '--remove', `tcp:${port}`);
    await delay(250);
  }
  throw new Error('Open the debug app before running this test');
}

async function evaluate(connection, expression) {
  const socket = new WebSocket(connection.url);
  try {
    return await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error('Android IPC timed out')), 15000);
      const finish = (callback, value) => { clearTimeout(timeout); callback(value); };
      socket.addEventListener('error', () => finish(reject, new Error('WebView connection failed')));
      socket.addEventListener('open', () => socket.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate',
        params: { expression, awaitPromise: true, returnByValue: true } })));
      socket.addEventListener('message', ({ data }) => {
        const response = JSON.parse(data);
        if (response.id !== 1) return;
        const error = response.error || response.result?.exceptionDetails;
        if (error) finish(reject, new Error(error.exception?.description || error.exception?.value || error.message || error.text));
        else finish(resolve, response.result.result.value);
      });
    });
  } finally { socket.close(); }
}

const invoke = (connection, command, args = {}) => evaluate(connection,
  `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`)
  .catch((error) => { throw new Error(`${command}: ${error.message}`); });

async function transferFixture() {
  const binary = path.join(__dirname, '../target/debug/examples/android_download_seed');
  assert(fs.existsSync(binary), 'Build the local peer first: cargo build --no-default-features --features webserver --example android_download_seed');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'lov-background-download-'));
  const name = `background-${randomBytes(6).toString('hex')}`;
  const clip = fs.readFileSync(path.join(__dirname, '../gen/android/app/src/androidTest/assets/native-player-ui-test.mp4'));
  const video = Buffer.alloc(16 * 1024 * 1024);
  for (let offset = 0; offset < video.length; offset += clip.length) clip.copy(video, offset);
  const source = path.join(root, `${name}-torrent.mp4`);
  fs.writeFileSync(source, video);
  const seed = spawn(binary, [source], { stdio: ['ignore', 'pipe', 'inherit'] });
  const lines = readline.createInterface({ input: seed.stdout });
  const disposeSeed = () => {
    seed.kill();
    lines.close();
    fs.rmSync(root, { recursive: true, force: true });
  };
  const ready = await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error('Local torrent peer did not start')), 15000);
    lines.once('line', (line) => { clearTimeout(timeout); resolve(JSON.parse(line)); });
    seed.once('error', reject);
    seed.once('exit', (code) => { clearTimeout(timeout); reject(new Error(`Local peer exited: ${code}`)); });
  }).catch((error) => { disposeSeed(); throw error; });
  let uploaded = 0;
  let served = 0;
  let announces = 0;
  if (!ready.seeded) { disposeSeed(); throw new Error('Local torrent fixture must be a complete seed'); }
  lines.on('line', (line) => { uploaded = JSON.parse(line).uploaded; });
  let torrent;
  const server = http.createServer((request, response) => {
    if (request.url.startsWith('/announce')) {
      announces++;
      response.end(Buffer.concat([Buffer.from('d8:completei1e10:incompletei1e8:intervali5e5:peers6:'),
        Buffer.from([10, 0, 2, 2, ready.port >> 8, ready.port & 255]), Buffer.from('e')]));
    } else if (request.url === '/fixture.torrent') {
      response.writeHead(200, { 'Content-Type': 'application/x-bittorrent', 'Content-Length': torrent.length });
      response.end(torrent);
    } else if (request.url === '/video.mp4') {
      const offset = Number(request.headers.range?.match(/bytes=(\d+)-/)?.[1] || 0);
      response.writeHead(offset ? 206 : 200, { 'Content-Type': 'video/mp4', 'Content-Length': video.length - offset,
        'Accept-Ranges': 'bytes', ...(offset ? { 'Content-Range': `bytes ${offset}-${video.length - 1}/${video.length}` } : {}) });
      if (request.method === 'HEAD') { response.end(); return; }
      let position = offset;
      let blocked = false;
      response.on('drain', () => { blocked = false; });
      const timer = setInterval(() => {
        if (blocked) return;
        const end = Math.min(position + 65536, video.length);
        blocked = !response.write(video.subarray(position, end));
        served += end - position;
        position = end;
        if (position === video.length) { clearInterval(timer); response.end(); }
      }, 200);
      response.on('close', () => clearInterval(timer));
    } else { response.writeHead(404); response.end(); }
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const url = `http://10.0.2.2:${server.address().port}`;
  const announce = `${url}/announce`;
  torrent = Buffer.concat([Buffer.from(`d8:announce${Buffer.byteLength(announce)}:${announce}`),
    Buffer.from(ready.torrent, 'base64').subarray(1)]);
  return { name, url, bytes: () => ({ served, uploaded, announces }), close: async () => {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
    disposeSeed();
  } };
}

async function awaitTasks(connection, requests) {
  let last;
  for (let attempt = 0; attempt < 40; attempt++) {
    const { results } = await invoke(connection, 'tasks_list');
    const tasks = requests.map(({ link }) => results.find((task) => task.key === link));
    last = tasks;
    for (const task of tasks) assert(!task?.errorString, `Fixture failed: ${task?.errorString}`);
    if (tasks.every((task) => task && !task.finished && !/^0 B(?:\/|$)/.test(task.sizeDetails))) return tasks;
    await delay(500);
  }
  throw new Error(`Both local download engines must transfer bytes before locking the screen: ${JSON.stringify(last)}`);
}

const wake = () => { run('shell', 'input', 'keyevent', 'KEYCODE_WAKEUP'); run('shell', 'wm', 'dismiss-keyguard'); };
const powerLocks = () => run('shell', 'dumpsys', 'power').split('Wake Locks: size=')[1]?.split('Suspend Blockers: size=')[0] || '';

(async () => {
  wake();
  run('shell', 'am', 'start', '-W', '-n', `${app}/.MainActivity`);
  await delay(2000);
  let connection = await connect();
  const queued = [];
  let fixture;
  try {
    const initial = await invoke(connection, 'tasks_list');
    assert(!initial.results.some((task) => !task.finished), 'Use an emulator without active downloads');
    fixture = process.argv.includes('--transfers') ? await transferFixture() : null;
    const requests = fixture ? [
      { engine: 'YouTube', name: `${fixture.name}-youtube`, link: `${fixture.url}/video.mp4` },
      { engine: 'Torrent', name: `${fixture.name}-torrent`, link: `${fixture.url}/fixture.torrent` },
    ] : [{ engine: 'Torrent', name: 'Background download probe',
      link: `magnet:?xt=urn:btih:${randomBytes(20).toString('hex')}&dn=Background%20download%20probe` }];
    for (const request of requests) {
      await invoke(connection, 'tasks_add', { payload: request });
      queued.push(request.link);
    }
    const before = fixture ? await awaitTasks(connection, requests).catch((error) => {
      throw new Error(`${error.message}; local fixture: ${JSON.stringify(fixture.bytes())}`);
    }) : (await invoke(connection, 'tasks_list')).results;
    for (const key of queued) assert(before.some((task) => task.key === key && !task.finished), 'Probe did not register');
    const bytes = fixture?.bytes();
    run('shell', 'input', 'keyevent', 'KEYCODE_HOME');
    run('shell', 'input', 'keyevent', 'KEYCODE_SLEEP');
    await delay(fixture ? 10000 : 1500);
    run('shell', 'am', 'make-uid-idle', app);
    // am kill reclaims only processes Android regards as safe to kill in the background.
    // A protected transfer must survive; this never force-stops a foreground service/job.
    run('shell', 'am', 'kill', app);
    await delay(1500);
    let pid = '';
    try { pid = run('shell', 'pidof', app); } catch {}
    assert.equal(pid, connection.pid, 'Screen-off download was lost when Android reclaimed the background app');
    const locks = powerLocks();
    assert(locks.includes('NativeDownloadJobService') || locks.includes(`${app}:downloads`),
      'Download has no CPU protection with the screen switched off');
    if (fixture) {
      assert(fixture.bytes().served > bytes.served, 'yt-dlp stopped receiving bytes while locked');
      assert(fixture.bytes().uploaded > bytes.uploaded, 'Torrent stopped receiving bytes while locked');
    }
    wake();
    run('shell', 'am', 'start', '-W', '-n', `${app}/.MainActivity`);
    const retained = await invoke(connection, 'tasks_list');
    for (const key of queued) assert(retained.results.some((task) => task.key === key && !task.finished), 'Download disappeared while locked');
    if (fixture) {
      const after = await awaitTasks(connection, requests);
      assert.notEqual(after[0].sizeDetails, before.find((task) => task.key === requests[0].link).sizeDetails,
        'yt-dlp file did not grow while locked');
      assert(after[1].percentDone > before.find((task) => task.key === requests[1].link).percentDone,
        'Torrent pieces did not advance while locked');
      console.log('PASS actual yt-dlp and torrent transfers continue with the screen off');
    }
    console.log('PASS Android screen-off download survives background process reclaim');
  } finally {
    wake();
    run('shell', 'am', 'start', '-W', '-n', `${app}/.MainActivity`);
    if (queued.length) {
      try {
        let pid = '';
        try { pid = run('shell', 'pidof', app); } catch {}
        if (pid !== connection.pid) {
          run('forward', '--remove', `tcp:${connection.port}`);
          connection = await connect();
        }
        for (const key of queued) await invoke(connection, 'tasks_delete', { key });
        for (let attempt = 0; attempt < 50 && /NativeDownloadJobService|:downloads/.test(powerLocks()); attempt++) await delay(100);
        assert(!/NativeDownloadJobService|:downloads/.test(powerLocks()), 'Download wake lock leaked after cancellation');
        if (fixture) run('shell', 'run-as', app, 'rm', '-f', `movies/.downloads/${fixture.name}-youtube.mp4`,
          `movies/.downloads/${fixture.name}-youtube.mp4.part`, `movies/.downloads/${fixture.name}-youtube.mp4.ytdl`);
      } catch (error) { console.error('Probe cleanup:', error.message); process.exitCode = 1; }
    }
    try { run('forward', '--remove', `tcp:${connection.port}`); }
    finally { await fixture?.close(); }
  }
})().catch((error) => { console.error(error.stack); process.exitCode = 1; });
