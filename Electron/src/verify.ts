import { clipboard, ClipboardItem, WebContentsView } from 'electron';
import assert from 'node:assert/strict';
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { browserStore, sandbox, type BrowserSession } from './browser.js';
import type { Store } from './store.js';

type Owner = { store: Store; views: Map<string, BrowserSession> };
type Context = { launcher: WebContentsView; live: Owner; demo: Owner };
type FixtureState = { id: string; cookie: string; admissions: boolean; bridge: string };
type Proof = { ids: string[]; pages: FixtureState[]; demoProfile: string };

export async function verify(phase: string, context: Context) {
  const originals = phase === 'create' ? await clipboard.read() : undefined;
  const saved = originals && await Promise.all(originals.map(async item => new ClipboardItem(
    Object.fromEntries(await Promise.all(item.types.map(async type => [type, await item.getType(type)]))),
  )));
  try {
    await runChecks(phase, context);
  } finally {
    if (saved) {
      const current = await clipboard.readText();
      if (context.demo.store.links.some(link => link.url === current)) {
        if (saved.length) await clipboard.write(saved);
        else clipboard.clear();
      }
    }
  }
}

async function runChecks(phase: string, { launcher, live, demo }: Context) {
  const js = <T>(source: string): Promise<T> => launcher.webContents.executeJavaScript(source);
  const text = (selector: string) => js<string>('document.querySelector(' + JSON.stringify(selector) + ').textContent');
  const click = (selector: string) => js('document.querySelector(' + JSON.stringify(selector) + ').click()');
  const call = <T>(name: string, payload?: unknown) =>
    js<{ value: T; error?: string }>('launcher.action(' + JSON.stringify([name, payload]) + '[0], ' +
      JSON.stringify([name, payload]) + '[1])');
  const recordPath = join(process.env.QUEUE_LAUNCHER_TEST_DATA!, 'proof.json');
  const liveUrl = process.env.QUEUE_LAUNCHER_TEST_URL!;
  const selected = (owner: Owner) => owner.views.get(owner.store.state.batch!.selected)!;

  async function wait(check: () => unknown | Promise<unknown>, description: string) {
    const deadline = Date.now() + 18000;
    do {
      if (await check()) return;
      await delay(50);
    } while (Date.now() < deadline);
    throw new Error(description);
  }

  const button = (number: number) => '#sessions li:nth-child(' + number + ') .session-select';
  const memberState = (browser: BrowserSession): Promise<FixtureState> =>
    browser.contents.executeJavaScript(String.raw`({
      id: localStorage.getItem('local-queue-id'),
      cookie: document.cookie,
      admissions: document.querySelector('#human').checked,
      bridge: typeof launcher + '/' + typeof require + '/' + typeof process
    })`);
  const loaded = (owner: Owner, size: number) => owner.views.size === size &&
    [...owner.views.values()].every(view => !view.loading && !view.error);
  const hasText = (selector: string, value: string) => async () => (await text(selector)).includes(value);
  const writeInput = (url: string, count: number) => js(
    'document.querySelector("#release-url").value = ' + JSON.stringify(url) + ';' +
    'document.querySelector("#count").value = ' + JSON.stringify(count));
  await wait(() => js('!document.querySelector("#demo").disabled'), 'launcher controls never initialized');

  if (phase === 'create') {
    assert((await call('open', { url: 'file:///secret.html', count: 2 })).error);
    assert((await call('open', { url: liveUrl, count: 51 })).error);
    assert((await call('open', { url: 'https://person:secret@example.test', count: 1 })).error);
    await writeInput(liveUrl, 2);
    await click('#open');
    await wait(() => loaded(live, 2), 'live pages did not load');
    await wait(() => js('document.querySelectorAll("#sessions li").length === 2'), 'rows did not render');
    const [first, second] = [...live.views.values()];
    await first.contents.executeJavaScript("document.querySelector('#human').click(); document.querySelector('#increment').click()");
    const a = await memberState(first);
    const b = await memberState(second);
    assert(a.id && b.id && a.id !== b.id && a.admissions && !b.admissions);
    assert(a.cookie.startsWith('fixture-cookie=') && b.cookie.startsWith('fixture-cookie=') && a.cookie !== b.cookie);
    assert.equal(a.bridge, 'undefined/undefined/undefined');
    await click(button(2));
    await wait(() => second.view.getVisible() && !first.view.getVisible(), 'session switch did not update visibility');
    await click('#next');
    await wait(() => first.view.getVisible(), 'next did not wrap');
    assert.equal(await first.contents.executeJavaScript("document.querySelector('#counter').textContent"), '1');
    await click('#mark');
    await wait(hasText('#ready', '1 of 2 ready'), 'ready count did not update');

    await first.contents.executeJavaScript("document.querySelector('#next-page').click(); void 0", true);
    await wait(() => first.contents.getURL().endsWith('/next'), 'second page did not load');
    await wait(() => !first.loading, 'second page did not finish');
    await wait(() => js('!document.querySelector("#back").disabled'), 'Back control did not enable');
    await click('#back');
    await wait(() => !first.loading && first.contents.getURL() === liveUrl, 'back did not restore fixture');
    await first.contents.executeJavaScript("document.querySelector('#increment').click()");
    const reloaded = new Promise<void>(resolve => first.contents.once('did-finish-load', () => resolve()));
    await click('#reload');
    await reloaded;
    assert.equal(await first.contents.executeJavaScript("document.querySelector('#counter').textContent"), '0');
    assert.equal((await memberState(first)).id, a.id);

    await click('#add');
    await wait(() => loaded(live, 3), 'add did not open a third session');
    const third = selected(live);
    const thirdContents = third.contents;
    await wait(hasText('#selected', 'Session 3'), 'added session did not become active');
    await click('#close');
    await wait(() => live.views.size === 2 && thirdContents.isDestroyed(), 'close did not remove third session');
    await wait(() => js('!document.querySelector("#demo").disabled'), 'close action did not finish');
    const emptyProfile = browserStore(live.store, third.id);
    assert.equal((await emptyProfile.cookies.get({ url: liveUrl })).length, 0);
    const probe = new WebContentsView({ webPreferences: { ...sandbox, session: emptyProfile } });
    try {
      await probe.webContents.loadURL(new URL('/next', liveUrl).href);
      assert(await probe.webContents.executeJavaScript('localStorage.length === 0 && document.cookie === ""'));
    } finally {
      probe.webContents.close();
    }
    const liveSelection = selected(live);
    await liveSelection.contents.executeJavaScript("document.querySelector('#increment').click()");

    await click('#demo');
    await wait(() => loaded(demo, 12), 'demo batch did not load twelve pages');
    await wait(() => demo.store.links.length === 12, 'demo share links were not captured');
    await click('#sort');
    await wait(() => demo.store.state.sort, 'sort did not persist');
    const ordered = await call<{ sessions: { id: string; ahead: number }[] }>('state');
    assert(ordered.value.sessions.every((entry, index, entries) => !index || entries[index - 1].ahead <= entry.ahead));
    await click(button(1));
    await wait(() => demo.store.state.batch!.selected === ordered.value.sessions[0].id, 'sorted selection did not update');
    await click('#next');
    await wait(() => demo.store.state.batch!.selected === ordered.value.sessions[1].id, 'next did not follow sorted order');
    const demoProfile = selected(demo).id;
    await click('#copyClose');
    await wait(() => demo.views.size === 11, 'copy and close did not close the session');
    assert((await clipboard.readText()).startsWith('https://queue-launcher.example/'));
    await wait(() => js('!document.querySelector("#history").disabled'), 'clipboard action did not finish');
    await click('#history');
    await wait(() => js('document.querySelector("#history-dialog").open'), 'history dialog did not open');
    await wait(() => [...demo.views.values()].every(browser => !browser.view.getVisible()), 'browser covered the history dialog');
    assert.equal(await js('document.querySelector("#history-rows").children.length'), 12);
    await click('#history-rows tr:first-child button');
    await wait(hasText('#history-feedback', 'Copied Session'), 'copying saved history did not work');
    await click('#history-dialog header button');
    await click('#demo');
    await wait(() => liveSelection.view.getVisible(), 'exiting demo did not restore live session');
    assert.equal(await liveSelection.contents.executeJavaScript("document.querySelector('#counter').textContent"), '1');
    assert.equal(live.store.links.length, 0);
    assert.equal(demo.store.links.length, 12);
    const proof: Proof = { ids: [first.id, second.id, third.id], pages: [a, b], demoProfile };
    writeFileSync(recordPath, JSON.stringify(proof));
    console.log('PASS live sessions, cookies, isolation, switch/next, readiness, add/close, back/reload, demo, sort and clipboard/history');
    return;
  }

  if (phase === 'restore') {
    const proof: Proof = JSON.parse(readFileSync(recordPath, 'utf8'));
    await wait(() => loaded(live, 2), 'live batch did not reopen');
    assert.deepEqual([...live.views.keys()], proof.ids.slice(0, 2));
    assert.deepEqual(await Promise.all([...live.views.values()].map(memberState)), proof.pages);
    assert.equal(live.store.state.batch!.sessions[0].ready, true);
    assert(!existsSync(live.store.profilePath(proof.ids[2])), 'closed live profile remained on disk');
    assert(!existsSync(demo.store.profilePath(proof.demoProfile)), 'closed demo profile remained on disk');

    await click('#demo');
    await wait(() => loaded(demo, 11), 'demo did not restore its own batch');
    assert(demo.store.state.sort && demo.store.links.length === 12);
    await click('#end');
    await wait(() => js('document.querySelector("#end-dialog").open'), 'end confirmation did not open');
    await click('#end-dialog button[value="cancel"]');
    assert.equal(demo.views.size, 11);
    await click('#end');
    await click('#confirm-end');
    await wait(() => demo.views.size === 0 && !demo.store.state.batch, 'end batch did not close demo');
    await wait(() => js('!document.querySelector("#demo").disabled'), 'end cleanup did not finish');
    assert.equal(demo.store.links.length, 12);
    await click('#demo');
    await wait(() => js('document.querySelector("#demo-label").hidden'), 'live mode did not return');
    await click('#end');
    await click('#confirm-end');
    await wait(() => live.views.size === 0 && !live.store.state.batch, 'end batch did not close live');
    await wait(() => js('!document.querySelector("#demo").disabled'), 'live end cleanup did not finish');

    for (const owner of [live, demo]) {
      const rows = readdirSync(owner.store.logs).flatMap(file =>
        readFileSync(join(owner.store.logs, file), 'utf8').trim().split('\n').map(line => JSON.parse(line)));
      assert(rows.some(row => row.event === 'batch_ended'));
      assert(rows.every(row => Object.keys(row).every(key =>
        ['timestamp', 'event', 'session', 'code', 'membersAhead'].includes(key))));
      assert(rows.every(row => ['session', 'code', 'membersAhead'].every(key =>
        row[key] === undefined || Number.isInteger(row[key]))));
      assert(!JSON.stringify(rows).includes('http') && !JSON.stringify(rows).includes('fixture-cookie'));
    }
    console.log('PASS same stores after restart, physical reclamation, independent demo state, confirmed end batch and private logs');
    return;
  }

  assert.equal(phase, 'limit');
  assert.equal(live.store.state.pending.length + demo.store.state.pending.length, 0);
  assert.equal(live.store.links.length, 0);
  assert.equal(demo.store.links.length, 12);
  assert(!live.store.state.batch && !demo.store.state.batch);
  await writeInput(liveUrl, 50);
  await click('#open');
  await wait(() => loaded(live, 50), '50 sessions failed to load');
  assert.equal(live.store.state.batch!.sessions.length, 50);
  await call('add');
  assert.equal(live.views.size, 50, 'session cap did not hold');
  assert(await js('document.querySelector("#add").disabled'));
  console.log('PASS end-batch cleanup and link history survived restart, 50-session cap');
}
