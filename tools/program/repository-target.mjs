#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const canonical = 'idosams/Mesh';
export function repositoryIdentity(url) {
  const match = url.match(/^(?:https:\/\/github\.com\/|ssh:\/\/git@github\.com\/|git@github\.com:)([^/]+\/[^/]+?)(?:\.git)?\/?$/);
  return match?.[1]?.toLowerCase() ?? null;
}

// A preflight, not authorization or a replacement for GitHub's review/check requirements.
// This command performs no fetch, edit, push, PR creation, approval, or merge.
export function checkRepository({ cwd = process.cwd(), action, base, pr, live } = {}) {
  const fail = message => { throw new Error(`Repository target refused: ${message}`); };
  if (!['edit', 'push', 'pr', 'merge'].includes(action)) fail('choose --action edit, push, pr, or merge');
  if (!base || base.startsWith('-')) fail('an explicit --base branch is required');
  const git = (...args) => execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
  try { git('check-ref-format', `refs/heads/${base}`); } catch { fail('invalid base branch'); }
  for (const mode of [[], ['--push']]) {
    const urls = git('remote', 'get-url', ...mode, '--all', 'origin').split('\n');
    if (!urls.length || urls.some(url => repositoryIdentity(url) !== canonical.toLowerCase())) {
      fail(`every origin fetch and push URL must identify ${canonical}; preserve this checkout and resolve the mismatch`);
    }
  }
  const branch = git('symbolic-ref', '--quiet', '--short', 'HEAD');
  if (branch === base || branch === 'main') fail('use a separate review branch, not the delivery base');
  const head = git('rev-parse', '--verify', 'HEAD^{commit}');
  const baseRef = `refs/remotes/origin/${base}`;
  const baseOid = git('rev-parse', '--verify', `${baseRef}^{commit}`);
  try {
    git('merge-base', '--is-ancestor', 'refs/remotes/origin/main', baseRef);
    git('merge-base', '--is-ancestor', baseRef, 'HEAD');
  } catch { fail('the intended base must descend from canonical main and be an ancestor of HEAD; reconcile without merging unrelated histories'); }
  if (action !== 'edit') {
    if (git('status', '--porcelain')) fail('record or preserve local changes before delivery');
    const lookup = live ?? (() => {
      const gh = (...args) => JSON.parse(execFileSync('gh', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }));
      const repository = gh('api', '--hostname', 'github.com', `repos/${canonical}`);
      const remoteOid = ref => {
        const lines = git('ls-remote', '--exit-code', 'origin', `refs/heads/${ref}`).split('\n');
        if (lines.length !== 1) fail('remote branch identity is ambiguous');
        return lines[0].split('\t')[0];
      };
      return {
        name: repository.full_name, defaultBranch: repository.default_branch,
        baseOid: remoteOid(base), headOid: action === 'push' ? null : remoteOid(branch),
        pull: action === 'merge' ? gh('api', '--hostname', 'github.com', `repos/${canonical}/pulls/${pr}`) : null,
      };
    });
    if (action === 'merge' && !/^[1-9]\d*$/.test(String(pr ?? ''))) fail('merge preflight requires --pr NUMBER');
    const observed = lookup({ action, base, branch, pr });
    if (observed.name !== canonical || observed.defaultBranch !== 'main') fail('live GitHub repository identity or default branch differs');
    if (observed.baseOid !== baseOid) fail('remote base moved; fetch it and reconcile before continuing');
    if (action !== 'push' && observed.headOid !== head) fail('published branch does not match the local revision');
    if (action === 'merge') {
      const pull = observed.pull;
      if (pull?.base?.repo?.full_name !== canonical || pull.base.ref !== base
          || pull.head?.repo?.full_name !== canonical || pull.head.ref !== branch
          || pull.head.sha !== head || pull.state !== 'open' || pull.draft) {
        fail('PR repository, base, head, or reviewable state differs');
      }
    }
  }
  return { repository: canonical, action, branch, head, base, baseOid, live: action !== 'edit' };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const options = {};
    for (let i = 2; i < process.argv.length; i += 2) {
      const key = process.argv[i];
      if (!['--action', '--base', '--pr'].includes(key) || !process.argv[i + 1] || options[key.slice(2)] !== undefined) throw new Error('Use --action ACTION --base BRANCH [--pr NUMBER] exactly once');
      options[key.slice(2)] = process.argv[i + 1];
    }
    console.log(JSON.stringify(checkRepository(options)));
  } catch (error) {
    // Do not echo remote URLs, credentials, command stderr, or private GitHub response content.
    console.error(error.message?.startsWith('Repository target refused:') ? error.message : 'Repository target could not be verified. Check identity, branch refs, and GitHub access; stop the requested action.');
    process.exitCode = 1;
  }
}
