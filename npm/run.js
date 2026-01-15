#!/usr/bin/env node

const { spawn } = require('child_process');
const path = require('path');

const BIN_NAME = process.platform === 'win32' ? 'luat.exe' : 'luat';
const BIN_PATH = path.join(__dirname, 'bin', BIN_NAME);

const child = spawn(BIN_PATH, process.argv.slice(2), {
  stdio: 'inherit',
  shell: false,
});

child.on('error', (err) => {
  if (err.code === 'ENOENT') {
    console.error('luat binary not found. Try reinstalling:');
    console.error('  npm uninstall -g @maravilla-labs/luat');
    console.error('  npm install -g @maravilla-labs/luat');
  } else {
    console.error('Failed to run luat:', err.message);
  }
  process.exit(1);
});

child.on('exit', (code) => {
  process.exit(code || 0);
});
