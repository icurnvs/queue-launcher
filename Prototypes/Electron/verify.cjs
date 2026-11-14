const { spawn } = require('node:child_process');
const { mkdtemp, rm } = require('node:fs/promises');
const { createServer } = require('node:net');
const { tmpdir } = require('node:os');
const { join } = require('node:path');
const electron = require('electron');

async function availablePort() {
  const server = createServer();
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const { port } = server.address();
  await new Promise(resolve => server.close(resolve));
  return port;
}

async function batch(modes) {
  const data = await mkdtemp(join(tmpdir(), 'queue-electron-'));
  const port = await availablePort();
  try {
    for (const mode of modes) {
      await new Promise((resolve, reject) => {
        const child = spawn(electron, ['.'], {
          cwd: __dirname,
          env: {
            ...process.env,
            QUEUE_PROTOTYPE_VERIFY: mode,
            QUEUE_PROTOTYPE_DATA: data,
            QUEUE_PROTOTYPE_PORT: String(port),
          },
          stdio: 'inherit',
        });
        const timer = setTimeout(() => child.kill(), 120_000);
        child.once('error', error => {
          clearTimeout(timer);
          reject(error);
        });
        child.once('close', (code, signal) => {
          clearTimeout(timer);
          if (code === 0) resolve();
          else reject(new Error(mode + ' failed (' + (signal ?? code) + ')'));
        });
      });
    }
  } finally {
    await rm(data, { recursive: true, force: true, maxRetries: 8, retryDelay: 250 });
  }
}

(async () => {
  console.log('Electron prototype checks on ' + process.platform + ' ' + process.arch);
  await batch(['create', 'restore', 'after-close']);
  await batch(['scale-12', 'scale-12']);
  await batch(['scale-50', 'scale-50']);
})().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
