const { spawn } = require('node:child_process');
const { mkdtemp, readFile, rm } = require('node:fs/promises');
const { createServer } = require('node:http');
const { tmpdir } = require('node:os');
const { join, resolve } = require('node:path');
const electronPath = require('electron');

async function main() {
  const app = resolve(__dirname, '..');
  const fixture = (await readFile(join(app, 'ui', 'fixture.html'), 'utf8')).replace('</body>',
    '<a id="next-page" href="/next">Next page</a></body>');
  const server = createServer((request, response) => {
    response.setHeader('Content-Type', 'text/html; charset=utf-8');
    if (request.url === '/waiting-room') response.end(fixture);
    else if (request.url === '/next') response.end('<!doctype html><h1>Navigation check</h1>');
    else response.writeHead(404).end();
  });
  const data = await mkdtemp(join(tmpdir(), 'queue-launcher-electron-'));
  try {
    await new Promise((accept, reject) => {
      server.once('error', reject);
      server.listen(0, '127.0.0.1', accept);
    });
    const url = 'http://127.0.0.1:' + server.address().port + '/waiting-room';
    for (const phase of ['create', 'restore', 'limit']) {
      await new Promise((accept, reject) => {
        const child = spawn(electronPath, [app], {
          stdio: 'inherit',
          env: {
            ...process.env,
            QUEUE_LAUNCHER_TEST: phase,
            QUEUE_LAUNCHER_TEST_DATA: data,
            QUEUE_LAUNCHER_TEST_URL: url,
          },
        });
        const timer = setTimeout(() => child.kill(), 150_000);
        child.once('error', error => {
          clearTimeout(timer);
          reject(error);
        });
        child.once('close', (code, signal) => {
          clearTimeout(timer);
          if (code === 0) accept();
          else reject(new Error(phase + ' failed with ' + (signal ?? code)));
        });
      });
    }
  } finally {
    server.close();
    await rm(data, { recursive: true, force: true, maxRetries: 8, retryDelay: 250 });
  }
}

main().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
