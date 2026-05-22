#!/usr/bin/env node
/**
 * checkpoint.mjs — AI 开发流程：暂存所有变更为 checkpoint 提交
 *
 * 自动 git add -A 并以指定前缀提交到当前分支。
 *
 * Usage:
 *   node scripts/checkpoint.mjs [options] <description>
 *
 * Options:
 *   --type <type>    提交类型前缀（默认 checkpoint）
 *                   可选: checkpoint, wip, feat, fix, refactor, chore, docs
 *   --allow-empty    工作区干净时也允许提交（默认不允许）
 *
 * Examples:
 *   node scripts/checkpoint.mjs "实现模型列表排序"
 *   node scripts/checkpoint.mjs --type feat "添加排序后端的冒泡算法"
 *   node scripts/checkpoint.mjs --type docs "更新 API 文档" --allow-empty
 */

import { execSync } from 'child_process';

function run(cmd) {
  return execSync(cmd, { encoding: 'utf8', stdio: 'pipe' });
}

function log(msg, color = '') {
  const colors = { green: '\x1B[32m', cyan: '\x1B[36m', yellow: '\x1B[33m', red: '\x1B[31m', gray: '\x1B[90m' };
  const reset = '\x1B[0m';
  console.log(`${colors[color] || ''}${msg}${reset}`);
}

// ── Parse args ──────────────────────────────────────────────
const args = process.argv.slice(2);
let type = 'checkpoint';
let allowEmpty = false;
let description = '';

for (let i = 0; i < args.length; i++) {
  if (args[i] === '--type' && i + 1 < args.length) {
    type = args[++i];
  } else if (args[i] === '--allow-empty') {
    allowEmpty = true;
  } else {
    description = args[i];
  }
}

const validTypes = ['checkpoint', 'wip', 'feat', 'fix', 'refactor', 'chore', 'docs'];
if (!validTypes.includes(type)) {
  console.error(`ERROR: Invalid type "${type}". Valid: ${validTypes.join(', ')}`);
  process.exit(1);
}

if (!description) {
  console.error('Usage: node scripts/checkpoint.mjs [--type <type>] [--allow-empty] <description>');
  process.exit(1);
}

// ── Check for changes ──────────────────────────────────────
const status = run('git status --porcelain');
if (!status.trim()) {
  if (allowEmpty) {
    log('Worktree clean, but --allow-empty set. Proceeding...', 'yellow');
  } else {
    log('✔ Worktree clean, nothing to checkpoint.', 'green');
    process.exit(0);
  }
}

// ── Stage everything ────────────────────────────────────────
run('git add -A');
log('✔ Staged all changes.', 'cyan');

// ── Commit ──────────────────────────────────────────────────
const subject = `${type}: ${description}`;
try {
  const hash = run(`git commit --no-verify -m "${subject.replace(/"/g, '\\"')}"`).trim();
  log(`✔ Checkpoint committed`, 'green');
  const shortHash = run('git rev-parse --short HEAD').trim();
  log(`  ${shortHash} ${subject}`, 'gray');
  run('git log --oneline -3');
} catch (e) {
  const out = e.stderr || e.message;
  if (out.includes('nothing to commit') || out.includes('nothing added')) {
    if (allowEmpty) {
      run(`git commit --no-verify --allow-empty -m "${subject.replace(/"/g, '\\"')}"`);
      const shortHash = run('git rev-parse --short HEAD').trim();
      log(`✔ Empty checkpoint committed: ${shortHash}`, 'green');
      log(`  ${shortHash} ${subject}`, 'gray');
    } else {
      log('✔ No changes to checkpoint.', 'green');
    }
  } else {
    log(`✖ Commit failed: ${out}`, 'red');
    process.exit(1);
  }
}
