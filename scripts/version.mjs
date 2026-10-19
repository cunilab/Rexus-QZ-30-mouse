#!/usr/bin/env node
import { readFileSync, writeFileSync, renameSync, unlinkSync } from 'node:fs';
import { resolve, dirname, basename, join } from 'node:path';
import { randomUUID } from 'node:crypto';

// Build metadata is deliberately excluded: release tags and updater versions must agree exactly.
const semver = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*))?$/;
function parseVersion(value) {
  const match = typeof value === 'string' && semver.exec(value);
  if (!match) throw new Error(`Invalid version: ${String(value)} (expected X.Y.Z or X.Y.Z-prerelease; no build metadata)`);
  return { core: match.slice(1, 4).map(BigInt), pre: match[4]?.split('.') };
}

function compareVersions(left, right) {
  const a = parseVersion(left);
  const b = parseVersion(right);
  for (let i = 0; i < 3; i++) {
    if (a.core[i] !== b.core[i]) return a.core[i] > b.core[i] ? 1 : -1;
  }
  if (!a.pre || !b.pre) return a.pre === b.pre ? 0 : a.pre ? -1 : 1;
  for (let i = 0; i < Math.max(a.pre.length, b.pre.length); i++) {
    if (a.pre[i] === undefined) return -1;
    if (b.pre[i] === undefined) return 1;
    if (a.pre[i] === b.pre[i]) continue;
    const aNumeric = /^\d+$/.test(a.pre[i]);
    const bNumeric = /^\d+$/.test(b.pre[i]);
    if (aNumeric && bNumeric) return BigInt(a.pre[i]) > BigInt(b.pre[i]) ? 1 : -1;
    if (aNumeric !== bNumeric) return aNumeric ? -1 : 1;
    return a.pre[i] > b.pre[i] ? 1 : -1;
  }
  return 0;
}

function tomlPackage(text, lock) {
  const sections = [...text.matchAll(/^\[([^\r\n]+)\][ \t]*(?:\r?\n|$)/gm)];
  const matches = sections.flatMap((section, index) => {
    if (section[1] !== (lock ? '[package]' : 'package')) return [];
    const start = section.index + section[0].length;
    const end = sections[index + 1]?.index ?? text.length;
    const body = text.slice(start, end);
    if (!/^name[ \t]*=[ \t]*"qz-flow"[ \t]*(?:#.*)?$/m.test(body)) return [];
    const versions = [...body.matchAll(/^version[ \t]*=[ \t]*"([^"\r\n]+)"[ \t]*(?:#.*)?$/gm)];
    if (versions.length !== 1) throw new Error('Expected exactly one qz-flow package version in TOML');
    const version = versions[0];
    const offset = start + version.index + version[0].indexOf('"') + 1;
    return [{ version: version[1], offset }];
  });
  if (matches.length !== 1) throw new Error('Expected exactly one qz-flow package in TOML');
  return matches[0];
}

function loadFiles() {
  const paths = ['package.json', 'package-lock.json', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock', 'src-tauri/tauri.conf.json'];
  return paths.map((path) => {
    const original = readFileSync(resolve(path), 'utf8');
    if (path.endsWith('.json')) {
      const data = JSON.parse(original);
      const versions = path === 'package-lock.json'
        ? [data.version, data.packages?.['']?.version] : [data.version];
      return { path, original, versions, update(version) {
        data.version = version;
        if (path === 'package-lock.json') data.packages[''].version = version;
        const newline = original.includes('\r\n') ? '\r\n' : '\n';
        return JSON.stringify(data, null, 2).replaceAll('\n', newline) + newline;
      } };
    }
    const entry = tomlPackage(original, path.endsWith('.lock'));
    return { path, original, versions: [entry.version], update(version) {
      return original.slice(0, entry.offset) + version + original.slice(entry.offset + entry.version.length);
    } };
  });
}

function atomicWrite(path, content) {
  const target = resolve(path);
  const temporary = join(dirname(target), `.${basename(target)}.${randomUUID()}.tmp`);
  try {
    writeFileSync(temporary, content, { flag: 'wx' });
    renameSync(temporary, target);
  } finally {
    try { unlinkSync(temporary); } catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
}

function main() {
  const [command, ...args] = process.argv.slice(2);
  const validCheck = command === 'check' && (args.length === 0 || (args.length === 2 && args[0] === '--tag'));
  const validSet = command === 'set' && args.length === 1;
  if (!validCheck && !validSet) throw new Error('Usage: node scripts/version.mjs check [--tag vX.Y.Z] | set X.Y.Z');
  const files = loadFiles();
  const current = files[0].versions[0];
  for (const file of files) {
    for (const version of file.versions) {
      parseVersion(version);
      if (version !== current) throw new Error(`Version mismatch: ${file.path} has ${version}; expected ${current}`);
    }
  }
  if (validCheck) {
    if (args.length && args[1] !== `v${current}`) throw new Error(`Tag mismatch: expected v${current}, got ${args[1]}`);
    console.log(`Versions agree: ${current}${args.length ? ` (${args[1]})` : ''}`);
    return;
  }
  const next = args[0];
  if (compareVersions(next, current) <= 0) throw new Error(`Version must increase from ${current}; got ${next}`);
  const changed = [];
  try {
    for (const file of files) {
      atomicWrite(file.path, file.update(next));
      changed.push(file);
    }
  } catch (error) {
    const rollbackErrors = [];
    for (const file of changed.reverse()) {
      try { atomicWrite(file.path, file.original); }
      catch (restoreError) { rollbackErrors.push(`${file.path}: ${restoreError.message}`); }
    }
    throw new Error(`${error.message}; ${rollbackErrors.length ? `ROLLBACK FAILED: ${rollbackErrors.join('; ')}` : 'original files restored'}`);
  }
  console.log(`Version updated: ${current} -> ${next}. No commit or tag created.`);
}

try { main(); }
catch (error) { console.error(error.message); process.exitCode = 1; }
