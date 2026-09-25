#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { dirname, isAbsolute, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

// This audit checks local Markdown destinations and literal npm script examples.
// It neither fetches remote links nor executes documentation commands. Heading
// fragments and generated HTML guides require separate rendered verification.
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const normalizeReference = (value) => value.trim().replace(/\s+/g, ' ').toLowerCase();

export function auditDocument(repository, file, source) {
  const errors = [];
  const lines = source.split('\n');
  const prose = [];
  const references = new Map();
  let fence;
  const report = (line, message) => errors.push(`${file}:${line}: ${message}`);
  const destination = (raw, line) => {
    const value = raw.replace(/^<|>$/g, '');
    if (/^(?:[a-z][a-z\d+.-]*:|\/\/|#)/i.test(value)) return;
    let path;
    try { path = decodeURIComponent(value.split(/[?#]/, 1)[0]); }
    catch { report(line, `invalid URL encoding: ${value}`); return; }
    if (!path) return;
    const target = resolve(dirname(resolve(repository, file)), path);
    const fromRoot = relative(repository, target);
    if (isAbsolute(path) || fromRoot === '..' || fromRoot.startsWith(`..${sep}`)) {
      report(line, `local link escapes the repository: ${value}`);
    } else if (!existsSync(target)) {
      report(line, `missing local link: ${value}`);
    }
  };
  const commands = (line, number) => {
    for (const match of line.matchAll(/\bnpm\s+(?:--prefix\s+([\w./-]+)\s+)?run\s+([\w:-]+)/g)) {
      const manifest = resolve(repository, match[1] ?? '.', 'package.json');
      const packagePath = relative(repository, manifest);
      if (packagePath.startsWith(`..${sep}`) || isAbsolute(packagePath)) {
        report(number, 'npm prefix escapes the repository');
        continue;
      }
      try {
        const pkg = JSON.parse(readFileSync(manifest, 'utf8'));
        if (!Object.hasOwn(pkg.scripts ?? {}, match[2])) {
          report(number, `unknown npm script ${match[2]} in ${packagePath}`);
        }
      } catch {
        report(number, `cannot read npm manifest ${packagePath}`);
      }
    }
  };
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    const marker = line.match(/^\s{0,3}(`{3,}|~{3,})(.*)$/);
    if (marker) {
      if (!fence) fence = { char: marker[1][0], length: marker[1].length, language: marker[2].trim() };
      else if (marker[1][0] === fence.char && marker[1].length >= fence.length && !marker[2].trim()) fence = undefined;
      continue;
    }
    if (fence) {
      if (/^(?:bash|sh|shell|console)$/.test(fence.language)) commands(line, i + 1);
      continue;
    }
    for (const code of line.matchAll(/`([^`]+)`/g)) commands(code[1], i + 1);
    const text = line.replace(/`+[^`]*`+/g, '');
    const definition = text.match(/^\s{0,3}\[([^\]]+)\]:\s*(<[^>]+>|\S+)/);
    if (definition) {
      references.set(normalizeReference(definition[1]), definition[2]);
      destination(definition[2], i + 1);
    } else prose.push([text, i + 1]);
  }
  for (const [text, line] of prose) {
    // One nested parenthesis level covers ordinary repository paths. Angle
    // destinations support spaces; optional link titles are not part of a path.
    for (const match of text.matchAll(/!?\[[^\]\n]*\]\(\s*(<[^>]+>|(?:[^\s()]|\([^()]*\))+)(?:\s+["'][^"']*["'])?\s*\)/g)) destination(match[1], line);
    for (const match of text.matchAll(/!?\[([^\]\n]+)\]\[([^\]\n]*)\]/g)) {
      const key = normalizeReference(match[2] || match[1]);
      if (!references.has(key)) report(line, `undefined link reference: ${key}`);
    }
  }
  return errors;
}

export function auditRepository(repository) {
  const files = execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard'], { cwd: repository, encoding: 'utf8' }).split('\0');
  const documents = [...new Set(files)].filter((file) => file.endsWith('.md') && existsSync(resolve(repository, file)) && statSync(resolve(repository, file)).isFile()).sort();
  return {
    documents: documents.length,
    errors: documents.flatMap((file) => auditDocument(repository, file, readFileSync(resolve(repository, file), 'utf8'))),
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const result = auditRepository(root);
  if (result.errors.length) {
    console.error(result.errors.join('\n'));
    process.exitCode = 1;
  } else console.log(`docs-check: PASS (${result.documents} Markdown documents; local links and literal npm scripts)`);
}
