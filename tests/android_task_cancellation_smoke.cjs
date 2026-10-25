#!/usr/bin/env node
// Exercise the actual installed Tauri command, including unresolved magnets.
// Requires a running debug app and Node 22+. Defaults to the emulator.
const { execFileSync } = require('node:child_process');
const { randomBytes } = require('node:crypto');
const path = require('node:path');
const os = require('node:os');

const serial = process.argv[2] || 'emulator-5554';
const sdk = process.env.ANDROID_HOME || path.join(os.homedir(), 'Library/Android/sdk');
const adb = process.env.LOV_ADB || path.join(sdk, 'platform-tools/adb');
const run = (...args) => execFileSync(adb, ['-s', serial, ...args], { encoding: 'utf8' }).trim();

async function evaluate(url, expression) {
  const socket = new WebSocket(url);
  try {
    return await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error('Cancellation smoke test timed out')), 45000);
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
  } finally {
    socket.close();
  }
}

(async () => {
  const pid = run('shell', 'pidof', 'com.abamaxa.lots_of_videos');
  if (!/^\d+$/.test(pid)) throw new Error('Open Lots of Videos before running this test');
  const port = run('forward', 'tcp:0', `localabstract:webview_devtools_remote_${pid}`);
  try {
    const pages = await (await fetch(`http://127.0.0.1:${port}/json`, { signal: AbortSignal.timeout(10000) })).json();
    const page = pages.find((item) => item.type === 'page');
    if (!page) throw new Error('No app WebView found');
    const hash = randomBytes(20).toString('hex');
    const result = await evaluate(page.webSocketDebuggerUrl, `(${async (hash) => {
      const invoke = window.__TAURI_INTERNALS__.invoke;
      const key = 'magnet:?xt=urn:btih:' + hash + '&dn=Cancellation%20smoke%20test';
      // This fails before touching any real task when the IPC signature is broken.
      await invoke('tasks_delete', { key: 'missing-cancellation-probe-' + hash });
      let queued = false;
      try {
        await invoke('tasks_add', { payload: { engine: 'Torrent', name: 'Cancellation smoke test ' + hash,
          link: key, series: 'Cancellation smoke test' } });
        queued = true;
        const running = await invoke('tasks_list');
        if (running.error) throw new Error(running.error);
        if (!running.results.some((task) => task.key === key)) throw new Error('Probe torrent did not register');
        await invoke('tasks_delete', { key });
        await invoke('tasks_delete', { key }); // repeated cancellation must also succeed
        const stopped = await invoke('tasks_list');
        if (stopped.error) throw new Error(stopped.error);
        if (stopped.results.some((task) => task.key === key)) throw new Error('Canceled torrent is still registered');
        return { missingKey: 'passed', unresolvedTorrent: 'canceled', repeatedCancellation: 'passed' };
      } finally {
        if (queued) await invoke('tasks_delete', { key });
      }
    }})(${JSON.stringify(hash)})`);
    console.log('PASS installed Android task cancellation:', JSON.stringify(result));
  } finally {
    run('forward', '--remove', `tcp:${port}`);
  }
})().catch((error) => { console.error(error.message); process.exitCode = 1; });
