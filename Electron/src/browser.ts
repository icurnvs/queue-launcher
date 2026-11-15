import { WebContentsView, session, type Session } from 'electron';
import { isWaitingRoom, observeStatus, parseStatus, resumeUrl, shareUrl, type QueueStatus } from './status.js';
import type { SavedSession, Store } from './store.js';

export const fixtureUrl = 'queue-fixture://offline/waiting-room';
export const demoFixtureUrl = fixtureUrl + '?demo=1';

function isFixture(url: unknown) {
  return url === fixtureUrl || url === demoFixtureUrl;
}
export const demoLink = 'https://queue-launcher.example/local-waiting-room.html';
export const sandbox = { sandbox: true, contextIsolation: true, nodeIntegration: false };
const readPage = (queue: boolean) => [
  '({ text: ' + (queue ? 'document.body?.innerText ?? ""' : '""') + ',',
  'error: ' + (queue ? 'document.body?.getAttribute("data-errorid") ?? ""' : '""') + ',',
  'challenge: Array.from(document.querySelectorAll("iframe")).some(frame =>',
  '/recaptcha/i.test(frame.title) && frame.getClientRects().length > 0 &&',
  'getComputedStyle(frame).visibility === "visible") })',
].join(' ');

export function allowedUrl(value: unknown, offline = false) {
  if (typeof value !== 'string') return undefined;
  const address = value.trim();
  if (isFixture(address)) return address;
  if (offline) return undefined;
  try {
    const url = new URL(address);
    if ((url.protocol === 'http:' || url.protocol === 'https:') && !url.username && !url.password) return url.href;
  } catch {
    return undefined;
  }
}

type Hooks = {
  refresh: () => void;
  log: (event: string, code?: number, ahead?: number) => void;
};

export function browserStore(store: Store, id: string): Session {
  return session.fromPath(store.profilePath(id));
}

export class BrowserSession {
  readonly view: WebContentsView;
  status?: QueueStatus;
  error?: string;
  loading = true;
  private version = 0;
  private lastObservation?: string;
  private observing = false;
  private readonly offline: boolean;

  constructor(
    readonly id: string, readonly startUrl: string, store: Store, fixture: string,
    private readonly hooks: Hooks,
  ) {
    this.offline = isFixture(startUrl);
    const profile = browserStore(store, id);
    if (this.offline && !profile.protocol.isProtocolHandled('queue-fixture')) {
      profile.protocol.handle('queue-fixture', request => request.url === startUrl
        ? new Response(fixture, { headers: {
          'Content-Type': 'text/html; charset=utf-8',
          'Content-Security-Policy': "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; base-uri 'none'",
        } }) : new Response(null, { status: 404 }));
    }
    profile.setPermissionCheckHandler(() => false);
    profile.setPermissionRequestHandler((_contents, _permission, callback) => callback(false));
    if (!profile.listenerCount('will-download')) profile.on('will-download', event => event.preventDefault());
    // Restrict all offline traffic. Live pages can use normal web resources such as CAPTCHA frames.
    if (this.offline) profile.webRequest.onBeforeRequest((details, callback) =>
      callback({ cancel: details.url !== startUrl }));
    this.view = new WebContentsView({ webPreferences: { ...sandbox, session: profile } });
    this.view.setVisible(false);
    const contents = this.view.webContents;
    const allowedNavigation = (value: string) => this.offline
      ? (value === startUrl ? value : undefined)
      : (isFixture(value) ? undefined : allowedUrl(value));
    contents.on('will-frame-navigate', event => {
      const emptyFrame = !this.offline && !event.isMainFrame &&
        (event.url === 'about:blank' || event.url === 'about:srcdoc');
      if (!emptyFrame && !allowedNavigation(event.url)) event.preventDefault();
    });
    contents.on('will-redirect', event => {
      if (!allowedNavigation(event.url)) event.preventDefault();
    });
    contents.setWindowOpenHandler(({ url }) => {
      const target = allowedNavigation(url);
      if (target) setImmediate(() => {
        if (!contents.isDestroyed()) this.navigate(target);
      });
      return { action: 'deny' };
    });
    contents.on('did-start-navigation', ({ isMainFrame, isSameDocument }) => {
      if (!isMainFrame) return;
      this.version++;
      if (isSameDocument) return;
      this.loading = true;
      this.error = undefined;
      this.status = undefined;
      this.lastObservation = undefined;
      this.hooks.refresh();
    });
    contents.on('did-navigate-in-page', (_event, url, isMainFrame) => {
      if (!isMainFrame) return;
      const previous = this.lastAddress?.split('#', 1)[0];
      if (previous !== url.split('#', 1)[0]) {
        this.status = undefined;
        this.lastObservation = undefined;
        hooks.refresh();
      }
      this.lastAddress = url;
    });
    contents.on('did-navigate', (_event, url) => {
      this.lastAddress = url;
      if (isWaitingRoom(new URL(url))) hooks.log('waiting_room_page_loaded');
      else if (this.lastWaiting) hooks.log('left_waiting_room');
      this.lastWaiting = isWaitingRoom(new URL(url));
    });
    contents.on('did-stop-loading', () => {
      this.loading = false;
      hooks.refresh();
    });
    contents.on('did-fail-load', (_event, code, _description, _url, isMainFrame) => {
      if (isMainFrame && code !== -3) this.fail('Page could not load (' + code + ').', 'url_load_failed', code);
    });
    contents.on('render-process-gone', () => {
      this.fail('This session’s web process closed.', 'web_process_terminated');
    });
  }

  private lastWaiting = false;
  private lastAddress?: string;

  get contents() { return this.view.webContents; }

  private fail(message: string, event: string, code?: number) {
    this.version++;
    this.loading = false;
    this.error = message;
    this.status = undefined;
    this.hooks.log(event, code);
    this.hooks.refresh();
  }

  navigate(url: string) {
    void this.contents.loadURL(url).catch(() => {
      // did-fail-load reports actionable errors. A superseded navigation can reject without an error page.
    });
  }

  open(entry: SavedSession) {
    const target = entry.resume && URL.canParse(entry.resume) && isWaitingRoom(new URL(entry.resume))
      ? entry.resume : this.startUrl;
    this.navigate(this.offline ? this.startUrl : target);
  }

  get link() {
    if (!this.status || this.status.ahead === undefined || this.contents.isDestroyed()) return undefined;
    const url = this.contents.getURL();
    if (!URL.canParse(url)) return undefined;
    if (isFixture(url) && this.status.queueId) return demoLink + '?q=' + this.status.queueId;
    return shareUrl(this.status, new URL(url));
  }

  async observe() {
    if (this.observing || this.loading || this.error || this.contents.isDestroyed()) return undefined;
    const address = this.contents.getURL();
    if (!URL.canParse(address)) return undefined;
    const url = new URL(address);
    const queue = isFixture(address) || isWaitingRoom(url);
    const version = this.version;
    this.observing = true;
    try {
      const page = await this.contents.executeJavaScriptInIsolatedWorld(1001, [{ code: readPage(queue) }]) as {
        text: string; error: string; challenge: boolean;
      };
      if (version !== this.version || this.contents.isDestroyed()) return undefined;
      const status = parseStatus(queue ? page.text : '', queue ? page.error : '');
      this.status = queue ? status : undefined;
      const observation = observeStatus(status, page.challenge);
      const key = observation ? JSON.stringify(observation) : undefined;
      if (observation && key !== this.lastObservation) {
        this.hooks.log(observation.event, observation.code, observation.membersAhead);
      }
      this.lastObservation = key;
      return { resume: resumeUrl(status, url), link: this.link, ahead: status.ahead };
    } catch {
      // A navigation or renderer shutdown can interrupt an observation.
      return undefined;
    } finally {
      this.observing = false;
    }
  }
}
