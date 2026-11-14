import { app, WebContentsView, type Session } from 'electron';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

type Entry = { id: string; label: string };
type Snapshot = { queueId: string; cookie: string; admitted: boolean; counter: string };
type Context = {
  windowShownMs: number;
  launcher: WebContentsView;
  views: Map<string, WebContentsView>;
  origin: string;
  queueSession(id: string): Session;
};

export async function verify(mode: string, context: Context) {
  const { launcher, views, origin, queueSession } = context;
  const proofPath = join(app.getPath('userData'), 'verification.json');
  const evaluate = <T>(script: string): Promise<T> => launcher.webContents.executeJavaScript(script);
  const state = () => evaluate<{ entries: Entry[]; active?: string; savedLink?: string }>('launcher.state()');
  const add = () => evaluate('launcher.add()');
  const select = (id: string) => evaluate('launcher.select(' + JSON.stringify(id) + ')');
  const viewFor = (id: string) => {
    const view = views.get(id);
    assert(view, 'session has a view');
    return view;
  };
  const snapshot = (id: string): Promise<Snapshot> => viewFor(id).webContents.executeJavaScript(`
    ({
      queueId: localStorage.getItem('local-queue-id'),
      cookie: document.cookie,
      admitted: document.querySelector('#human').checked,
      counter: document.querySelector('#counter').textContent
    })
  `);
  const waitForLauncher = async (condition: string) => {
    const deadline = performance.now() + 5000;
    while (!(await evaluate(condition))) {
      assert(performance.now() < deadline, 'launcher UI did not update');
      await delay(20);
    }
  };
  await waitForLauncher("document.querySelector('#count').textContent.includes('/')");

  if (mode === 'create') {
    await evaluate("document.querySelector('#add').click()");
    await waitForLauncher("document.querySelector('#sessions').children.length === 1 && !document.querySelector('#add').disabled");
    await evaluate("document.querySelector('#add').click()");
    await waitForLauncher("document.querySelector('#sessions').children.length === 2 && !document.querySelector('#add').disabled");
    await evaluate("document.querySelector('#saveLink').click()");
    await waitForLauncher("document.querySelector('#saved').textContent.includes('queue-launcher.example')");

    const { entries, savedLink } = await state();
    const [first, second] = entries.map(entry => entry.id);
    await viewFor(first).webContents.executeJavaScript(`
      document.querySelector('#human').click();
      document.querySelector('#increment').click();
    `);
    const left = await snapshot(first);
    const right = await snapshot(second);
    assert(left.queueId && right.queueId && left.queueId !== right.queueId, 'local storage is isolated');
    assert(left.cookie.startsWith('fixture-cookie=') && right.cookie.startsWith('fixture-cookie=') &&
      left.cookie !== right.cookie, 'cookies are isolated');
    assert(left.admitted && !right.admitted, 'changing admission affects one session');
    assert(!await viewFor(first).webContents.executeJavaScript(
      "typeof window.launcher !== 'undefined' || typeof require !== 'undefined' || typeof process !== 'undefined'"
    ), 'fixture has no host bridge or Node globals');

    const original = viewFor(first).webContents;
    for (const index of [0, 1, 0]) {
      await evaluate('document.querySelectorAll("#sessions button")[' + index + '].click()');
      await waitForLauncher('document.querySelectorAll("#sessions button")[' + index +
        '].getAttribute("aria-pressed") === "true" && !document.querySelector("#add").disabled');
      assert(viewFor(entries[index].id).getVisible(), 'selected view is visible');
      assert(!viewFor(entries[1 - index].id).getVisible(), 'other view is hidden');
    }
    assert(viewFor(first).webContents === original && (await snapshot(first)).counter === '1',
      'switching kept the same renderer and its page-only counter');
    writeFileSync(proofPath, JSON.stringify({ first, second, left, right, savedLink }));
    console.log('PASS two views, cookie and local storage isolation, sandbox, switching without reload');
    return;
  }

  if (mode === 'restore' || mode === 'after-close') {
    const proof = JSON.parse(readFileSync(proofPath, 'utf8')) as {
      first: string; second: string; left: Snapshot; right: Snapshot; savedLink: string;
    };
    const { first, second, left, right, savedLink } = proof;
    const restored = await state();
    assert(savedLink && restored.savedLink === savedLink, 'sample link survived');
    const expectedIds = mode === 'restore' ? [first, second] : [second];
    assert(restored.entries.map(entry => entry.id).join('/') === expectedIds.join('/'), 'saved session IDs restored');
    async function compare(id: string, expected: Snapshot) {
      const current = await snapshot(id);
      assert(current.queueId === expected.queueId && current.cookie === expected.cookie &&
        current.admitted === expected.admitted, 'same session data survived');
      assert(current.counter === '0', 'page-only state reset on process relaunch');
    }
    await compare(second, right);
    if (mode === 'restore') {
      await compare(first, left);
      await select(first);
      const closed = viewFor(first).webContents;
      await evaluate("document.querySelector('#close').click()");
      await waitForLauncher("document.querySelector('#sessions').children.length === 1 && !document.querySelector('#add').disabled");
      assert(closed.isDestroyed() && !views.has(first), 'closing destroys the selected web contents');
      assert((await state()).savedLink === savedLink, 'closing retained the sample link');
      await compare(second, right);
    }

    const store = queueSession(first);
    assert((await store.cookies.get({ url: origin })).length === 0, 'closed store has no fixture cookies');
    const probe = new WebContentsView({
      webPreferences: { session: store, sandbox: true, contextIsolation: true, nodeIntegration: false },
    });
    try {
      await probe.webContents.loadURL(origin + '/empty');
      assert(await probe.webContents.executeJavaScript('localStorage.length === 0 && document.cookie === ""'),
        'closed store has empty local storage and cookies on the same origin');
    } finally {
      probe.webContents.close();
    }
    console.log(mode === 'restore'
      ? 'PASS process restart, same stores, selective cleanup, saved sample link'
      : 'PASS cleanup still holds after another process restart');
    return;
  }

  const count = mode === 'scale-12' ? 12 : mode === 'scale-50' ? 50 : 0;
  assert(count, 'unknown verification mode');
  const summary = async (phase: string) => {
    const metrics = app.getAppMetrics();
    const rendererPids = new Set([...views.values()].map(view => view.webContents.getOSProcessId()));
    const pages = await Promise.all([...views.keys()].map(snapshot));
    assert(new Set(pages.map(page => page.queueId)).size === count &&
      pages.every(page => page.queueId), 'all queue IDs are distinct');
    assert(new Set(pages.map(page => page.cookie)).size === count &&
      pages.every(page => page.cookie.startsWith('fixture-cookie=')), 'all cookies are distinct');
    const entries = (await state()).entries;
    const switches: number[] = [];
    for (const [index, entry] of entries.entries()) {
      const start = performance.now();
      await select(entry.id);
      const view = viewFor(entry.id);
      assert(view.getVisible(), 'selected view is visible');
      await view.webContents.executeJavaScript(
        'new Promise(resolve => requestAnimationFrame(() => resolve()))'
      );
      switches.push(Math.round(performance.now() - start));
      assert([...views.values()].filter(view => view.getVisible()).length === 1,
        'only one view is visible at switch ' + index);
    }
    switches.sort((left, right) => left - right);
    console.log(JSON.stringify({
      phase, views: count, queueRendererProcesses: rendererPids.size, electronProcesses: metrics.length,
      summedWorkingSetMiB: Math.round(metrics.reduce((total, process) =>
        total + process.memory.workingSetSize, 0) / 1024),
      switchToAnimationFrameMedianMs: switches[Math.floor(switches.length / 2)],
      switchToAnimationFrameP95Ms: switches[Math.ceil(switches.length * 0.95) - 1],
      switchToAnimationFrameMaxMs: switches.at(-1),
    }));
  };
  if (views.size === 0) {
    const start = performance.now();
    for (let index = 0; index < count; index++) await add();
    assert((await state()).entries.length === count, 'all views opened');
    if (count === 50) {
      await add();
      assert((await state()).entries.length === 50, '51st view rejected');
    }
    console.log(JSON.stringify({ views: count, sequentialAddMs: Math.round(performance.now() - start) }));
    await summary('added');
  } else {
    assert(views.size === count, 'full batch restored');
    console.log(JSON.stringify({ views: count, processToWindowMs: context.windowShownMs }));
    await summary('restored');
  }
}
