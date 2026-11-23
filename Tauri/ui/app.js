"use strict";

const controls = Object.fromEntries(
  Array.from(document.querySelectorAll("[id]"), item => [item.id, item])
);
const rows = new Map();
const historySort = { key: "seen", direction: "descending" };
const drafts = { live: { url: "", count: "5" }, demo: { url: "", count: "12" } };

let state;
let busyAction;
let modal;
let localNotice = "";
let historyFeedback = "";
let historySignature;
let queue = Promise.resolve();
let layoutSignature;
let layoutRequest = Promise.resolve();
let layoutScheduled = false;

function invoke(kind, values = {}) {
  const request = queue.then(() => window.__TAURI_INTERNALS__.invoke("port", {
    action: { kind, ...values }
  }));
  // A failed operation must not block later commands.
  queue = request.catch(() => {});
  return request;
}

function activeSession() {
  return state?.sessions.find(session => session.number === state.selected);
}

function browserVisible(session) {
  return Boolean(session && session.browserReady && !session.error && !session.opening);
}

function syncLayout() {
  const bounds = controls["browser-slot"].getBoundingClientRect();
  const x = Math.max(0, Math.ceil(bounds.left));
  const y = Math.max(0, Math.ceil(bounds.top));
  const width = Math.max(0, Math.floor(bounds.right) - x);
  const height = Math.max(0, Math.floor(bounds.bottom) - y);
  const visible = Boolean(width && height && !modal && browserVisible(activeSession()));
  const signature = [x, y, width, height, visible].join(",");
  if (signature === layoutSignature) return layoutRequest;

  layoutSignature = signature;
  layoutRequest = invoke("layout", { x, y, width, height, visible }).catch(error => {
    if (layoutSignature === signature) layoutSignature = undefined;
    throw error;
  });
  return layoutRequest;
}

function scheduleLayout() {
  if (layoutScheduled || !state) return;
  layoutScheduled = true;
  requestAnimationFrame(() => {
    layoutScheduled = false;
    // A later snapshot or resize retries transient failures.
    syncLayout().catch(() => {});
  });
}

function setText(target, value) {
  if (target.textContent !== value) target.textContent = value;
}

function showText(target, value) {
  setText(target, value || "");
  target.hidden = !value;
}

function modeDraft(demo) {
  return drafts[demo ? "demo" : "live"];
}

function render(next) {
  const previous = state;
  const previousSelection = previous?.selected;
  if (previous && previous.demo !== next.demo) {
    const draft = modeDraft(previous.demo);
    draft.url = controls["release-url"].value;
    draft.count = controls.count.value;
  }
  if (!previous || previous.demo !== next.demo) {
    const draft = modeDraft(next.demo);
    controls["release-url"].value = next.url || draft.url;
    controls.count.value = draft.count;
    historySignature = undefined;
  } else if (next.url && next.url !== previous.url) {
    controls["release-url"].value = next.url;
  }

  state = next;
  if (previous?.demo !== state.demo) {
    document.title = state.demo ? "Queue Launcher - Demo mode" : "Queue Launcher";
  }
  const active = activeSession();
  renderSessionRows(previousSelection !== state.selected || previous?.demo !== state.demo);
  renderControls(active);
  if (modal === controls["history-dialog"]) renderHistory();
  scheduleLayout();
}

function validDestination(text) {
  try {
    const url = new URL(text.trim());
    return ["http:", "https:", "file:"].includes(url.protocol);
  } catch {
    return false;
  }
}

function renderControls(active = activeSession()) {
  const busy = Boolean(busyAction);
  const hasSessions = Boolean(state?.sessions.length);
  const ready = state?.sessions.filter(session => session.ready).length ?? 0;
  const demo = Boolean(state?.demo);

  controls["demo-indicator"].hidden = !demo;
  setText(controls.demo, demo ? "Exit demo mode" : "Demo mode");
  controls.demo.disabled = !state || busy;
  setText(controls.add, busyAction === "add" ? "Adding session..." : "Add session");
  setText(controls.open, busyAction === "open" ? "Opening sessions..." : "Open sessions");
  controls.add.hidden = controls.end.hidden = !hasSessions;
  controls.add.disabled = busy || !hasSessions || state.sessions.length >= 50;
  controls.end.disabled = busy || !hasSessions;
  controls.history.disabled = controls.logs.disabled = controls.settings.disabled = !state || busy;
  controls.setup.hidden = hasSessions;
  controls["release-url"].disabled = busy || demo || !state;
  controls.count.disabled = busy || !state;
  controls.open.disabled = busy || !state || (!demo && !validDestination(controls["release-url"].value));
  controls.sort.disabled = !state || busy;
  controls.sort.checked = Boolean(state?.sort);
  controls["empty-sessions"].hidden = hasSessions;
  controls["browser-controls"].hidden = controls["session-actions"].hidden = !hasSessions;
  controls["browser-navigation"].hidden = !hasSessions;
  const currentUrl = state?.currentUrl || "";
  if (controls["current-url"].value !== currentUrl) controls["current-url"].value = currentUrl;
  controls["current-url"].title = currentUrl;
  controls["current-url"].disabled = !active;
  setText(controls["ready-count"], hasSessions ? ready + " of " + state.sessions.length + " ready" : "");
  setText(controls["browser-heading"], active ? "Session " + active.number : "Browser");
  controls.mark.disabled = controls.close.disabled = busy || !active;
  controls.copy.disabled = controls["copy-close"].disabled = busy || !active?.shareable;
  controls.back.disabled = busy || !active?.canGoBack || !browserVisible(active);
  controls.forward.disabled = busy || !active?.canGoForward || !browserVisible(active);
  controls.reload.disabled = busy || !browserVisible(active);
  const loading = Boolean(active?.loading);
  const reloadLabel = loading ? "Stop loading" : "Reload page";
  controls.reload.dataset.loading = String(loading);
  controls.reload.setAttribute("aria-label", reloadLabel);
  controls.reload.title = reloadLabel;
  setText(controls.mark, active?.ready ? "Mark pending" : "Mark ready");
  controls["copy-close"].setAttribute("aria-label",
    active ? "Copy link and close Session " + active.number : "Copy and close");
  controls.copy.setAttribute("aria-label", active ? "Copy link for Session " + active.number : "Copy link");

  for (const row of rows.values()) {
    row.card.disabled = busy;
    const session = state.sessions.find(session => session.number === Number(row.card.dataset.session));
    row.copy.disabled = row.copyClose.disabled = busy || !session?.shareable;
  }

  controls.placeholder.hidden = browserVisible(active);
  controls.retry.disabled = busy;
  controls.retry.hidden = !active?.error;

  if (active?.error) {
    setText(controls["placeholder-title"], "Page couldn't load");
    showText(controls["placeholder-detail"], active.error);
  } else if (active || ["open", "retry", "toggle_demo"].includes(busyAction)) {
    setText(controls["placeholder-title"], "Opening sessions...");
    showText(controls["placeholder-detail"], "Preparing the browser.");
  } else if (state) {
    setText(controls["placeholder-title"], "No sessions open");
    showText(controls["placeholder-detail"], demo
      ? "Open sessions to start another demo batch."
      : "Enter a release URL above to start a batch.");
  } else {
    setText(controls["placeholder-title"], "Starting Queue Launcher...");
    showText(controls["placeholder-detail"], "");
  }
  showText(controls.notice, localNotice || state?.notice);
  showText(controls.feedback, state?.feedback);
}

function makeText(tag, className, value) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  setText(node, value);
  return node;
}

function createRow(number) {
  const item = document.createElement("li");
  item.className = "session-row";
  const card = document.createElement("button");
  card.className = "session-card";
  card.dataset.session = number;
  card.type = "button";
  const copy = makeText("button", "row-copy", "Copy");
  copy.type = "button";
  copy.setAttribute("aria-label", "Copy queue link for Session " + number);
  copy.addEventListener("click", () => perform("copy", { number }));

  card.addEventListener("click", () => perform("select", { number }));
  item.addEventListener("click", event => {
    if (!event.target.closest("button")) perform("select", { number });
  });
  item.addEventListener("contextmenu", event => {
    event.preventDefault();
    showSessionActions(number, event.clientX, event.clientY);
  });
  card.addEventListener("keydown", event => {
    if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
      event.preventDefault();
      const bounds = card.getBoundingClientRect();
      showSessionActions(number, bounds.left + 12, bounds.bottom);
      return;
    }
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const sessions = state.sessions;
    const current = sessions.findIndex(session => session.number === number);
    const next = event.key === "Home" ? 0 : event.key === "End" ? sessions.length - 1
      : Math.max(0, Math.min(sessions.length - 1, current + (event.key === "ArrowDown" ? 1 : -1)));
    const target = sessions[next].number;
    rows.get(target).card.focus();
    if (target !== number) perform("select", { number: target });
  });

  const copyClose = makeText("button", "row-copy-close", "Copy and close");
  copyClose.type = "button";
  copyClose.setAttribute("aria-label", "Copy link and close Session " + number);
  copyClose.addEventListener("click", () => perform("copy_close", { number }));
  const actions = makeText("div", "row-actions", "");
  actions.append(copy, copyClose);
  item.append(card, actions);
  return { item, card, copy, copyClose };
}

function renderCard(row, session, best) {
  const status = session.status;
  const isBest = status?.ahead != null ? status.ahead === best.ahead
    : best.ahead === Infinity && status?.estimatedWaitSeconds != null && status.estimatedWaitSeconds === best.wait;
  const signature = JSON.stringify([session.ready, session.error, status, isBest, session.shareable]);
  if (signature === row.signature) return;
  row.signature = signature;

  const top = makeText("span", "session-top", "");
  top.append(
    makeText("span", "session-title", "Session " + session.number),
    makeText("span", session.ready ? "session-state ready" : "session-state", session.ready ? "Ready" : "Pending")
  );
  const details = [top];
  if (status?.message) details.push(makeText("span", "session-detail error", status.message));
  if (status?.errorId != null) details.push(makeText("span", "session-detail error", "Queue-it error " + status.errorId));
  if (status?.ahead != null) {
    const position = makeText("span", "queue-position", "");
    position.append(makeText("span", "", status.ahead.toLocaleString() + " ahead"));
    if (isBest) position.append(makeText("span", "best-spot", "Best spot"));
    details.push(position);
  } else if (status?.inWaitingRoom && !status.message) {
    details.push(makeText("span", "session-detail", "In the waiting room"));
  }
  if (status?.estimatedWait) {
    const estimate = makeText("span", "session-detail", "Estimated wait " + status.estimatedWait);
    if (isBest && status.ahead == null) estimate.append(document.createTextNode(" "), makeText("span", "best-spot", "Shortest wait"));
    details.push(estimate);
  }
  if (status?.timeRemaining) details.push(makeText("span", "session-detail", status.timeRemaining + " left"));
  else if (status?.purchaseWindow) details.push(makeText("span", "session-detail", status.purchaseWindow + " once admitted"));
  if (session.error && !status?.message) details.push(makeText("span", "session-detail error", "Browser unavailable"));
  row.copy.disabled = row.copyClose.disabled = !session.shareable;
  row.card.replaceChildren(...details);
}

function renderSessionRows(revealSelection) {
  const sessions = state.sessions;
  const focused = document.activeElement;
  const kept = new Set(sessions.map(session => session.number));
  for (const [number, row] of rows) {
    if (!kept.has(number)) {
      row.item.remove();
      rows.delete(number);
    }
  }

  const best = {
    ahead: Math.min(...sessions.map(session => session.status?.ahead ?? Infinity)),
    wait: Math.min(...sessions.map(session => session.status?.estimatedWaitSeconds ?? Infinity))
  };
  const list = controls["session-list"];
  let firstUnordered = list.firstElementChild;
  for (const session of sessions) {
    let row = rows.get(session.number);
    if (!row) {
      row = createRow(session.number);
      rows.set(session.number, row);
    }
    if (row.item === firstUnordered) firstUnordered = firstUnordered.nextElementSibling;
    else list.insertBefore(row.item, firstUnordered);
    const selected = session.number === state.selected;
    row.item.classList.toggle("selected", selected);
    row.card.setAttribute("aria-current", selected ? "true" : "false");
    row.card.tabIndex = selected || (state.selected == null && session === sessions[0]) ? 0 : -1;
    row.copy.tabIndex = row.copyClose.tabIndex = 0;
    renderCard(row, session, best);
  }

  if (focused?.isConnected && focused !== document.activeElement && list.contains(focused)) {
    focused.focus({ preventScroll: true });
  }
  if (revealSelection) rows.get(state.selected)?.item.scrollIntoView({ block: "nearest" });
}

const dateFormatter = new Intl.DateTimeFormat(undefined, { dateStyle: "short", timeStyle: "short" });

function displayDate(timestamp) {
  const date = new Date(timestamp);
  return Number.isNaN(date.getTime()) ? "" : dateFormatter.format(date);
}

function renderHistory() {
  const history = state.history;
  const signature = JSON.stringify([history, historySort]);
  setText(controls["history-feedback"], historyFeedback);
  if (signature === historySignature) return;
  historySignature = signature;

  const hasLinks = history.length > 0;
  controls["history-scroll"].hidden = !hasLinks;
  setText(controls["history-description"], hasLinks
    ? "Saved queue links may expire at the waiting room."
    : "No queue links saved yet.");
  const batchDates = new Map();
  for (const item of history) {
    const seen = Date.parse(item.seen);
    const first = batchDates.get(item.batch);
    if (first === undefined || seen < first) batchDates.set(item.batch, seen);
  }

  const items = history.map((item, index) => ({
    ...item, index, seen: Date.parse(item.seen), batchSeen: batchDates.get(item.batch)
  }));
  const { key, direction } = historySort;
  items.sort((a, b) => {
    const left = a[key];
    const right = b[key];
    const result = key === "url" ? left.localeCompare(right) : (left ?? Infinity) - (right ?? Infinity);
    return Number.isNaN(result) ? 0 : direction === "ascending" ? result : -result;
  });

  const activeIndex = document.activeElement?.dataset.historyIndex;
  const links = [];
  for (const item of items) {
    const row = document.createElement("tr");
    row.append(
      makeText("td", "", displayDate(item.batchSeen)),
      makeText("td", "", String(item.session)),
      makeText("td", "", item.ahead == null ? "" : item.ahead.toLocaleString()),
      makeText("td", "", displayDate(item.seen)),
      makeText("td", "history-link", item.url)
    );
    const cell = document.createElement("td");
    const copy = makeText("button", "", "Copy link");
    copy.type = "button";
    copy.dataset.historyIndex = String(item.index);
    copy.setAttribute("aria-label", "Copy saved link for Session " + item.session);
    copy.addEventListener("click", async () => {
      const ok = await perform("copy_history", { index: item.index });
      historyFeedback = ok ? "Copied Session " + item.session + " link." : localNotice;
      setText(controls["history-feedback"], historyFeedback);
    });
    cell.append(copy);
    row.append(cell);
    links.push(row);
  }

  controls["history-rows"].replaceChildren(...links);
  for (const button of controls["history-table"].querySelectorAll("[data-sort-key]")) {
    if (button.dataset.sortKey === key) button.parentElement.setAttribute("aria-sort", direction);
    else button.parentElement.removeAttribute("aria-sort");
  }
  if (activeIndex !== undefined) {
    controls["history-rows"].querySelector("[data-history-index='" + activeIndex + "']")
      ?.focus({ preventScroll: true });
  }
}

function errorText(error) {
  return error instanceof Error ? error.message : String(error);
}

async function perform(kind, values) {
  if (busyAction) return false;
  const origin = document.activeElement;
  busyAction = kind;
  localNotice = "";
  renderControls();
  try {
    render(await invoke(kind, values));
    return true;
  } catch (error) {
    localNotice = errorText(error);
    await refresh();
    return false;
  } finally {
    busyAction = undefined;
    renderControls();
    scheduleLayout();
    if (document.activeElement === document.body) {
      const selected = rows.get(state?.selected)?.card;
      const canReturn = origin?.isConnected && !origin.disabled && !origin.closest("[hidden]");
      const fallback = state?.demo ? controls.count : controls["release-url"];
      const target = kind === "open" && selected ? selected
        : canReturn ? origin : selected || fallback;
      target?.focus({ preventScroll: true });
    }
  }
}

async function refresh() {
  try {
    render(await invoke("status"));
  } catch (error) {
    if (!state && !localNotice) localNotice = errorText(error);
    renderControls();
  }
}

async function showDialog(dialog) {
  if (modal || busyAction) return;
  modal = dialog;
  try {
    await syncLayout();
    dialog.returnValue = "";
    dialog.showModal();
    dialog.querySelector("[data-initial-focus]")?.focus();
  } catch (error) {
    modal = undefined;
    localNotice = errorText(error);
    renderControls();
    scheduleLayout();
  }
}

function closeDialog(dialog) {
  if (modal === dialog) modal = undefined;
  scheduleLayout();
}

function dismissSettings() {
  if (modal !== controls["settings-dialog"]) return;
  controls["settings-dialog"].close();
  closeDialog(controls["settings-dialog"]);
}

async function showSessionActions(number, x, y) {
  if (modal || busyAction) return;
  try {
    await window.__TAURI_INTERNALS__.invoke("session_menu", { number, x, y });
  } catch (error) {
    localNotice = errorText(error);
    renderControls();
  }
}

controls["release-url"].addEventListener("input", () => renderControls());
controls.setup.addEventListener("submit", event => {
  event.preventDefault();
  perform("open", { url: controls["release-url"].value.trim(), count: controls.count.valueAsNumber });
});
controls.sort.addEventListener("change", event => perform("sort", { enabled: event.target.checked }));
controls.demo.addEventListener("click", () => { dismissSettings(); perform("toggle_demo"); });
controls.add.addEventListener("click", () => perform("add"));
controls.logs.addEventListener("click", () => { dismissSettings(); perform("open_logs"); });
controls.reload.addEventListener("click", () => perform(activeSession()?.loading ? "stop" : "reload"));
for (const kind of ["back", "forward", "mark", "retry"]) {
  controls[kind].addEventListener("click", () => perform(kind));
}
for (const kind of ["copy", "copy_close", "close"]) {
  controls[kind.replaceAll("_", "-")].addEventListener("click", () => {
    const active = activeSession();
    if (active) perform(kind, { number: active.number });
  });
}

controls.end.addEventListener("click", () => showDialog(controls["end-dialog"]));
controls["end-dialog"].addEventListener("close", event => {
  closeDialog(event.target);
  if (event.target.returnValue === "end") perform("end");
});

controls.history.addEventListener("click", () => {
  dismissSettings();
  historyFeedback = "";
  renderHistory();
  showDialog(controls["history-dialog"]);
});
controls["history-dialog"].addEventListener("close", event => closeDialog(event.target));
controls["history-table"].addEventListener("click", event => {
  const key = event.target.closest("button[data-sort-key]")?.dataset.sortKey;
  if (!key) return;
  historySort.direction = key === historySort.key && historySort.direction === "ascending"
    ? "descending" : "ascending";
  historySort.key = key;
  renderHistory();
});

const layoutObserver = new ResizeObserver(scheduleLayout);
for (const element of [controls["browser-slot"], document.querySelector(".browser-header"),
  document.querySelector(".masthead")]) {
  layoutObserver.observe(element);
}
window.addEventListener("resize", scheduleLayout);
renderControls();
(async function poll() {
  if (!busyAction) await refresh();
  setTimeout(poll, 1000);
})();

controls.settings.addEventListener("click", () => showDialog(controls["settings-dialog"]));
controls["settings-close"].addEventListener("click", () => controls["settings-dialog"].close());
controls["settings-dialog"].addEventListener("close", event => closeDialog(event.target));
