#!/usr/bin/env node
'use strict';

import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pkgRoot = join(__dirname, '..');

function getBinaryName() {
  const platform = process.platform;
  const arch = process.arch;

  // Map to the naming convention used in dist/
  const platformArchMap = {
    'linux:x64': 'codei-x86_64-unknown-linux-musl',
    'linux:arm64': 'codei-aarch64-unknown-linux-musl',
    'darwin:x64': 'codei-x86_64-apple-darwin',
    'darwin:arm64': 'codei-aarch64-apple-darwin',
    'win32:x64': 'codei-x86_64-pc-windows-gnullvm.exe',
    'win32:arm64': 'codei-aarch64-pc-windows-gnullvm.exe',
  };

  const key = `${platform}:${arch}`;
  return platformArchMap[key];
}

function main() {
  const binaryName = getBinaryName();

  if (!binaryName) {
    console.error(
      `Unsupported platform: ${process.platform} ${process.arch}\n` +
        `codei supports: linux/darwin/win32 on x64/arm64`
    );
    process.exit(1);
  }

  // Try bundled binary first (from platform-specific package)
  const bundledPath = join(pkgRoot, 'bin', binaryName);

  if (existsSync(bundledPath)) {
    const child = spawn(bundledPath, process.argv.slice(2), {
      stdio: 'inherit',
      env: process.env,
    });

    child.on('error', (err) => {
      console.error(`Failed to start codei: ${err.message}`);
      process.exit(1);
    });

    child.on('exit', (code, signal) => {
      if (signal) {
        process.exit(1);
      } else {
        process.exit(code ?? 1);
      }
    });
    return;
  }

  console.error(
    `codei binary not found for ${process.platform} ${process.arch}.\n` +
      `Please ensure you installed the correct platform package.`
  );
  process.exit(1);
}

main();
