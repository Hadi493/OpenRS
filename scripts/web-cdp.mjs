#!/usr/bin/env node
// Drives a headless Chromium page over the DevTools protocol for the OpenRS
// web smoke test.
//
// Why this exists
// ---------------
// `--virtual-time-budget --dump-dom` used to be enough, but it dumps the DOM as
// soon as the virtual clock runs out, which happens before this app's wasm
// finishes instantiating: the page is still showing `#loading_text` and no
// console output has happened yet. Waiting over CDP measures the thing the test
// actually cares about -- the app removing `#loading_text` -- instead of a
// timer that happens to have expired.
//
// What it does
// ------------
// 1. Attaches to the first page target of a running Chromium (`--remote-debugging-port`).
// 2. Navigates to the page and polls its state until the app has started, or
//    until the deadline passes.
// 3. Collects console messages and uncaught exceptions.
// 4. Captures a screenshot when the app started.
//
// Prints one line per console message, then `state: {...}`, then `result: ok`
// or `result: timeout`. Exits 0 only on `result: ok`.
//
// Usage: node scripts/web-cdp.mjs <debug-url> <page-url> <screenshot.png> <millis>

const [debugUrl, pageUrl, screenshotPath, budgetText] = process.argv.slice(2);
const budgetMillis = Number(budgetText || 60_000);

if (!debugUrl || !pageUrl || !screenshotPath || !Number.isFinite(budgetMillis)) {
  console.error(
    'usage: web-cdp.mjs <debug-url> <page-url> <screenshot.png> <millis>'
  );
  process.exit(2);
}

const targets = await fetch(`${debugUrl}/json/list`).then((response) =>
  response.json()
);
const page = targets.find((target) => target.type === 'page');
if (!page) {
  console.error('result: no-page-target');
  process.exit(1);
}

const consoleLines = [];
const socket = new WebSocket(page.webSocketDebuggerUrl);
let nextId = 0;

function send(method, params = {}) {
  socket.send(JSON.stringify({ id: ++nextId, method, params }));
}

function evaluate(expression) {
  const id = ++nextId;
  socket.send(
    JSON.stringify({ id, method: 'Runtime.evaluate', params: { expression } })
  );
  return id;
}

const pendingEvaluations = new Map();

socket.addEventListener('message', (event) => {
  const message = JSON.parse(event.data);

  if (message.method === 'Runtime.consoleAPICalled') {
    const text = (message.params.args || [])
      .map((arg) => arg.value ?? arg.description ?? JSON.stringify(arg))
      .join(' ');
    consoleLines.push(`console.${message.params.type}: ${text}`);
  } else if (message.method === 'Runtime.exceptionThrown') {
    const details = message.params.exceptionDetails;
    consoleLines.push(
      `exception: ${details.text} ${details.exception?.description ?? ''}`
    );
  } else if (message.method === 'Log.entryAdded') {
    consoleLines.push(
      `log.${message.params.entry.level}: ${message.params.entry.text}`
    );
  } else if (message.id && pendingEvaluations.has(message.id)) {
    const resolve = pendingEvaluations.get(message.id);
    pendingEvaluations.delete(message.id);
    resolve(message);
  }
});

function pageState() {
  return new Promise((resolve) => {
    const id = evaluate(
      "JSON.stringify({isolated: crossOriginIsolated, canvas: !!document.getElementById('the_canvas_id'), loading: !!document.getElementById('loading_text')})"
    );
    pendingEvaluations.set(id, resolve);
  });
}

function wait(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function finish(code, result) {
  for (const line of consoleLines) {
    console.log(line);
  }
  process.exitCode = code;
  console.log(`result: ${result}`);
  // Give the socket a tick to flush before the process ends.
  setTimeout(() => process.exit(code), 50);
}

socket.addEventListener('open', () => {
  send('Runtime.enable');
  send('Log.enable');
  send('Page.enable');
  send('Page.navigate', { url: pageUrl });
});

socket.addEventListener('error', () => finish(1, 'websocket-error'));

const deadline = Date.now() + budgetMillis;
let lastState = null;

while (Date.now() < deadline) {
  await wait(250);

  const reply = await pageState();
  const value = reply?.result?.result?.value;
  if (value) {
    lastState = JSON.parse(value);
    if (lastState.canvas && !lastState.loading) {
      // The app removed `#loading_text`, which only happens after
      // `WebRunner::start` resolves.
      break;
    }
  }
}

console.log(`state: ${JSON.stringify(lastState)}`);
if (lastState && lastState.canvas && !lastState.loading) {
  const shotId = ++nextId;
  const screenshot = new Promise((resolve) => {
    pendingEvaluations.set(shotId, resolve);
  });
  socket.send(
    JSON.stringify({
      id: shotId,
      method: 'Page.captureScreenshot',
      params: { format: 'png' },
    })
  );
  const reply = await screenshot;
  const data = reply?.result?.data;
  if (data) {
    const { writeFileSync } = await import('node:fs');
    writeFileSync(screenshotPath, Buffer.from(data, 'base64'));
  }
  finish(0, 'ok');
} else {
  finish(1, 'timeout');
}
