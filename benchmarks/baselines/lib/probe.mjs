// The plan §11 environment record: commit, hardware, OS and filesystem, build.
//
// Per-OS, because there is no portable way to ask and a portable guess is
// exactly the metadata lie the schema refuses to publish. This mirrors
// `crates/mesh-bench/src/env/**` so a baseline row and a Mesh row describe the
// same host in the same words; `verify.mjs` checks that agreement rather than
// assuming it.

import { realpathSync, readFileSync, existsSync } from 'node:fs';
import { arch, platform } from 'node:os';
import { attempt, output } from './exec.mjs';

/** Host CPU, cores and memory. Throws on a host this build cannot interrogate. */
export function probeHardware() {
  if (platform() === 'darwin') {
    return {
      cpu_model: output('sysctl', ['-n', 'machdep.cpu.brand_string']),
      physical_cores: Number(output('sysctl', ['-n', 'hw.physicalcpu'])),
      logical_cores: Number(output('sysctl', ['-n', 'hw.logicalcpu'])),
      memory_bytes: Number(output('sysctl', ['-n', 'hw.memsize'])),
    };
  }
  if (platform() === 'linux') {
    const cpuinfo = readFileSync('/proc/cpuinfo', 'utf8');
    const meminfo = readFileSync('/proc/meminfo', 'utf8');
    const logical = cpuinfo.split('\n').filter((line) => line.startsWith('processor')).length;
    const perSocket = Number(field(cpuinfo, 'cpu cores') ?? logical) || logical;
    const siblings = Number(field(cpuinfo, 'siblings') ?? logical) || logical;
    const threadsPerCore = Math.max(Math.floor(siblings / Math.max(perSocket, 1)), 1);
    return {
      cpu_model: field(cpuinfo, 'model name') ?? field(cpuinfo, 'Model') ?? '',
      physical_cores: Math.max(Math.floor(logical / threadsPerCore), 1),
      logical_cores: logical,
      memory_bytes: Number(String(field(meminfo, 'MemTotal')).replace(' kB', '')) * 1024,
    };
  }
  throw new Error(`probe: unsupported host \`${platform()}\` — no hardware record, no row`);
}

/** OS name, version, architecture, and the filesystem backing `dataPath`. */
export function probePlatform(dataPath) {
  return {
    os: platform() === 'darwin' ? 'macos' : platform(),
    os_version: osVersion(),
    arch: arch() === 'arm64' ? 'aarch64' : arch(),
    filesystem: probeFilesystem(dataPath),
  };
}

/** The filesystem type backing `path`, by longest-prefix match on the mount table. */
export function probeFilesystem(path) {
  const absolute = realpathSync(path);
  const table = mountTable();
  const covering = table
    .filter((mount) => covers(mount.mountPath, absolute))
    .sort((left, right) => right.mountPath.length - left.mountPath.length);
  if (covering.length === 0) {
    throw new Error(`probe: no mount point covers ${absolute} — platform.filesystem is unknowable`);
  }
  return covering[0].filesystem;
}

/** Remote, commit and dirtiness of the checkout at `repoRoot`. */
export function probeRepository(repoRoot, remoteName = 'origin') {
  const inside = output('git', ['-C', repoRoot, 'rev-parse', '--is-inside-work-tree']);
  if (inside !== 'true') {
    throw new Error(`probe: ${repoRoot} is not inside a git work tree`);
  }
  const commit = output('git', ['-C', repoRoot, 'rev-parse', 'HEAD']);
  if (commit.length !== 40) {
    throw new Error(`probe: \`git rev-parse HEAD\` returned \`${commit}\``);
  }
  return {
    remote: output('git', ['-C', repoRoot, 'remote', 'get-url', remoteName]),
    commit,
    dirty: output('git', ['-C', repoRoot, 'status', '--porcelain']).length > 0,
  };
}

/**
 * The `build` block for a *baseline* row.
 *
 * A baseline is a vendor binary this repository does not compile, so the four
 * Rust-shaped fields say so in words rather than carrying a borrowed value from
 * the Mesh build. The load-bearing fact — the tool and its exact version — is
 * in the row's `baseline` block, and it is pinned.
 */
export function vendorBuildProfile(tool, version) {
  return {
    profile: 'vendor-binary',
    opt_level: 'vendor-default (not chosen by this repository)',
    debug_info: 'vendor-default (not chosen by this repository)',
    rustc_version: `n/a — ${tool} ${version} is not built from this workspace`,
    target_triple: hostTriple(),
  };
}

/** The host target triple, in the spelling rustc uses. */
export function hostTriple() {
  const probed = attempt('rustc', ['-vV']);
  if (probed.code === 0) {
    const line = probed.stdout.split('\n').find((row) => row.startsWith('host: '));
    if (line) return line.slice('host: '.length).trim();
  }
  const cpu = arch() === 'arm64' ? 'aarch64' : arch();
  return platform() === 'darwin' ? `${cpu}-apple-darwin` : `${cpu}-unknown-linux-gnu`;
}

function osVersion() {
  if (platform() === 'darwin') {
    return `${output('sw_vers', ['-productVersion'])} (${output('sw_vers', ['-buildVersion'])})`;
  }
  if (platform() === 'linux') {
    return readFileSync('/proc/sys/kernel/osrelease', 'utf8').trim();
  }
  throw new Error(`probe: no OS version for \`${platform()}\``);
}

function mountTable() {
  if (existsSync('/proc/self/mounts')) {
    return readFileSync('/proc/self/mounts', 'utf8')
      .split('\n')
      .map((line) => line.split(/\s+/))
      .filter((fields) => fields.length >= 3)
      .map((fields) => ({ mountPath: unescapeOctal(fields[1]), filesystem: fields[2] }));
  }
  return output('mount', [])
    .split('\n')
    .map((line) => {
      const onIndex = line.indexOf(' on ');
      if (onIndex < 0) return null;
      const rest = line.slice(onIndex + 4);
      const open = rest.lastIndexOf(' (');
      if (open < 0) return null;
      const filesystem = rest
        .slice(open + 2)
        .replace(/\)$/, '')
        .split(',')[0]
        .trim();
      if (filesystem.length === 0) return null;
      return { mountPath: rest.slice(0, open), filesystem };
    })
    .filter(Boolean);
}

function covers(mountPath, path) {
  if (mountPath === '/') return path.startsWith('/');
  return path === mountPath || path.startsWith(`${mountPath}/`);
}

function unescapeOctal(text) {
  return text.replace(/\\([0-7]{3})/g, (_match, digits) =>
    String.fromCharCode(Number.parseInt(digits, 8)),
  );
}

function field(text, key) {
  const line = text.split('\n').find((row) => row.startsWith(key));
  if (!line) return null;
  const colon = line.indexOf(':');
  if (colon < 0) return null;
  const value = line.slice(colon + 1).trim();
  return value.length > 0 ? value : null;
}
