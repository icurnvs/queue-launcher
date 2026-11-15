import {
  app, BaseWindow, clipboard, dialog, ipcMain, Menu, protocol, shell, WebContentsView, type IpcMainEvent,
  type IpcMainInvokeEvent, type Rectangle,
} from 'electron';
import { randomUUID } from 'node:crypto';
import { mkdirSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { allowedUrl, BrowserSession, browserStore, demoFixtureUrl, sandbox } from './browser.js';
import { Store, type Batch, type SavedSession } from './store.js';

const limit = 50;
const ui = join(__dirname, '..', 'ui');
const launcherFile = join(ui, 'index.html');
const launcherUrl = pathToFileURL(launcherFile).href;
const testMode = !app.isPackaged && process.env.QUEUE_LAUNCHER_TEST;
protocol.registerSchemesAsPrivileged([
  { scheme: 'queue-fixture', privileges: { standard: true, secure: true } },
]);
app.setName('Queue Launcher');
if (process.platform === 'win32') app.setAppUserModelId('com.queue-launcher.desktop');
const directory = testMode
  ? resolve(process.env.QUEUE_LAUNCHER_TEST_DATA!)
  : join(app.getPath('appData'), 'Queue Launcher Electron');
const chromiumDirectory = join(directory, 'Chromium');
mkdirSync(chromiumDirectory, { recursive: true });
app.setPath('userData', directory);
app.setPath('sessionData', chromiumDirectory);

type Mode = {
  store: Store;
  views: Map<string, BrowserSession>;
  openErrors: Set<string>;
  demo: boolean;
  opened: boolean;
  notice?: string;
};
function createMode(demo: boolean): Mode {
  return {
    store: new Store(join(directory, demo ? 'Demo' : 'Live')),
    views: new Map(), openErrors: new Set(), demo, opened: false,
  };
}

async function launch() {
  await app.whenReady();
  const fixture = readFileSync(join(ui, 'fixture.html'), 'utf8');
  const live = createMode(false);
  const demo = createMode(true);
  for (const owner of [live, demo]) {
    if (owner.store.reclaimProfiles()) owner.notice = 'Some closed profiles could not be removed. They will be retried on restart.';
  }
  let mode = live;
  let pane: Rectangle | undefined;
  let blocked = false;
  let closing = false;
  let closed = false;
  let publishQueued = false;
  let operations = Promise.resolve();
  const window = new BaseWindow({
    title: 'Queue Launcher', width: 1280, height: 830, minWidth: 760, minHeight: 560, show: false,
  });
  app.on('second-instance', () => {
    if (closing) return;
    if (window.isMinimized()) window.restore();
    window.focus();
  });
  const launcher = new WebContentsView({ webPreferences: { ...sandbox, preload: join(ui, 'preload.cjs') } });
  window.contentView.addChildView(launcher);
  launcher.webContents.on('will-frame-navigate', event => event.preventDefault());
  launcher.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  Menu.setApplicationMenu(Menu.buildFromTemplate([
    ...(process.platform === 'darwin' ? [{
      label: app.name, submenu: [{ role: 'about' as const }, { type: 'separator' as const },
        { label: 'Quit Queue Launcher', accelerator: 'Cmd+Q', click: () => window.close() }],
    }] : []),
    { role: 'editMenu' },
  ]));

  const active = () => mode.store.state.batch?.selected;
  const current = () => mode.views.get(active() ?? '');

  function list(owner = mode) {
    const entries = owner.store.state.batch?.sessions ?? [];
    if (!owner.store.state.sort) return entries;
    const position = (id: string) => owner.views.get(id)?.status?.ahead ?? Infinity;
    return [...entries].sort((a, b) => position(a.id) - position(b.id) || a.number - b.number);
  }

  function state() {
    return {
      demo: mode.demo, sort: mode.store.state.sort, active: active(), limit,
      totalReady: mode.store.state.batch?.sessions.filter(entry => entry.ready).length ?? 0,
      sessions: list().map(entry => {
        const browser = mode.views.get(entry.id);
        const status = browser?.status;
        return {
          id: entry.id, number: entry.number, ready: entry.ready,
          loading: browser?.loading ?? !mode.openErrors.has(entry.id),
          error: mode.openErrors.has(entry.id) ? 'Could not open this browser profile.' : browser?.error,
          ahead: status?.ahead, wait: status?.wait, remaining: status?.remaining,
          purchaseWindow: status?.purchaseWindow, message: status?.message, code: status?.code,
          waiting: status?.waiting, shareable: Boolean(browser?.link),
          canGoBack: browser?.contents.navigationHistory.canGoBack() ?? false,
        };
      }),
      notice: mode.notice,
    };
  }

  function layout() {
    if (closed) return;
    const { width, height } = window.getContentBounds();
    launcher.setBounds({ x: 0, y: 0, width, height });
    const browser = current();
    if (browser && pane && !blocked && !browser.error) {
      const x = Math.min(pane.x, width);
      const y = Math.min(pane.y, height);
      browser.view.setBounds({ x, y, width: Math.min(pane.width, width - x), height: Math.min(pane.height, height - y) });
    }
    for (const owner of [live, demo]) {
      for (const view of owner.views.values()) {
        const visible = view === browser && Boolean(pane?.width && pane.height) && !blocked && !view.error;
        if (view.view.getVisible() !== visible) view.view.setVisible(visible);
      }
    }
  }

  function refresh() {
    if (closing || publishQueued) return;
    publishQueued = true;
    setImmediate(() => {
      publishQueued = false;
      if (closing) return;
      layout();
      launcher.webContents.send('launcher:state', state());
    });
  }

  function record(owner: Mode, id: string, name: string, number?: number, code?: number, ahead?: number) {
    try {
      owner.store.log(id, name, number, code, ahead);
    } catch {
      owner.notice = 'Could not write drop logs. Check the app data folder.';
    }
  }

  function mount(owner: Mode, batch: Batch, entry: SavedSession) {
    owner.openErrors.delete(entry.id);
    try {
      const startUrl = allowedUrl(batch.url, owner.demo);
      if (!startUrl) throw new Error('Invalid saved release URL.');
      const browser = new BrowserSession(entry.id, startUrl, owner.store, fixture, {
        refresh, log: (event, code, ahead) => record(owner, batch.id, event, entry.number, code, ahead),
      });
      owner.views.set(entry.id, browser);
      window.contentView.addChildView(browser.view);
      browser.open(entry);
    } catch {
      dispose(owner, entry.id);
      owner.openErrors.add(entry.id);
      record(owner, batch.id, 'webview_initialization_failed', entry.number);
    }
  }

  function restore(owner: Mode) {
    if (owner.opened) return;
    const batch = owner.store.state.batch;
    if (batch) {
      record(owner, batch.id, 'batch_restored');
      for (const entry of batch.sessions) {
        record(owner, batch.id, 'session_restored', entry.number);
        mount(owner, batch, entry);
      }
    }
    owner.opened = true;
  }

  function saveBatch(batch: Batch | undefined, owner = mode) {
    owner.store.save({ ...owner.store.state, batch });
  }

  function start(urlInput: unknown, countInput: unknown) {
    if (mode.store.state.batch) return;
    const url = allowedUrl(urlInput, mode.demo);
    if (!url) throw new Error('Enter an HTTP or HTTPS release URL or use the offline fixture.');
    if (typeof countInput !== 'number' || !Number.isInteger(countInput) || countInput < 1 || countInput > limit) {
      throw new Error('Enter a session count from 1 to 50.');
    }
    const sessions = Array.from({ length: countInput }, (_, index) => ({
      id: randomUUID(), number: index + 1, ready: false,
    }));
    const batch = { id: randomUUID(), url, nextNumber: countInput + 1, selected: sessions[0].id, sessions };
    saveBatch(batch);
    record(mode, batch.id, 'batch_started');
    for (const entry of sessions) {
      record(mode, batch.id, 'session_opened', entry.number);
      mount(mode, batch, entry);
    }
  }

  function dispose(owner: Mode, id: string) {
    owner.openErrors.delete(id);
    const browser = owner.views.get(id);
    if (!browser) return;
    owner.views.delete(id);
    window.contentView.removeChildView(browser.view);
    browser.contents.close();
  }

  async function capture(owner: Mode, id: string, browser: BrowserSession) {
    const result = await browser.observe();
    const batch = owner.store.state.batch;
    if (closing || !result || !batch || owner.views.get(id) !== browser) return;
    const entry = batch.sessions.find(item => item.id === id);
    if (!entry) return;
    try {
      if (result.link && result.ahead !== undefined) owner.store.saveLink(batch.id, entry, result.link, result.ahead);
      if (result.resume && result.resume !== entry.resume) {
        saveBatch({ ...batch, sessions: batch.sessions.map(item => item === entry
          ? { ...item, resume: result.resume } : item) }, owner);
      }
    } catch {
      owner.notice = 'Could not save queue links or resume data. Check the app data folder.';
    }
  }

  async function remove(all: boolean, id = active()) {
    const owner = mode;
    const batch = owner.store.state.batch;
    if (!batch) return;
    const items = all ? batch.sessions : batch.sessions.filter(entry => entry.id === id);
    if (!items.length) return;
    const ids = new Set(items.map(entry => entry.id));
    const sessions = batch.sessions.filter(entry => !ids.has(entry.id));
    const ordered = list(owner);
    const index = ordered.findIndex(entry => entry.id === batch.selected);
    const selected = all ? undefined : ids.has(batch.selected)
      ? (ordered[index + 1] ?? ordered[index - 1])?.id : batch.selected;
    owner.store.save({
      ...owner.store.state, pending: [...owner.store.state.pending, ...ids],
      batch: selected ? { ...batch, sessions, selected } : undefined,
    });
    for (const entry of items) {
      dispose(owner, entry.id);
      record(owner, batch.id, 'session_closed', entry.number);
    }
    if (!sessions.length) record(owner, batch.id, 'batch_ended');
    refresh();
    const cleared = await Promise.allSettled(items.map(async entry => {
      const profile = browserStore(owner.store, entry.id);
      await profile.clearData();
      profile.flushStorageData();
      await profile.cookies.flushStore();
    }));
    return cleared.some(result => result.status === 'rejected')
      ? 'Some browser data could not be cleared. The app will retry on restart.' : undefined;
  }

  function authenticate(event: IpcMainEvent | IpcMainInvokeEvent) {
    return !closing && event.sender === launcher.webContents &&
      event.senderFrame === launcher.webContents.mainFrame && event.senderFrame.url === launcherUrl;
  }

  async function action(name: unknown, payload: unknown): Promise<unknown> {
    const batch = mode.store.state.batch;
    const entry = batch?.sessions.find(item => item.id === batch.selected);
    const browser = current();
    switch (name) {
      case 'state': return state();
      case 'open': {
        const form = payload as { url?: unknown; count?: unknown } | undefined;
        start(form?.url, form?.count);
        break;
      }
      case 'select':
        if (batch && batch.sessions.some(item => item.id === payload)) saveBatch({ ...batch, selected: payload as string });
        break;
      case 'next': {
        const ordered = list();
        if (batch && ordered.length > 1) {
          const index = ordered.findIndex(item => item.id === batch.selected);
          saveBatch({ ...batch, selected: ordered[(index + 1) % ordered.length].id });
        }
        break;
      }
      case 'add': {
        if (!batch || batch.sessions.length >= limit) break;
        const added = { id: randomUUID(), number: batch.nextNumber, ready: false };
        const next = { ...batch, nextNumber: added.number + 1, sessions: [...batch.sessions, added], selected: added.id };
        saveBatch(next);
        record(mode, batch.id, 'session_opened', added.number);
        mount(mode, next, added);
        break;
      }
      case 'mark':
        if (batch && entry) {
          const ready = !entry.ready;
          saveBatch({ ...batch, sessions: batch.sessions.map(item => item === entry ? { ...item, ready } : item) });
          record(mode, batch.id, ready ? 'marked_ready' : 'marked_pending', entry.number);
        }
        break;
      case 'sort':
        if (typeof payload === 'boolean') mode.store.save({ ...mode.store.state, sort: payload });
        break;
      case 'back':
        if (browser?.contents.navigationHistory.canGoBack()) browser.contents.navigationHistory.goBack();
        break;
      case 'reload':
        if (browser && !browser.error && batch && entry) {
          record(mode, batch.id, 'manual_reload', entry.number);
          browser.contents.reload();
        }
        break;
      case 'retry':
        if (entry && batch) {
          dispose(mode, entry.id);
          mount(mode, batch, entry);
        }
        break;
      case 'copy':
      case 'copyClose': {
        const requested = typeof payload === 'string' ? mode.views.get(payload) : browser;
        const link = requested?.link;
        const session = batch?.sessions.find(item => item.id === requested?.id);
        if (!batch || !link || !session || !requested || requested.status?.ahead === undefined) break;
        mode.store.saveLink(batch.id, session, link, requested.status.ahead);
        await clipboard.writeText(link);
        if (name === 'copyClose') {
          const notice = await remove(false, session.id);
          return notice ?? 'Copied Session ' + session.number + ' link and closed it.';
        }
        return 'Copied Session ' + session.number + ' link.';
      }
      case 'close': return remove(false, typeof payload === 'string' ? payload : undefined);
      case 'sessionMenu': {
        if (typeof payload !== 'string') break;
        const target = mode.views.get(payload);
        if (!batch?.sessions.some(item => item.id === payload)) break;
        const command = await new Promise<string | undefined>(resolve => {
          let selected: string | undefined;
          const items = Menu.buildFromTemplate([
            { label: 'Copy link', enabled: Boolean(target?.link), click: () => { selected = 'copy'; } },
            { label: 'Copy and close', enabled: Boolean(target?.link), click: () => { selected = 'copyClose'; } },
            { type: 'separator' },
            { label: 'Close session', click: () => { selected = 'close'; } },
          ]);
          items.popup({ window, callback: () => resolve(selected) });
        });
        if (command) return action(command, payload);
        break;
      }
      case 'end': return remove(true);
      case 'demo':
        mode = mode.demo ? live : demo;
        restore(mode);
        if (mode.demo && !mode.store.state.batch) start(demoFixtureUrl, 12);
        window.setTitle(mode.demo ? 'Queue Launcher · Demo mode' : 'Queue Launcher');
        break;
      case 'history': return mode.store.links;
      case 'copyHistory': {
        const link = mode.store.links.find(item => item.id === payload);
        if (link) {
          await clipboard.writeText(link.url);
          return 'Copied Session ' + link.session + ' link.';
        }
        break;
      }
      case 'logs':
        mkdirSync(mode.store.logs, { recursive: true });
        return shell.openPath(mode.store.logs).then(error => error ? 'Could not open the drop logs folder.' : undefined);
      default: throw new Error('Unknown launcher action.');
    }
  }

  ipcMain.handle('launcher:action', (event, name: unknown, payload: unknown) => {
    if (!authenticate(event)) throw new Error('Invalid launcher sender.');
    const result = operations.then(() => action(name, payload));
    operations = result.then(() => {}, () => {});
    return result.then(value => ({ value }), error => ({
      error: ['open'].includes(String(name)) && error instanceof Error &&
        /^(Enter an HTTP|Enter a session count)/.test(error.message)
        ? error.message : 'Could not complete this action. Check the app data folder and try again.',
    })).finally(refresh);
  });
  ipcMain.on('launcher:bounds', (event, value: unknown) => {
    if (!authenticate(event)) return;
    if (value === null) {
      blocked = true;
    } else {
      const rect = value as Rectangle | undefined;
      if (!rect || ![rect.x, rect.y, rect.width, rect.height].every(n => Number.isInteger(n) && n >= 0)) return;
      pane = rect;
      blocked = false;
    }
    layout();
  });
  window.on('resize', layout);

  const poll = setInterval(() => {
    if (closing) return;
    // Each browser observes independently, so a stalled page cannot block the other sessions.
    for (const owner of [live, demo]) {
      for (const [id, browser] of owner.views) void capture(owner, id, browser).then(refresh);
    }
  }, 3000);

  window.on('close', event => {
    event.preventDefault();
    if (closing) return;
    closing = true;
    clearInterval(poll);
    void operations.then(async () => {
      await Promise.all([live, demo].flatMap(owner => [...owner.views.keys()].map(async id => {
        const profile = browserStore(owner.store, id);
        profile.flushStorageData();
        await profile.cookies.flushStore();
      })));
    }).then(() => shutdown(0), () => shutdown(1));
  });

  function shutdown(code: number) {
    for (const owner of [live, demo]) for (const id of owner.views.keys()) dispose(owner, id);
    launcher.webContents.close();
    closed = true;
    window.destroy();
    app.exit(code);
  }

  restore(live);
  await launcher.webContents.loadFile(launcherFile);
  window.show();
  launcher.webContents.focus();
  refresh();

  if (testMode) {
    const { verify } = await import('./verify.js');
    await verify(testMode, { launcher, live, demo });
    window.close();
  }
}

if (app.requestSingleInstanceLock()) {
  void launch().catch(error => {
    if (testMode) console.error(error?.stack);
    else if (app.isReady()) dialog.showErrorBox('Queue Launcher', 'Could not open Queue Launcher. Check its app data folder.');
    app.exit(1);
  });
} else {
  app.exit(0);
}
