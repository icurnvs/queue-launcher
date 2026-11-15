const byId = id => document.getElementById(id);
const page = byId('page');
const endDialog = byId('end-dialog');
const historyDialog = byId('history-dialog');
const number = new Intl.NumberFormat();
const date = new Intl.DateTimeFormat(undefined, { dateStyle: 'short', timeStyle: 'short' });
const formValues = [{ url: '', count: '5' }, { url: 'queue-fixture://offline/waiting-room?demo=1', count: '12' }];
let state;
let busy = false;
let renderedRows = '';
let history = [];
let sortKey = 'seen';
let sortDirection = -1;

function setText(element, value) {
  if (element.textContent !== value) element.textContent = value;
}

function positionBrowser() {
  if (endDialog.open || historyDialog.open || !state?.active) {
    window.launcher.bounds(null);
    return;
  }
  const bounds = page.getBoundingClientRect();
  const x = Math.ceil(bounds.x);
  const y = Math.ceil(bounds.y);
  window.launcher.bounds({
    x, y, width: Math.max(0, Math.floor(bounds.right) - x), height: Math.max(0, Math.floor(bounds.bottom) - y),
  });
}

async function run(name, payload, feedback = byId('feedback')) {
  if (busy) return undefined;
  busy = true;
  renderControls();
  setText(feedback, '');
  try {
    const response = await window.launcher.action(name, payload);
    if (response.error) setText(feedback, response.error);
    else if (typeof response.value === 'string') setText(feedback, response.value);
    return response.value;
  } catch {
    setText(feedback, 'Could not reach Queue Launcher. Try again.');
  } finally {
    busy = false;
    renderControls();
  }
}

function span(text, className = '') {
  const element = document.createElement('span');
  element.textContent = text;
  element.className = className;
  return element;
}

function sessionRow(entry, best) {
  const row = document.createElement('li');
  row.className = 'session-row';
  const select = document.createElement('button');
  select.className = 'session-select';
  select.dataset.focus = entry.id;
  select.setAttribute('aria-pressed', String(entry.id === state.active));
  const heading = span('Session ' + entry.number, 'session-heading');
  heading.append(span(entry.ready ? 'Ready' : 'Pending', entry.ready ? 'session-state is-ready' : 'session-state'));
  select.append(heading);
  if (entry.ahead !== undefined) {
    const position = span(number.format(entry.ahead) + ' ahead', 'position');
    if (entry.ahead === best) position.append(span('Best spot', 'best'));
    select.append(position);
  } else if (entry.waiting) {
    select.append(span('In the waiting room', 'details'));
  }
  if (entry.wait) select.append(span('Est. wait ' + entry.wait, 'details'));
  if (entry.purchaseWindow) select.append(span('Purchase window ' + entry.purchaseWindow, 'details'));
  if (entry.remaining) select.append(span('Time remaining ' + entry.remaining, 'details'));
  if (entry.message || entry.error) select.append(span(entry.message || entry.error, 'details is-error'));
  if (entry.code !== undefined) select.append(span('Queue-it error ' + entry.code, 'details is-error'));
  select.onclick = () => run('select', entry.id);
  row.oncontextmenu = event => {
    event.preventDefault();
    run('sessionMenu', entry.id);
  };
  row.append(select);
  if (entry.shareable) {
    const copy = document.createElement('button');
    copy.textContent = 'Copy';
    copy.className = 'row-copy';
    copy.dataset.focus = entry.id + '-copy';
    copy.setAttribute('aria-label', 'Copy link for Session ' + entry.number);
    copy.onclick = () => run('copy', entry.id);
    row.append(copy);
  }
  return row;
}

function renderControls() {
  const selection = state?.sessions.find(entry => entry.id === state.active);
  for (const id of ['demo', 'history', 'logs', 'open', 'end', 'fixture', 'sort']) byId(id).disabled = !state || busy;
  byId('add').disabled = busy || (state?.sessions.length ?? 0) >= (state?.limit ?? 0);
  byId('next').disabled = busy || (state?.sessions.length ?? 0) < 2;
  byId('back').disabled = busy || !selection?.canGoBack;
  byId('reload').disabled = busy || !selection || selection.loading || Boolean(selection.error);
  byId('mark').disabled = busy || !selection;
  byId('close').disabled = busy || !selection;
  byId('retry').disabled = busy;
  byId('copy').disabled = byId('copyClose').disabled = busy || !selection?.shareable;
}

function render(next) {
  if (next.demo !== state?.demo) {
    const values = formValues[Number(next.demo)];
    byId('release-url').value = values.url;
    byId('count').value = values.count;
  }
  const priorActive = state?.active;
  state = next;
  const { sessions, demo, active } = state;
  const selection = sessions.find(entry => entry.id === active);
  byId('demo-label').hidden = !demo;
  setText(byId('demo'), demo ? 'Exit demo mode' : 'Demo mode');
  byId('release-url').readOnly = demo;
  byId('fixture').hidden = demo;
  byId('setup').hidden = Boolean(sessions.length);
  for (const id of ['end', 'add', 'session-actions', 'navigation']) byId(id).hidden = !selection;
  byId('empty-list').hidden = Boolean(sessions.length);
  byId('sort').checked = state.sort;
  setText(byId('ready'), sessions.length ? state.totalReady + ' of ' + sessions.length + ' ready' : '');
  setText(byId('selected'), selection ? 'Session ' + selection.number : 'Browser');
  setText(byId('mark'), selection?.ready ? 'Mark pending' : 'Mark ready');
  setText(byId('page-title'), selection?.error ? 'Page could not load' : selection ? 'Opening session…' : 'No browser session selected');
  setText(byId('page-message'), selection?.error ?? (selection ? 'Preparing the browser.'
    : demo ? 'Open sessions to start another demo batch.' : 'Enter a release URL or try Demo mode.'));
  byId('retry').hidden = !selection?.error;

  const signature = JSON.stringify([active, sessions]);
  if (signature !== renderedRows) {
    renderedRows = signature;
    const focus = document.activeElement?.dataset.focus;
    const best = Math.min(...sessions.map(entry => entry.ahead ?? Infinity));
    byId('sessions').replaceChildren(...sessions.map(entry => sessionRow(entry, best)));
    if (focus) byId('sessions').querySelector('[data-focus="' + focus + '"]')?.focus({ preventScroll: true });
    if (priorActive !== active) byId('sessions').querySelector('[aria-pressed="true"]')?.scrollIntoView({ block: 'nearest' });
  }
  if (state.notice && !byId('feedback').textContent) setText(byId('feedback'), state.notice);
  renderControls();
  positionBrowser();
}

function drawHistory() {
  const earliest = new Map();
  for (const link of history) {
    const date = earliest.get(link.batch);
    if (!date || date > link.seen) earliest.set(link.batch, link.seen);
  }
  const sortValue = link => sortKey === 'batch' ? earliest.get(link.batch) : link[sortKey];
  const sorted = [...history].sort((a, b) => sortDirection *
    (sortValue(a) > sortValue(b) ? 1 : sortValue(a) < sortValue(b) ? -1 : 0));
  const rows = sorted.map(link => {
    const row = document.createElement('tr');
    for (const value of [date.format(new Date(earliest.get(link.batch))), link.session,
      number.format(link.ahead), date.format(new Date(link.seen)), link.url]) {
      row.insertCell().textContent = value;
    }
    const copy = document.createElement('button');
    copy.textContent = 'Copy';
    copy.setAttribute('aria-label', 'Copy saved link for Session ' + link.session);
    copy.onclick = () => run('copyHistory', link.id, byId('history-feedback'));
    row.insertCell().append(copy);
    return row;
  });
  byId('history-rows').replaceChildren(...rows);
  for (const button of historyDialog.querySelectorAll('[data-sort]')) {
    if (button.dataset.sort === sortKey) button.parentElement.setAttribute('aria-sort', sortDirection < 0 ? 'descending' : 'ascending');
    else button.parentElement.removeAttribute('aria-sort');
  }
}

byId('setup').onsubmit = event => {
  event.preventDefault();
  run('open', { url: byId('release-url').value, count: byId('count').valueAsNumber });
};
byId('fixture').onclick = () => {
  byId('release-url').value = 'queue-fixture://offline/waiting-room';
  byId('release-url').focus();
};
byId('demo').onclick = () => {
  if (!state) return;
  formValues[Number(state.demo)] = { url: byId('release-url').value, count: byId('count').value };
  run('demo');
};
byId('sort').onchange = event => run('sort', event.target.checked);
for (const id of ['add', 'next', 'mark', 'back', 'reload', 'retry', 'copy', 'copyClose', 'close', 'logs']) {
  byId(id).onclick = () => run(id);
}
byId('end').onclick = () => {
  endDialog.returnValue = '';
  endDialog.showModal();
  positionBrowser();
};
endDialog.onclose = () => {
  positionBrowser();
  if (endDialog.returnValue === 'end') run('end');
};
byId('history').onclick = async () => {
  const entries = await run('history');
  if (!entries || historyDialog.open) return;
  history = entries;
  byId('history-table').hidden = !entries.length;
  setText(byId('history-hint'), entries.length ? 'Saved queue links may expire at the waiting room.' : 'No queue links saved yet.');
  setText(byId('history-feedback'), '');
  drawHistory();
  historyDialog.showModal();
  positionBrowser();
};
historyDialog.onclose = positionBrowser;
for (const button of historyDialog.querySelectorAll('[data-sort]')) {
  button.onclick = () => {
    sortDirection = sortKey === button.dataset.sort ? -sortDirection : 1;
    sortKey = button.dataset.sort;
    drawHistory();
  };
}

new ResizeObserver(positionBrowser).observe(page);
window.addEventListener('resize', positionBrowser);
window.launcher.onState(render);
window.launcher.action('state').then(({ value }) => { if (value && !state) render(value); });
