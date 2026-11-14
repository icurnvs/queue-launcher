import { app, BaseWindow, ipcMain, Menu, session, WebContentsView } from 'electron';
import { randomUUID } from 'node:crypto';
import { mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { verify } from './checks.js';

const assets = join(__dirname, '..');
const sidebarWidth = 250;
const limit = 50;
const mode = process.env.QUEUE_PROTOTYPE_VERIFY;
const port = Number(process.env.QUEUE_PROTOTYPE_PORT ?? 38147);
const origin = 'http://127.0.0.1:' + port;
const fixtureUrl = origin + '/waiting-room';
const sampleLink = 'https://queue-launcher.example/waiting-room';
const launcherFile = join(assets, 'launcher.html');
const launcherUrl = pathToFileURL(launcherFile).href;
const scratch = process.env.QUEUE_PROTOTYPE_DATA;
if (scratch) {
  mkdirSync(scratch, { recursive: true });
  app.setPath('userData', scratch);
}
app.setName('Queue Launcher Electron Prototype');

type Entry = { id: string; label: string };
type Manifest = { entries: Entry[]; savedLink?: string; clearing?: string };
const manifestPath = join(app.getPath('userData'), 'launcher.json');

function readManifest(): Manifest {
  try {
    return JSON.parse(readFileSync(manifestPath, 'utf8'));
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
    return { entries: [] };
  }
}

function saveManifest(manifest: Manifest) {
  writeFileSync(manifestPath + '.tmp', JSON.stringify(manifest, null, 2) + '\n');
  renameSync(manifestPath + '.tmp', manifestPath);
}

function queueSession(id: string) {
  return session.fromPartition('persist:' + id);
}

async function clearSession(id: string) {
  const store = queueSession(id);
  await store.clearData();
  await store.cookies.flushStore();
  store.flushStorageData();
}

const fixture = readFileSync(join(assets, 'fixture.html'));
const server = createServer((request, response) => {
  response.setHeader('Content-Type', 'text/html; charset=utf-8');
  response.setHeader('Cache-Control', 'no-store');
  response.setHeader('Content-Security-Policy',
    "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; base-uri 'none'");
  switch (request.url) {
    case '/waiting-room': response.end(fixture); break;
    case '/empty': response.end('<!doctype html><title>Empty fixture</title>'); break;
    default: response.writeHead(404).end();
  }
});

async function run() {
  await app.whenReady();
  await new Promise<void>((resolve, reject) => {
    server.once('error', reject);
    server.listen(port, '127.0.0.1', resolve);
  });

  let manifest = readManifest();
  if (manifest.clearing) {
    await clearSession(manifest.clearing);
    manifest = { entries: manifest.entries, savedLink: manifest.savedLink };
    saveManifest(manifest);
  }

  Menu.setApplicationMenu(null);
  const window = new BaseWindow({
    title: 'Queue Launcher Electron Prototype',
    width: 1040, height: 670, minWidth: 640, minHeight: 440, show: false,
  });
  const sandbox = { sandbox: true, contextIsolation: true, nodeIntegration: false };
  const launcher = new WebContentsView({
    webPreferences: { ...sandbox, preload: join(assets, 'preload.cjs') },
  });
  launcher.webContents.on('will-navigate', event => event.preventDefault());
  launcher.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  window.contentView.addChildView(launcher);

  const views = new Map<string, WebContentsView>();
  let active: string | undefined;
  let shuttingDown = false;
  let operation = Promise.resolve();

  function layout() {
    const { width, height } = window.getContentBounds();
    launcher.setBounds({ x: 0, y: 0, width: sidebarWidth, height });
    for (const view of views.values()) {
      view.setBounds({ x: sidebarWidth, y: 0, width: width - sidebarWidth, height });
    }
  }

  function select(id?: string) {
    if (active) views.get(active)?.setVisible(false);
    active = id;
    if (id) views.get(id)?.setVisible(true);
  }

  async function open(entry: Entry) {
    const store = queueSession(entry.id);
    store.setPermissionCheckHandler(() => false);
    store.setPermissionRequestHandler((_contents, _permission, callback) => callback(false));
    store.webRequest.onBeforeRequest((details, callback) => {
      callback({ cancel: details.url !== fixtureUrl && details.url !== origin + '/empty' });
    });
    const view = new WebContentsView({ webPreferences: { ...sandbox, session: store } });
    view.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
    view.setVisible(false);
    window.contentView.addChildView(view);
    views.set(entry.id, view);
    const { width, height } = window.getContentBounds();
    view.setBounds({ x: sidebarWidth, y: 0, width: width - sidebarWidth, height });
    if (!active) select(entry.id);
    try {
      await view.webContents.loadURL(fixtureUrl);
    } catch (error) {
      dispose(entry.id);
      if (active === entry.id) select(views.keys().next().value);
      throw error;
    }
  }

  function dispose(id: string) {
    const view = views.get(id);
    if (!view) return;
    window.contentView.removeChildView(view);
    view.webContents.close();
    views.delete(id);
  }

  async function finishClear() {
    if (!manifest.clearing) return;
    await clearSession(manifest.clearing);
    const next = { entries: manifest.entries, savedLink: manifest.savedLink };
    saveManifest(next);
    manifest = next;
  }

  function update(next: Manifest) {
    saveManifest(next);
    manifest = next;
  }

  async function handle(action: unknown, id: unknown) {
    switch (action) {
      case 'state': return { ...manifest, active, limit };
      case 'add': {
        if (manifest.entries.length >= limit) return;
        const number = Math.max(0, ...manifest.entries.map(entry => Number(entry.label.slice(8)))) + 1;
        const entry = { id: randomUUID(), label: 'Session ' + number };
        await open(entry);
        try {
          update({ ...manifest, entries: [...manifest.entries, entry] });
        } catch (error) {
          dispose(entry.id);
          if (active === entry.id) select();
          await clearSession(entry.id);
          throw error;
        }
        select(entry.id);
        return;
      }
      case 'select':
        if (typeof id === 'string' && views.has(id)) select(id);
        return;
      case 'saveLink':
        update({ ...manifest, savedLink: sampleLink });
        return;
      case 'close': {
        await finishClear();
        const idToClose = active;
        if (!idToClose) return;
        const entries = manifest.entries.filter(entry => entry.id !== idToClose);
        update({ ...manifest, entries, clearing: idToClose });
        dispose(idToClose);
        select(entries[0]?.id);
        await finishClear();
        return;
      }
      default: throw new Error('Unknown launcher action');
    }
  }

  ipcMain.handle('launcher', (event, action: unknown, id: unknown) => {
    if (shuttingDown || event.sender !== launcher.webContents ||
        event.senderFrame !== launcher.webContents.mainFrame || event.senderFrame.url !== launcherUrl) {
      throw new Error('Invalid launcher sender');
    }
    const result = operation.then(() => handle(action, id));
    operation = result.then(() => {}, () => {});
    return result;
  });

  window.on('resize', layout);
  window.on('close', event => {
    if (shuttingDown) {
      event.preventDefault();
      return;
    }
    event.preventDefault();
    shuttingDown = true;
    void operation.then(async () => {
      await Promise.all([...views.keys()].map(async id => {
        const store = queueSession(id);
        store.flushStorageData();
        await store.cookies.flushStore();
      }));
    }).then(() => finishShutdown(0), () => finishShutdown(1));
  });

  function finishShutdown(code: number) {
    for (const id of views.keys()) dispose(id);
    launcher.webContents.close();
    window.destroy();
    app.exit(code);
  }

  // Show the shell after the saved views are loaded so controls cannot race startup.
  layout();
  const shell = launcher.webContents.loadFile(launcherFile);
  await Promise.all([shell, ...manifest.entries.map(open)]);
  window.show();

  if (mode) {
    await verify(mode, {
      launcher, views, origin, queueSession, windowShownMs: Math.round(process.uptime() * 1000),
    });
    window.close();
  }
}

if (app.requestSingleInstanceLock()) {
  void run().catch(error => {
    console.error(mode ? error : 'Prototype failed to start. Check whether its local port is available.');
    app.exit(1);
  });
} else {
  app.exit(1);
}
