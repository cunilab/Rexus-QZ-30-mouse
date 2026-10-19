import { appendFileSync, copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { createHash, createPublicKey, verify } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { basename, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export const platforms = ['windows-x86_64', 'darwin-aarch64', 'darwin-x86_64'];
const readJson = path => JSON.parse(readFileSync(path, 'utf8'));
const run = (command, args) => execFileSync(command, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
const sha256 = path => createHash('sha256').update(readFileSync(path)).digest('hex');
export const assetName = name => name.replace(/[^A-Za-z0-9._-]/g, '.');
function assert(condition, message) { if (!condition) throw new Error(message); }
function tagVersion(tag) {
  assert(/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(tag || ''), 'Expected a version tag vX.Y.Z or vX.Y.Z-beta.N');
  return tag.slice(1);
}

// Tauri uses base64-encoded minisign text. Verify both the artifact and trusted comment.
export function verifySignature(data, signature, publicKey) {
  const keyLines = Buffer.from(publicKey.trim(), 'base64').toString('utf8').trim().split(/\r?\n/);
  const lines = Buffer.from(signature.trim(), 'base64').toString('utf8').trim().split(/\r?\n/);
  const key = Buffer.from(keyLines[1] || '', 'base64');
  const sig = Buffer.from(lines[1] || '', 'base64');
  assert(key.length === 42 && sig.length === 74 && lines[2]?.startsWith('trusted comment: '), 'Malformed updater signature/key');
  assert(key.subarray(2, 10).equals(sig.subarray(2, 10)), 'Updater signing key ID mismatch');
  const algorithm = sig.subarray(0, 2).toString();
  assert(algorithm === 'ED' || algorithm === 'Ed', 'Unsupported minisign algorithm');
  const publicObject = createPublicKey({
    key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), key.subarray(10)]),
    format: 'der', type: 'spki',
  });
  const message = algorithm === 'ED' ? createHash('blake2b512').update(data).digest() : data;
  assert(verify(null, message, publicObject, sig.subarray(10)), 'Invalid updater artifact signature');
  assert(verify(null, Buffer.concat([sig.subarray(10), Buffer.from(lines[2].slice(17))]), publicObject, Buffer.from(lines[3] || '', 'base64')), 'Invalid trusted-comment signature');
}

function configuredPublicKey() {
  return readJson('src-tauri/tauri.release.conf.json').plugins.updater.pubkey;
}

function releaseNotes(version) {
  const sections = readFileSync('CHANGELOG.md', 'utf8').split(/^## /m);
  const section = sections.find(text => text.startsWith(`[${version}]`));
  assert(section, `Missing CHANGELOG.md entry for ${version}`);
  return `## ${section.trim()}`;
}

async function github(path) {
  const token = process.env.GH_TOKEN;
  assert(token, 'GH_TOKEN is required');
  const response = await fetch(`https://api.github.com/repos/${process.env.GITHUB_REPOSITORY}/${path}`, {
    headers: { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28' },
  });
  if (response.status === 404) return null;
  assert(response.ok, `GitHub request failed: ${response.status}`);
  return response.json();
}

async function requireNoRelease(tag) {
  assert(!await github(`releases/tags/${encodeURIComponent(tag)}`), `Release ${tag} already exists; refusing to overwrite it`);
}

export async function guard() {
  const tag = process.env.RELEASE_TAG;
  tagVersion(tag);
  const sha = run('git', ['rev-parse', '--verify', `refs/tags/${tag}^{commit}`]);
  run('git', ['merge-base', '--is-ancestor', sha, 'origin/main']);
  run('git', ['checkout', '--detach', sha]);
  run(process.execPath, ['scripts/version.mjs', 'check', '--tag', tag]);
  releaseNotes(tagVersion(tag));
  await requireNoRelease(tag);
  assert(process.env.GITHUB_OUTPUT, 'GITHUB_OUTPUT is required');
  appendFileSync(process.env.GITHUB_OUTPUT, `tag=${tag}\nsha=${sha}\n`);
  console.log(`Validated ${tag} at ${sha} on main`);
}

export function stage() {
  const platform = process.env.RELEASE_PLATFORM;
  const tag = process.env.RELEASE_TAG;
  const version = tagVersion(tag);
  assert(platforms.includes(platform), 'Unknown release platform');
  assert(readJson('package.json').version === version, 'Tag/version mismatch');
  const paths = JSON.parse(process.env.ARTIFACT_PATHS || '[]');
  assert(Array.isArray(paths) && paths.every(p => typeof p === 'string'), 'Invalid Tauri artifactPaths');
  const files = paths.filter(path => existsSync(path) && statSync(path).isFile() && /\.(exe|dmg|gz|sig)$/.test(path));
  const isWindows = platform.startsWith('windows');
  const updater = files.filter(path => isWindows ? path.endsWith('-setup.exe') : path.endsWith('.app.tar.gz'));
  assert(updater.length === 1, 'Expected exactly one updater bundle');
  const source = updater[0];
  const signature = readFileSync(`${source}.sig`, 'utf8').trim();
  verifySignature(readFileSync(source), signature, configuredPublicKey());
  if (!files.includes(`${source}.sig`)) files.push(`${source}.sig`);
  assert(isWindows || files.some(path => path.endsWith('.dmg')), 'Missing Mac DMG installer');
  mkdirSync('release-assets', { recursive: true });
  const records = [];
  for (const path of [...new Set(files)]) {
    const name = `${platform}-${assetName(basename(path))}`;
    assert(!existsSync(`release-assets/${name}`), `Duplicate artifact: ${name}`);
    copyFileSync(path, `release-assets/${name}`);
    records.push({ name, sha256: sha256(path) });
  }
  writeFileSync(`release-assets/${platform}.json`, `${JSON.stringify({ platform, version, tag, updater: `${platform}-${assetName(basename(source))}`, signature, files: records }, null, 2)}\n`);
  console.log(`Staged ${platform}: ${records.length} signed release files`);
}

export function assemble(directory, tag, repository, publicKey) {
  const version = tagVersion(tag);
  assert(/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository || ''), 'Invalid repository');
  const updates = {};
  const files = [];
  for (const platform of platforms) {
    const record = readJson(`${directory}/${platform}.json`);
    assert(record.platform === platform && record.version === version && record.tag === tag, `Mismatched ${platform} metadata`);
    assert(Array.isArray(record.files) && record.files.length > 0, `Missing ${platform} files`);
    const ownFiles = [];
    for (const entry of record.files) {
      assert(typeof entry.name === 'string' && /^[A-Za-z0-9._-]+$/.test(entry.name) && basename(entry.name) === entry.name && entry.name.startsWith(`${platform}-`), 'Unsafe artifact name');
      const path = `${directory}/${entry.name}`;
      assert(statSync(path).isFile() && sha256(path) === entry.sha256, `Artifact checksum mismatch: ${entry.name}`);
      assert(!files.includes(entry.name), 'Duplicate artifact');
      files.push(entry.name);
      ownFiles.push(entry.name);
    }
    assert(ownFiles.includes(record.updater) && ownFiles.includes(`${record.updater}.sig`), `Missing signed ${platform} updater`);
    assert(platform.startsWith('windows') ? record.updater.endsWith('-setup.exe') : record.updater.endsWith('.app.tar.gz') && ownFiles.some(name => name.endsWith('.dmg')), 'Incorrect bundle type');
    assert(readFileSync(`${directory}/${record.updater}.sig`, 'utf8').trim() === record.signature, 'Signature metadata mismatch');
    verifySignature(readFileSync(`${directory}/${record.updater}`), record.signature, publicKey);
    updates[platform] = { signature: record.signature, url: `https://github.com/${repository}/releases/download/${tag}/${encodeURIComponent(record.updater)}` };
  }
  // Fail rather than silently publishing unexpected or stale artifacts.
  const expected = new Set([...files, ...platforms.map(p => `${p}.json`)]);
  assert(readdirSync(directory).every(name => expected.has(name)), 'Unexpected release assets');
  const notes = releaseNotes(version);
  writeFileSync(`${directory}/latest.json`, `${JSON.stringify({ version, notes, pub_date: new Date().toISOString(), platforms: updates }, null, 2)}\n`);
  const assets = [...files, 'latest.json'].sort();
  writeFileSync(`${directory}/SHA256SUMS.txt`, assets.map(name => `${sha256(`${directory}/${name}`)}  ${name}`).join('\n') + '\n');
  return [...assets, 'SHA256SUMS.txt'];
}

export async function draft() {
  const tag = process.env.RELEASE_TAG;
  run(process.execPath, ['scripts/version.mjs', 'check', '--tag', tag]);
  assert(run('git', ['rev-parse', 'HEAD']) === process.env.RELEASE_SHA, 'Release commit mismatch');
  await requireNoRelease(tag);
  const assets = assemble('release-assets', tag, process.env.GITHUB_REPOSITORY, configuredPublicKey());
  const notes = `${releaseNotes(tagVersion(tag))}\n\nBuild commit: ${process.env.RELEASE_SHA}\n\nUpdater packages are signed. Windows Authenticode and Apple Developer signing/notarization are NOT configured; OS security warnings may occur. Mac hardware behavior remains unverified.\n`;
  writeFileSync('release-notes.txt', notes);
  const args = ['release', 'create', tag, '--verify-tag', '--draft', '--title', `QZ Flow ${tag}`, '--notes-file', 'release-notes.txt'];
  if (tag.includes('-')) args.push('--prerelease');
  args.push(...assets.map(name => `release-assets/${name}`));
  console.log(run('gh', args));
  const release = await github(`releases/tags/${encodeURIComponent(tag)}`);
  assert(release?.draft, 'Expected an unpublished draft release');
  assert(release.assets.length === assets.length && assets.every(name => release.assets.some(asset => asset.name === name && asset.size === statSync(`release-assets/${name}`).size)), 'Uploaded release assets differ from validated files; keep this draft unpublished');
  console.log('Draft only: inspect artifacts and checksums before publishing.');
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const command = { guard, stage, draft }[process.argv[2]];
    assert(command, 'Usage: node scripts/release.mjs guard|stage|draft');
    await command();
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
