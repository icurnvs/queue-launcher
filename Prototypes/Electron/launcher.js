const sessions = document.querySelector('#sessions');
const error = document.querySelector('#error');
const actionButtons = [...document.querySelectorAll('body > button')];

function render({ entries, active, savedLink, limit }) {
  document.querySelector('#count').textContent = entries.length + ' / ' + limit;
  document.querySelector('#saved').textContent = savedLink ? 'Saved link: ' + savedLink : '';
  document.querySelector('#add').disabled = entries.length >= limit;
  document.querySelector('#close').disabled = !active;
  sessions.replaceChildren(...entries.map(({ id, label }) => {
    const button = document.createElement('button');
    button.dataset.id = id;
    button.textContent = label;
    button.setAttribute('aria-pressed', String(id === active));
    button.onclick = () => update(() => window.launcher.select(id));
    return button;
  }));
}

let busy = false;
async function update(action) {
  if (busy) return;
  busy = true;
  actionButtons.forEach(button => { button.disabled = true; });
  error.textContent = '';
  try {
    await action();
  } catch {
    error.textContent = 'Action failed. Try again.';
  } finally {
    try {
      actionButtons.forEach(button => { button.disabled = false; });
      render(await window.launcher.state());
    } catch {
      error.textContent = 'Could not read launcher state.';
    }
    busy = false;
  }
}

actionButtons.forEach(button => {
  button.onclick = () => update(window.launcher[button.id]);
});
update(() => {});
