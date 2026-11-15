import { randomUUID } from 'node:crypto';
import { appendFileSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { uuidPattern } from './status.js';

export type SavedSession = { id: string; number: number; ready: boolean; resume?: string };
export type Batch = { id: string; url: string; nextNumber: number; selected: string; sessions: SavedSession[] };
export type SavedLink = { id: string; batch: string; session: number; url: string; seen: string; ahead: number };
type State = { batch?: Batch; pending: string[]; sort: boolean };

function read<T>(path: string, fallback: T): T {
  try {
    return JSON.parse(readFileSync(path, 'utf8'));
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return fallback;
    throw error;
  }
}

function write(path: string, value: unknown) {
  const temporary = path + '.tmp';
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(temporary, JSON.stringify(value, null, 2) + '\n');
  renameSync(temporary, path);
}

export class Store {
  state: State;
  links: SavedLink[];
  readonly logs: string;
  private readonly manifest: string;
  private readonly history: string;
  private readonly profiles: string;

  constructor(directory: string) {
    this.manifest = join(directory, 'batch.json');
    this.history = join(directory, 'queue-links.json');
    this.logs = join(directory, 'Drop Logs');
    this.profiles = join(directory, 'Profiles');
    this.state = read<State>(this.manifest, { pending: [], sort: false });
    this.links = read<SavedLink[]>(this.history, []);
  }

  profilePath(id: string) {
    if (!uuidPattern.test(id)) throw new Error('Invalid profile ID.');
    return join(this.profiles, id);
  }

  save(state: State) {
    write(this.manifest, state);
    this.state = state;
  }

  reclaimProfiles() {
    const pending = [];
    for (const id of this.state.pending) {
      if (this.state.batch?.sessions.some(entry => entry.id === id)) {
        pending.push(id);
        continue;
      }
      try {
        rmSync(this.profilePath(id), { recursive: true, force: true });
      } catch {
        pending.push(id);
      }
    }
    if (pending.length !== this.state.pending.length) this.save({ ...this.state, pending });
    return pending.length;
  }

  saveLink(batch: string, entry: SavedSession, url: string, ahead: number) {
    if (this.links.some(link => link.batch === batch && link.session === entry.number && link.url === url)) return;
    const links = [...this.links, {
      id: randomUUID(), batch, session: entry.number, url, ahead, seen: new Date().toISOString(),
    }];
    write(this.history, links);
    this.links = links;
  }

  log(batch: string, event: string, session?: number, code?: number, membersAhead?: number) {
    if (!uuidPattern.test(batch)) throw new Error('Invalid batch ID.');
    mkdirSync(this.logs, { recursive: true });
    appendFileSync(join(this.logs, batch + '.jsonl'), JSON.stringify({
      timestamp: new Date().toISOString(), event, session, code, membersAhead,
    }) + '\n');
  }
}
