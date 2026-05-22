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

function run(cmd, opts = {}) {
  return execSync(cmd, { encoding: 'utf8', stdio: opts.silent ? 'pipe' : 'pipe', ...opts });
}

function log(msg, color = '') {
  const colors = { green: '\x1B[32m', cyan: '\x1B[36m', yellow: '\x1B[33m', red: '\x1B[31m', gray: '\x1B[90m' };
  const reset = '\x1B[0m';
  console.log(`${colors[color] || ''}${msg}${reset}`);
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
try {
  run(`git rev-parse --verify "${branch}"`, { stdio: 'ignore' });
  run(`git checkout "${branch}"`);
  log(`✔ Switched to existing branch: ${branch}`, 'green');
  run(`git log --oneline -3`);
  process.exit(0);
} catch { /* branch doesn't exist, continue */ }

// ── Create from master ─────────────────────────────────────
log(`Creating branch '${branch}' from master...`, 'cyan');

// Try to fetch (best-effort)
try { run('git fetch origin master 2>/dev/null', { stdio: 'pipe' }); } catch { /* offline */ }

run('git checkout master 2>/dev/null');
run(`git checkout -b "${branch}"`);

log(`✔ Switched to new branch: ${branch}`, 'green');
run('git log --oneline -3');
