#!/usr/bin/env node
/**
 * finish-session.mjs — AI 开发流程：结束 Session 并归档合并
 *
 * 显示当前分支与 master 的差异和提交历史，并执行 squash-merge 归档。
 *
 * Usage:
 *   node scripts/finish-session.mjs            # 查看汇总，手动合并
 *   node scripts/finish-session.mjs --squash   # 直接 squash-merge 归档
 *
 * --squash 流程:
 *   1. git checkout master
 *   2. git merge --squash ai/<branch>
 *   3. git commit（打开编辑器填写正式提交信息）
 *   4. 询问是否删除 session 分支
 */

import { execSync } from 'child_process';
import { createInterface } from 'readline';

function run(cmd, opts = {}) {
  return execSync(cmd, { encoding: 'utf8', stdio: 'pipe', ...opts });
}

function log(msg, color = '') {
  const colors = { green: '\x1B[32m', cyan: '\x1B[36m', yellow: '\x1B[33m', red: '\x1B[31m', gray: '\x1B[90m', white: '\x1B[37m' };
  const reset = '\x1B[0m';
  console.log(`${colors[color] || ''}${msg}${reset}`);
}

function ask(query) {
  return new Promise(resolve => {
    const rl = createInterface({ input: process.stdin, output: process.stdout });
    rl.question(query, answer => { rl.close(); resolve(answer.trim().toLowerCase()); });
  });
}

// ── Parse args ──────────────────────────────────────────────
const squash = process.argv.includes('--squash');

// ── Get branch info ─────────────────────────────────────────
const branch = run('git rev-parse --abbrev-ref HEAD').trim();
if (branch === 'master') {
  log('ERROR: Already on master branch.', 'red');
  process.exit(1);
}

log('════════════════════════════════════════════', 'cyan');
log(`  Session Branch: ${branch}`, 'white');
log('════════════════════════════════════════════', 'cyan');

// ── Diff stats ──────────────────────────────────────────────
log('\n[Change stats (diff from master)]', 'cyan');
try {
  const stats = run('git diff master --stat');
  process.stdout.write(stats || '  (binary or no changes)\n');
} catch {
  log('  (unable to compute diff)', 'gray');
}

// ── Commit history ──────────────────────────────────────────
log('\n[Commit history]', 'cyan');
try {
  const logOut = run('git log master..HEAD --oneline');
  if (logOut.trim()) {
    process.stdout.write(logOut);
  } else {
    log('  (no difference from master)', 'gray');
  }
} catch {
  log('  (unable to get history)', 'gray');
}

// ── Commit count ────────────────────────────────────────────
let count = 0;
try {
  count = parseInt(run('git rev-list --count master..HEAD').trim(), 10);
} catch { /* 0 */ }
log(`\nCommits to merge: ${count}`, count > 0 ? 'yellow' : 'gray');

// ── Squash-merge ────────────────────────────────────────────
if (squash) {
  log('\nRunning squash-merge to master...', 'yellow');

  try {
    run('git checkout master 2>/dev/null');
    run('git pull origin master 2>/dev/null', { stdio: 'pipe' });

    if (count <= 1) {
      run(`git merge --ff-only "${branch}"`);
    } else {
      run(`git merge --squash "${branch}"`);
      // Let user write the commit message in editor
      log('Please write a proper commit message in the editor...', 'cyan');
      run('git commit');
    }

    log('\n✔ Merge complete!', 'green');
    run('git log --oneline -3');

    const ans = await ask('\nDelete session branch? (y/n): ');
    if (ans === 'y' || ans === 'yes') {
      run(`git branch -D "${branch}"`);
      log(`✔ Branch '${branch}' deleted.`, 'green');
    }
  } catch (e) {
    const msg = e.stderr || e.message;
    if (msg.includes('merge failed') || msg.includes('conflict')) {
      log('✖ Merge conflict detected. Please resolve manually.', 'red');
    } else {
      log(`✖ Merge failed: ${msg}`, 'red');
    }
    process.exit(1);
  }
} else {
  // ── Recommendations ──────────────────────────────────────────
  log('\n[Recommended actions]', 'cyan');
  log('\n  1. Review all changes:\n     git diff master --stat');
  log('\n  2. Review full diff:\n     git diff master');
  log('\n  3. Squash-merge to master:\n     git checkout master && git merge --squash "' + branch + '" && git commit');
  log('\n  4. One-command archive:\n     node scripts/finish-session.mjs --squash');
}
