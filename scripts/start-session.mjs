#!/usr/bin/env node
/**
 * start-session.mjs — AI 开发流程：开始新 Session
 *
 * 从 master 创建 ai/<description> 分支并切换过去。
 * 若分支已存在则直接切换（可恢复中断的 session）。
 *
 * Usage:
 *   node scripts/start-session.mjs <description>
 *
 * Examples:
 *   node scripts/start-session.mjs add-model-sort
 *   node scripts/start-session.mjs fix-provider-crash
 */

import { execSync } from 'child_process';
import { platform } from 'os';

const NULL = platform() === 'win32' ? 'NUL' : '/dev/null';

function run(cmd) {
  return execSync(cmd, { encoding: 'utf8', stdio: 'pipe' });
}

function quiet(cmd) {
  try { return execSync(cmd, { encoding: 'utf8', stdio: 'pipe', windowsHide: true }); }
  catch { return ''; }
}

function log(msg, color = '') {
  const colors = { green: '\x1B[32m', cyan: '\x1B[36m', yellow: '\x1B[33m', red: '\x1B[31m', gray: '\x1B[90m' };
  console.log(`${colors[color] || ''}${msg}\x1B[0m`);
}

function error(msg) {
  log(`ERROR: ${msg}`, 'red');
  process.exit(1);
}

// ── Parse args ──────────────────────────────────────────────
const desc = process.argv[2];
if (!desc) {
  console.error('Usage: node scripts/start-session.mjs <description>');
  console.error('  e.g. node scripts/start-session.mjs add-model-sort');
  process.exit(1);
}
if (!/^[a-zA-Z0-9._-]+$/.test(desc)) {
  error('Description must only contain: letters, digits, dots, hyphens, underscores');
}

const branch = `ai/${desc}`;

// ── Check worktree clean ───────────────────────────────────
const status = run('git status --porcelain');
if (status.trim()) {
  log('Worktree has uncommitted changes:', 'yellow');
  process.stdout.write(status);
  error('Please commit or stash them first');
}

// ── Branch exists? Switch to it ────────────────────────────
if (quiet(`git rev-parse --verify "${branch}" 2>${NULL}`)) {
  run(`git checkout "${branch}"`);
  log(`✔ Switched to existing branch: ${branch}`, 'green');
  quiet('git log --oneline -3');
  process.exit(0);
}

// ── Create from master ─────────────────────────────────────
log(`Creating branch '${branch}' from master...`, 'cyan');

quiet(`git fetch origin master 2>${NULL}`);
quiet(`git checkout master 2>${NULL}`);
run(`git checkout -b "${branch}"`);

log(`✔ Switched to new branch: ${branch}`, 'green');
quiet('git log --oneline -3');
