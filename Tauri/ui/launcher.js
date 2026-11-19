const byId = id => document.getElementById(id);
const session = byId('session');
const feedback = byId('feedback');
let busy = false;
let status;

function render(next) {
  status = next;
  session.replaceChildren(...Array.from({ length: next.count }, (_, index) =>
    new Option(String(index + 1), String(index))));
  if (next.selected !== null) session.value = String(next.selected);
  feedback.textContent = next.cleanupError ?? [
    next.count + ' open',
    next.openMs === null ? '' : 'Created in ' + next.openMs + ' ms',
    next.pendingRemoval ? next.pendingRemoval + ' store(s) pending removal' : ''
  ].filter(Boolean).join(' | ');
  byId('links').textContent = next.links.length ? 'Saved link: ' + next.links[0] : '';
  byId('count').max = 50 - next.count;
}

function updateControls() {
  session.disabled = busy || !status?.count;
  byId('close').disabled = busy || !status?.count;
  byId('save').disabled = busy;
  byId('add').querySelector('button').disabled = busy || status?.count === 50;
}

async function act(kind, fields = {}) {
  if (busy) return;
  busy = true;
  updateControls();
  try {
    render(await window.__TAURI_INTERNALS__.invoke('launcher', { action: { kind, ...fields } }));
  } catch (error) {
    feedback.textContent = String(error);
  } finally {
    busy = false;
    updateControls();
  }
}

byId('add').onsubmit = event => {
  event.preventDefault();
  act('add', { count: byId('count').valueAsNumber });
};
session.onchange = () => act('select', { index: Number(session.value) });
byId('close').onclick = () => act('close', { index: Number(session.value) });
byId('save').onclick = () => act('save_link');
act('status');
setInterval(() => { if (status?.pendingRemoval) act('status'); }, 1000);
