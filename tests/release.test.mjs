import test from 'node:test';
import assert from 'node:assert/strict';
import { generateKeyPairSync, sign, createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { assemble, assetName, platforms, verifySignature } from '../scripts/release.mjs';
const script = fileURLToPath(new URL('../scripts/release.mjs', import.meta.url));

function signingFixture() {
  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  const id = Buffer.from('0102030405060708', 'hex');
  const key = Buffer.concat([Buffer.from('Ed'), id, publicKey.export({ format: 'der', type: 'spki' }).subarray(-32)]);
  const encodedKey = Buffer.from(`untrusted comment: fixture\n${key.toString('base64')}\n`).toString('base64');
  return {
    key: encodedKey,
    signature(data) {
      const main = sign(null, createHash('blake2b512').update(data).digest(), privateKey);
      const comment = 'timestamp:1234';
      const global = sign(null, Buffer.concat([main, Buffer.from(comment)]), privateKey);
      const sig = Buffer.concat([Buffer.from('ED'), id, main]);
      return Buffer.from(`untrusted comment: fixture\n${sig.toString('base64')}\ntrusted comment: ${comment}\n${global.toString('base64')}\n`).toString('base64');
    },
  };
}

test('updater verification rejects altered payload, key, comment and malformed signatures', () => {
  const fixture = signingFixture();
  const payload = Buffer.from('installer');
  const signature = fixture.signature(payload);
  verifySignature(payload, signature, fixture.key);
  assert.throws(() => verifySignature(Buffer.from('tampered'), signature, fixture.key), /artifact signature/);
  assert.throws(() => verifySignature(payload, signature, signingFixture().key), /artifact signature/);
  const changedComment = Buffer.from(Buffer.from(signature, 'base64').toString().replace('timestamp:1234', 'timestamp:9999')).toString('base64');
  assert.throws(() => verifySignature(payload, changedComment, fixture.key), /trusted-comment/);
  assert.throws(() => verifySignature(payload, 'bad', fixture.key), /Malformed/);
});

function assets(directory, fixture) {
  for (const platform of platforms) {
    const updater = assetName(`${platform}-QZ Flow${platform.startsWith('windows') ? '-setup.exe' : '.app.tar.gz'}`);
    const data = Buffer.from(`installer ${platform}`);
    const signature = fixture.signature(data);
    const files = [[updater, data], [`${updater}.sig`, Buffer.from(signature)]];
    if (platform.startsWith('darwin')) files.push([`${platform}-QZ.Flow.dmg`, Buffer.from('disk image')]);
    for (const [name, content] of files) writeFileSync(join(directory, name), content);
    writeFileSync(join(directory, `${platform}.json`), JSON.stringify({ platform, version: '0.1.0', tag: 'v0.1.0', updater, signature,
      files: files.map(([name, content]) => ({ name, sha256: createHash('sha256').update(content).digest('hex') })),
    }));
  }
}

test('release assembly requires all three intact platforms and generates tag-specific feed/checksums', () => {
  const directory = mkdtempSync(join(tmpdir(), 'qz-release-'));
  const fixture = signingFixture();
  try {
    assets(directory, fixture);
    const result = assemble(directory, 'v0.1.0', 'owner/repo', fixture.key);
    const feed = JSON.parse(readFileSync(join(directory, 'latest.json'), 'utf8'));
    assert.deepEqual(Object.keys(feed.platforms), platforms);
    assert.match(feed.platforms['darwin-x86_64'].url, /releases\/download\/v0.1.0\/darwin-x86_64-QZ.Flow.app.tar.gz$/);
    assert.equal(assetName('QZ Flow_0.1.0.exe'), 'QZ.Flow_0.1.0.exe');
    assert.ok(result.includes('SHA256SUMS.txt'));
    const checksumLines = readFileSync(join(directory, 'SHA256SUMS.txt'), 'utf8').trim().split('\n');
    assert.equal(checksumLines.length, result.length - 1);
    rmSync(join(directory, 'latest.json'));
    rmSync(join(directory, 'SHA256SUMS.txt'));
    writeFileSync(join(directory, 'darwin-x86_64-QZ.Flow.app.tar.gz'), 'altered');
    assert.throws(() => assemble(directory, 'v0.1.0', 'owner/repo', fixture.key), /checksum mismatch/);
    assets(directory, fixture);
    rmSync(join(directory, 'darwin-x86_64.json'));
    assert.throws(() => assemble(directory, 'v0.1.0', 'owner/repo', fixture.key), /ENOENT/);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test('release assembly rejects traversal metadata, wrong versions and stale extra assets', () => {
  const directory = mkdtempSync(join(tmpdir(), 'qz-release-'));
  const fixture = signingFixture();
  try {
    assets(directory, fixture);
    assert.throws(() => assemble(directory, 'v0.2.0', 'owner/repo', fixture.key), /Mismatched/);
    const path = join(directory, 'windows-x86_64.json');
    const record = JSON.parse(readFileSync(path, 'utf8'));
    record.files[0].name = '../outside.exe';
    writeFileSync(path, JSON.stringify(record));
    assert.throws(() => assemble(directory, 'v0.1.0', 'owner/repo', fixture.key), /Unsafe/);
    assets(directory, fixture);
    writeFileSync(join(directory, 'stale.exe'), 'stale');
    assert.throws(() => assemble(directory, 'v0.1.0', 'owner/repo', fixture.key), /Unexpected/);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test('stage normalizes GitHub filenames and rejects tampered CLI artifacts', () => {
  const directory = mkdtempSync(join(tmpdir(), 'qz-stage-'));
  const fixture = signingFixture();
  try {
    mkdirSync(join(directory, 'src-tauri'));
    writeFileSync(join(directory, 'package.json'), JSON.stringify({ version: '0.1.0' }));
    writeFileSync(join(directory, 'src-tauri/tauri.release.conf.json'), JSON.stringify({ plugins: { updater: { pubkey: fixture.key } } }));
    const installer = join(directory, 'QZ Flow_0.1.0_x64-setup.exe');
    const data = Buffer.from('valid installer');
    writeFileSync(installer, data);
    writeFileSync(`${installer}.sig`, fixture.signature(data));
    const env = { ...process.env, RELEASE_TAG: 'v0.1.0', RELEASE_PLATFORM: 'windows-x86_64', ARTIFACT_PATHS: JSON.stringify([installer]) };
    const valid = spawnSync(process.execPath, [script, 'stage'], { cwd: directory, env, encoding: 'utf8' });
    assert.equal(valid.status, 0, valid.stderr);
    const record = JSON.parse(readFileSync(join(directory, 'release-assets/windows-x86_64.json'), 'utf8'));
    assert.equal(record.updater, 'windows-x86_64-QZ.Flow_0.1.0_x64-setup.exe');
    assert.ok(record.files.some(file => file.name === `${record.updater}.sig`));
    rmSync(join(directory, 'release-assets'), { recursive: true });
    writeFileSync(installer, 'tampered');
    const invalid = spawnSync(process.execPath, [script, 'stage'], { cwd: directory, env, encoding: 'utf8' });
    assert.notEqual(invalid.status, 0);
    assert.match(invalid.stderr, /artifact signature/);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test('release guard rejects unsafe tags and commits outside main before network access', () => {
  const directory = mkdtempSync(join(tmpdir(), 'qz-guard-'));
  try {
    const git = args => execFileSync('git', args, { cwd: directory, stdio: 'pipe' });
    git(['init', '-b', 'main']);
    git(['-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '--allow-empty', '-m', 'main']);
    git(['update-ref', 'refs/remotes/origin/main', 'HEAD']);
    git(['checkout', '-b', 'unmerged']);
    git(['-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '--allow-empty', '-m', 'unmerged']);
    git(['tag', 'v0.1.0']);
    for (const tag of ['--upload-pack=bad', 'v0.1.0']) {
      const result = spawnSync(process.execPath, [script, 'guard'], { cwd: directory, env: { ...process.env, RELEASE_TAG: tag, GH_TOKEN: '' }, encoding: 'utf8' });
      assert.notEqual(result.status, 0);
      assert.doesNotMatch(result.stderr, /GH_TOKEN is required/);
    }
  } finally { rmSync(directory, { recursive: true, force: true }); }
});
