#!/usr/bin/env node
'use strict';

/**
 * Postinstall script: copies the correct platform binary into bin/
 * so the main bin/codei.js can find it.
 *
 * The platform-specific npm packages (e.g. codei-linux-x64-musl)
 * are installed as optionalDependencies and contain the actual binary.
 */

import { chmodSync, copyFileSync, existsSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pkgRoot = join(__dirname, '..');
const binDir = join(pkgRoot, 'bin');

function getBinaryName() {
  const platform = process.platform;
  const arch = process.arch;

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

function findPlatformPkgName() {
  const platform = process.platform;
  const arch = process.arch;

  const pkgNameMap = {
    'linux:x64': '@codei/codei-linux-x64-musl',
    'linux:arm64': '@codei/codei-linux-arm64-musl',
    'darwin:x64': '@codei/codei-darwin-x64',
    'darwin:arm64': '@codei/codei-darwin-arm64',
    'win32:x64': '@codei/codei-win32-x64',
    'win32:arm64': '@codei/codei-win32-arm64',
  };

  const key = `${platform}:${arch}`;
  return pkgNameMap[key];
}

function main() {
  const binaryName = getBinaryName();
  const pkgName = findPlatformPkgName();

  if (!binaryName || !pkgName) {
    // Unsupported platform — skip silently (optionalDependencies will also skip)
    return;
  }

  // Ensure bin directory exists
  if (!existsSync(binDir)) {
    mkdirSync(binDir, { recursive: true });
  }

  // The platform-specific package is installed under node_modules/<pkgName>/bin/
  const platformPkgBin = join(pkgRoot, 'node_modules', pkgName, 'bin', binaryName);

  if (!existsSync(platformPkgBin)) {
    // Binary not found in platform package — might be a dev/local setup
    // Check if binary already exists in bin/
    const localBin = join(binDir, binaryName);
    if (existsSync(localBin)) {
      return; // Already present
    }
    console.error(
      `[codei] Binary not found in platform package ${pkgName}. ` +
        `Skipping postinstall copy.`
    );
    return;
  }

  const destPath = join(binDir, binaryName);
  try {
    copyFileSync(platformPkgBin, destPath);
    // Make executable on Unix
    if (process.platform !== 'win32') {
      chmodSync(destPath, 0o755);
    }
  } catch (err) {
    console.error(`[codei] Failed to copy binary: ${err.message}`);
  }
}

main();
