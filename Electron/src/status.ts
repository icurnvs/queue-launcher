export const uuidPattern = /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i;

export function parseStatus(text: string, errorId: string) {
  const flat = text.replace(/\s+/g, ' ');
  const capture = (pattern: RegExp) => pattern.exec(flat)?.[1];
  const queueId = capture(/queue id\s*:\s*([0-9a-f-]{36})\b/i);
  const ahead = capture(/(?:members ahead of you|number of users in line ahead of you)\s*:\s*([0-9]+(?:,[0-9]{3})*)\b/i);
  const message = /no more attempts/i.test(flat) ? 'No more attempts'
    : /the event has ended|this product has sold out/i.test(flat) ? 'Event ended'
    : /(your|this) (queue session|session|queue number|turn) has expired/i.test(flat) ? 'Session expired'
    : undefined;
  const code = /^\s*[+-]?\d+\s*$/.test(errorId) ? Number(errorId) : NaN;
  return {
    queueId: !message && queueId && uuidPattern.test(queueId) ? queueId.toLowerCase() : undefined,
    ahead: !message && ahead && Number(ahead.replaceAll(',', '')) <= 2147483647
      ? Number(ahead.replaceAll(',', '')) : undefined,
    wait: message ? undefined : capture(/(?:estimated time to enter|your estimated wait time is)\s*:\s*((?:(?:more|less) than |about )?\d+\s*(?:hours?|minutes?|seconds?)(?:\s+(?:and\s+)?\d+\s*(?:minutes?|seconds?))?|over an hour|under a minute|calculating)/i),
    purchaseWindow: message ? undefined : capture(/(\d+\s+minutes?)\s+to complete your purchase/i),
    remaining: message ? undefined : capture(/(?:time remaining|time left)\s*:\s*(\d{1,2}:\d{2}(?::\d{2})?|\d+\s+(?:minutes?|seconds?))/i),
    waiting: !message && /you are (?:now )?in (?:the |a )?(?:waiting room|(?:virtual )?queue)|in the waiting room/i.test(flat),
    message,
    code: Number.isInteger(code) && Math.abs(code) <= 2147483647 ? code : undefined,
  };
}

export type QueueStatus = ReturnType<typeof parseStatus>;

export function observeStatus(status: QueueStatus, challengeVisible: boolean) {
  const event = status.message === 'No more attempts' ? 'no_more_attempts'
    : status.message === 'Event ended' ? 'event_ended'
    : status.message === 'Session expired' ? 'session_expired'
    : status.code !== undefined ? 'queue_error'
    : status.ahead !== undefined ? 'queue_position'
    : status.waiting ? 'waiting_room'
    : challengeVisible ? 'captcha_visible'
    : undefined;
  return event ? { event, code: status.code, membersAhead: status.ahead } : undefined;
}

export function isWaitingRoom(url: URL) {
  return url.protocol === 'https:' && url.hostname === 'wr.costco.com' && !url.port && !url.username && !url.password;
}

function parameters(url: URL) {
  return url.search.slice(1).split('&').filter(Boolean).map(raw => {
    const index = raw.indexOf('=');
    const name = index < 0 ? raw : raw.slice(0, index);
    const value = index < 0 ? '' : raw.slice(index + 1);
    const decode = (part: string) => {
      try { return decodeURIComponent(part); } catch { return part; }
    };
    return { raw, name: decode(name), value: decode(value) };
  });
}

export function resumeUrl(status: QueueStatus, url: URL) {
  if (status.message || !isWaitingRoom(url)) return undefined;
  const items = parameters(url);
  if (items.some(item => item.name === 'q' && uuidPattern.test(item.value))) return url.href;
  if (!status.queueId) return undefined;
  const result = new URL(url);
  result.search = [...items.filter(item => item.name !== 'q').map(item => item.raw),
    'q=' + status.queueId].join('&');
  return result.href;
}

export function shareUrl(status: QueueStatus, url: URL) {
  const resume = resumeUrl(status, url);
  if (status.ahead === undefined || !resume) return undefined;
  const result = new URL(resume);
  const items = parameters(result);
  const required = ['c', 'e', 't', 'q'];
  for (const name of required) {
    const matches = items.filter(item => item.name === name);
    if (matches.length !== 1 || !matches[0].value) return undefined;
  }
  result.pathname = '/';
  result.hash = '';
  result.search = items.filter(item => required.includes(item.name) || item.name === 'cid')
    .map(item => item.raw).join('&');
  return result.href;
}
