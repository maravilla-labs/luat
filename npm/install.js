#!/usr/bin/env node

const https = require('https');
const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');
const zlib = require('zlib');

const REPO = 'maravilla-labs/luat';
const BIN_NAME = process.platform === 'win32' ? 'luat.exe' : 'luat';
const BIN_DIR = path.join(__dirname, 'bin');
const BIN_PATH = path.join(BIN_DIR, BIN_NAME);

function getPlatform() {
  const platform = process.platform;
  const arch = process.arch;

  const platforms = {
    'darwin-x64': 'x86_64-apple-darwin',
    'darwin-arm64': 'aarch64-apple-darwin',
    'linux-x64': 'x86_64-unknown-linux-gnu',
    'linux-arm64': 'aarch64-unknown-linux-gnu',
    'win32-x64': 'x86_64-pc-windows-msvc',
  };

  const key = `${platform}-${arch}`;
  const target = platforms[key];

  if (!target) {
    console.error(`Unsupported platform: ${platform}-${arch}`);
    console.error('Supported platforms: darwin-x64, darwin-arm64, linux-x64, linux-arm64, win32-x64');
    process.exit(1);
  }

  return target;
}

async function getLatestVersion() {
  return new Promise((resolve, reject) => {
    const options = {
      hostname: 'api.github.com',
      path: `/repos/${REPO}/releases/latest`,
      headers: {
        'User-Agent': 'luat-npm-installer',
      },
    };

    https.get(options, (res) => {
      let data = '';
      res.on('data', (chunk) => (data += chunk));
      res.on('end', () => {
        try {
          const release = JSON.parse(data);
          resolve(release.tag_name);
        } catch (e) {
          reject(new Error('Failed to parse release data'));
        }
      });
    }).on('error', reject);
  });
}

async function downloadFile(url, dest) {
  return new Promise((resolve, reject) => {
    const follow = (url) => {
      https.get(url, { headers: { 'User-Agent': 'luat-npm-installer' } }, (res) => {
        if (res.statusCode === 302 || res.statusCode === 301) {
          follow(res.headers.location);
          return;
        }

        if (res.statusCode !== 200) {
          reject(new Error(`Failed to download: ${res.statusCode}`));
          return;
        }

        const file = fs.createWriteStream(dest);
        res.pipe(file);
        file.on('finish', () => {
          file.close();
          resolve();
        });
      }).on('error', reject);
    };

    follow(url);
  });
}

async function extractTarGz(tarPath, destDir) {
  return new Promise((resolve, reject) => {
    const tar = require('child_process');
    try {
      execSync(`tar -xzf "${tarPath}" -C "${destDir}"`, { stdio: 'inherit' });
      resolve();
    } catch (e) {
      reject(e);
    }
  });
}

async function extractZip(zipPath, destDir) {
  return new Promise((resolve, reject) => {
    try {
      execSync(`powershell -command "Expand-Archive -Force '${zipPath}' '${destDir}'"`, { stdio: 'inherit' });
      resolve();
    } catch (e) {
      reject(e);
    }
  });
}

async function install() {
  console.log('Installing luat...');

  const platform = getPlatform();
  const version = await getLatestVersion();
  const isWindows = process.platform === 'win32';
  const ext = isWindows ? 'zip' : 'tar.gz';
  const archiveName = `luat-${version}-${platform}.${ext}`;
  const downloadUrl = `https://github.com/${REPO}/releases/download/${version}/${archiveName}`;

  console.log(`Platform: ${platform}`);
  console.log(`Version: ${version}`);
  console.log(`Downloading from: ${downloadUrl}`);

  // Create bin directory
  if (!fs.existsSync(BIN_DIR)) {
    fs.mkdirSync(BIN_DIR, { recursive: true });
  }

  const archivePath = path.join(BIN_DIR, archiveName);

  try {
    await downloadFile(downloadUrl, archivePath);
    console.log('Download complete, extracting...');

    if (isWindows) {
      await extractZip(archivePath, BIN_DIR);
    } else {
      await extractTarGz(archivePath, BIN_DIR);
    }

    // Clean up archive
    fs.unlinkSync(archivePath);

    // Make binary executable (Unix)
    if (!isWindows) {
      fs.chmodSync(BIN_PATH, 0o755);
    }

    console.log(`luat installed successfully to ${BIN_PATH}`);
  } catch (error) {
    console.error('Installation failed:', error.message);
    console.error('');
    console.error('You can manually download the binary from:');
    console.error(`https://github.com/${REPO}/releases`);
    process.exit(1);
  }
}

install();
