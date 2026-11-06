// 统计 #[tauri::command] 函数的文档覆盖率。
// 用法：node scripts/check-ipc-docs.mjs
//
// 判定：命令函数 = 带 `#[tauri::command]` 属性的 `pub fn`。
// 文档 = 该函数签名正上方（跳过空行与其它属性）连续的 `///` 行。
// 注意：文档通常写在 `#[tauri::command]` 属性**之前**，而非之后。

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const ROOT = join(process.cwd(), 'src-tauri', 'src', 'app', 'commands');

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (p.endsWith('.rs')) out.push(p);
  }
  return out;
}

let total = 0;
let documented = 0;
const missing = [];

for (const file of walk(ROOT)) {
  const rel = relative(join(process.cwd(), 'src-tauri'), file);
  const lines = readFileSync(file, 'utf8').split(/\r?\n/);

  for (let i = 0; i < lines.length; i++) {
    if (!lines[i].trim().startsWith('#[tauri::command]')) continue;
    total++;

    // 找该属性所属的函数签名：向下跳过属性与空行，遇到 fn 即止
    let j = i + 1;
    let fnLine = -1;
    while (j < lines.length) {
      const t = lines[j].trim();
      if (t === '' || t.startsWith('#[')) { j++; continue; }
      if (/^(pub\s+)?(async\s+)?fn\s+\w+/.test(t)) { fnLine = j; break; }
      break;
    }
    if (fnLine < 0) continue;

    // 从函数签名向上找连续 `///` 行（允许中间夹其它属性）
    let k = fnLine - 1;
    let found = false;
    while (k >= 0) {
      const t = lines[k].trim();
      if (t === '') { k--; continue; }
      if (t.startsWith('///')) { found = true; k--; continue; }
      if (t.startsWith('#[')) { k--; continue; }
      break;
    }

    if (found) documented++;
    else missing.push(`${rel}:${fnLine + 1}  ${lines[fnLine].trim().slice(0, 70)}`);
  }
}

console.log(`#[tauri::command] 命令：${total} 个，已文档化 ${documented} 个 (${((100 * documented) / total).toFixed(1)}%)`);
if (missing.length) {
  console.log(`\n缺少文档的 ${missing.length} 个：`);
  for (const m of missing.slice(0, 30)) console.log('  ' + m);
  if (missing.length > 30) console.log(`  ... 还有 ${missing.length - 30} 个`);
}
process.exit(missing.length === 0 ? 0 : 1);
