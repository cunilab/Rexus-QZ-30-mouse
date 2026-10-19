import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';

const script = fileURLToPath(new URL('../scripts/version.mjs', import.meta.url));
function fixture(t, version = '0.1.0') {
  const root = mkdtempSync(join(tmpdir(), 'qz-version-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, 'src-tauri'));
  const files = {
    'package.json': JSON.stringify({ name: 'qz-flow', version, scripts: { test: 'keep me' } }),
    'package-lock.json': JSON.stringify({ name: 'qz-flow', version, packages: { '': { version }, 'node_modules/example': { version: '0.1.0', integrity: 'preserve' } } }),
    'src-tauri/tauri.conf.json': JSON.stringify({ version, app: { windows: [{ label: 'main' }] } }),
    'src-tauri/Cargo.toml': `[package]\r\nname = "qz-flow"\r\nversion = "${version}" # keep comment\r\n\r\n[dependencies]\r\nother = "0.1.0"\r\n`,
    'src-tauri/Cargo.lock': `version = 4\n\n[[package]]\nname = "another"\nversion = "0.1.0"\n\n[[package]]\nname = "qz-flow"\nversion = "${version}"\ndependencies = ["another"]\n\n[[package]]\nname = "zebra"\nversion = "0.1.0"\n`,
  };
  for (const [path, content] of Object.entries(files)) writeFileSync(join(root, path), content);
  return { root, files, read: (path) => readFileSync(join(root, path), 'utf8'),
    run: (...args) => spawnSync(process.execPath, [script, ...args], { cwd: root, encoding: 'utf8' }) };
}

test('check validates all versions and exact release tag', (t) => {
  const f = fixture(t);
  assert.equal(f.run('check').status, 0);
  assert.equal(f.run('check', '--tag', 'v0.1.0').status, 0);
  for (const tag of ['0.1.0', 'v0.2.0', 'v0.1.0-extra']) {
    const result = f.run('check', '--tag', tag);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /Tag mismatch/);
  }
});

test('mismatch, missing version and malformed files fail without mutations', (t) => {
  const f = fixture(t);
  const path = 'package-lock.json';
  const lock = JSON.parse(f.read(path));
  lock.packages[''].version = '0.2.0';
  writeFileSync(join(f.root, path), JSON.stringify(lock));
  assert.match(f.run('check').stderr, /Version mismatch/);
  assert.match(f.run('set', '0.3.0').stderr, /Version mismatch/);
  assert.equal(f.read('package.json'), f.files['package.json']);
  delete lock.packages[''].version;
  writeFileSync(join(f.root, path), JSON.stringify(lock));
  assert.match(f.run('check').stderr, /Invalid version/);
  writeFileSync(join(f.root, path), '{broken');
  assert.equal(f.run('check').status, 1);
  writeFileSync(join(f.root, path), f.files[path]);
  writeFileSync(join(f.root, 'src-tauri/Cargo.lock'), '[[package]]\nname = "other"\nversion = "0.1.0"\n');
  assert.match(f.run('check').stderr, /Expected exactly one qz-flow package/);
});

test('set preserves unrelated settings and dependencies, then passes tag check', (t) => {
  const f = fixture(t);
  assert.equal(f.run('set', '1.2.3-beta.1').status, 0);
  assert.equal(f.run('check', '--tag', 'v1.2.3-beta.1').status, 0);
  assert.deepEqual(JSON.parse(f.read('package.json')).scripts, { test: 'keep me' });
  assert.deepEqual(JSON.parse(f.read('package-lock.json')).packages['node_modules/example'], { version: '0.1.0', integrity: 'preserve' });
  assert.deepEqual(JSON.parse(f.read('src-tauri/tauri.conf.json')).app, { windows: [{ label: 'main' }] });
  for (const path of ['src-tauri/Cargo.toml', 'src-tauri/Cargo.lock']) {
    assert.equal(f.read(path), f.files[path].replace('version = "0.1.0"' + (path.endsWith('.toml') ? ' # keep comment' : '\ndependencies'), 'version = "1.2.3-beta.1"' + (path.endsWith('.toml') ? ' # keep comment' : '\ndependencies')));
  }
  assert.equal(f.run('set', '1.2.3').status, 0);
  assert.equal(f.run('check', '--tag', 'v1.2.3').status, 0);
});

test('strict semver and increasing precedence are enforced', (t) => {
  const f = fixture(t, '1.2.3-beta.2');
  for (const version of ['1.2.3-beta.2', '1.2.3-beta.1', '1.2.2', '01.2.3', '1.2', '1.2.3-01', '1.2.3+build', '1.2.3-', 'v1.2.4']) {
    assert.equal(f.run('set', version).status, 1, version);
  }
  assert.equal(f.run('set', '1.2.3-beta.10').status, 0);
  assert.equal(f.run('set', '1.2.3-rc.1').status, 0);
  assert.equal(f.run('set', '1.2.3').status, 0);
  assert.equal(f.run('set', '1.2.3-rc.2').status, 1);
  assert.equal(f.run('set', '2.0.0-0').status, 0);
});

test('a failed rename restores originals and removes staged files', (t) => {
  const f = fixture(t);
  const hook = join(f.root, 'fail-rename.mjs');
  writeFileSync(hook, `import fs from 'node:fs';\nimport { syncBuiltinESMExports } from 'node:module';\nconst rename = fs.renameSync;\nlet calls = 0;\nfs.renameSync = (...args) => { if (++calls === 3) throw new Error('simulated rename failure'); return rename(...args); };\nsyncBuiltinESMExports();\n`);
  const result = spawnSync(process.execPath, ['--import', pathToFileURL(hook).href, script, 'set', '0.2.0'], { cwd: f.root, encoding: 'utf8' });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /simulated rename failure; original files restored/);
  for (const [path, content] of Object.entries(f.files)) assert.equal(f.read(path), content, path);
  for (const directory of [f.root, resolve(f.root, 'src-tauri')]) assert.ok(readdirSync(directory).every((name) => !name.endsWith('.tmp')));
});
