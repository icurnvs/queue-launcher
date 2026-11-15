const assert = require('node:assert/strict');
const { test } = require('node:test');
const { parseStatus, observeStatus, resumeUrl, shareUrl } = require('../dist/status.js');

const id = '274eb8c4-773b-47d2-b68a-a430522318b6';
const base = 'https://wr.costco.com/wait?c=costco&e=drop&t=https%3A%2F%2Fwww.costco.com%2Fa%3Fx%3D1%252B2&cid=en-US';
const status = parseStatus([
  'Queue ID: ' + id, 'You are now in the waiting room', 'Members ahead of you: 1,234',
  'Estimated time to enter: about 1 hour and 12 minutes',
  'You have 10 minutes to complete your purchase', 'Time remaining: 09:41',
].join('\n'), '');

test('parses visible waiting-room details', () => {
  assert.deepEqual(status, {
    queueId: id, ahead: 1234, wait: 'about 1 hour and 12 minutes',
    purchaseWindow: '10 minutes', remaining: '09:41', waiting: true,
    message: undefined, code: undefined,
  });
});

test('constructs a scoped resume URL and preserves encoded share parameters', () => {
  const url = new URL(base + '&token=private#fragment');
  assert.equal(resumeUrl(status, url), base + '&token=private&q=' + id + '#fragment');
  assert.equal(shareUrl(status, url), base.replace('/wait?', '/?') + '&q=' + id);
  assert.equal(shareUrl(status, new URL(base + '&e=duplicate')), undefined);
  assert.equal(shareUrl(status, new URL(base + '&q=' + id + '&q=' + id)), undefined);
  assert.equal(resumeUrl(status, new URL('https://other.example/')), undefined);
});

test('prioritizes queue errors and records a manually handled challenge', () => {
  assert.deepEqual(observeStatus(parseStatus('No more attempts left', '42'), true),
    { event: 'no_more_attempts', code: 42, membersAhead: undefined });
  assert.equal(observeStatus(parseStatus('Waiting for verification', ''), true).event, 'captcha_visible');
  assert.equal(parseStatus('This queue number has expired', '').message, 'Session expired');
  assert.equal(parseStatus('This product has sold out', '').message, 'Event ended');
});
