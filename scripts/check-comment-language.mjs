// 检查 src-tauri 下是否残留英文代码注释。
// 用法：node scripts/check-comment-language.mjs
//
// 规则（见 AGENTS.md「输出语言」）：
//   - `//`、`///`、`//!` 注释统一使用简体中文
//   - 标识符、字符串字面量、log 消息、错误 Display 文本、IPC 事件名保持英文
//   - rustdoc 内部链接（[`Foo`]）属于代码，不翻译
//
// 判定方式：注释正文里出现 CJK 字符即视为中文注释；
// 完全不含 CJK 的注释判为英文残留（需要人工确认是否属于豁免项）。

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const ROOT = join(process.cwd(), 'src-tauri', 'src');
const CJK = /[\u4e00-\u9fff]/;

// 注释里允许出现的英文片段（术语、代码标识、路径、链接等）
const ALLOW = [
  /^\s*$/,
  /^\/\/\s*$/,
  /^\/\/!/,
  /^\/\//,
  /^\s*\/\/\s*[-=*_`~+#]*\s*$/, // 纯分隔线
  /^\s*\/\/\s*(TODO|FIXME|HACK|NOTE)\b/i,
  /^\s*\/\/\s*\/\//,
];

const isSeparator = (line) => /^\s*\/\/[/!]*\s*[-=*_`~+#\s]*$/.test(line);
const isRustdocLinkOnly = (line) => /^\s*\/\/[/!]*\s*(\[.*\]|`.*`|\{.*\})\s*$/.test(line);
const hasCode = (line) => /`[^`]+`|\[[^\]]+\]\([^)]*\)|https?:\/\//.test(line);

// 纯文件名 / 工具名 / 版本约束 / 表格行 / YAML 片段 —— 这些是标识符或数据，不是散文
const isIdentifierOnly = (line) =>
  /^[\w.\-/]+\s*$/.test(line) ||                    // .nvmrc / fnm / volta / go.mod
  /^[\w.\-]+(\s*[<>=]+\s*[\w.]+)+$/.test(line) ||   // Node >=18
  /^\|\s*[-:|\s]*\|$/.test(line) ||                 // markdown 表格分隔行
  /^\|.*\|$/.test(line) ||                          // markdown 表格行
  /^```/.test(line) ||                              // 代码块围栏
  /^[\w\-]+\s*:\s*.*$/.test(line);                  // yaml 键值

// 整行都是标识符/代码（无散文）：以反引号包裹的词、或 "x" -> "y" 形式
const isCodeOnly = (line) =>
  /^`[^`]+`(\s*->\s*`[^`]+`)?$/.test(line) ||      // `text-embedding-3-small`
  /^"[^"]*"\s*->\s*"[^"]*"$/.test(line) ||           // "go1.22.4" -> "1.22.4"
  /^\w+\s*\([^)]*\)$/.test(line);                   // @@read_file(path)

// 列表项整行都是标识符/映射：`Node 22.x → npm 10.x` / `PhaseChanged → `orchestrator_phase``
const isMappingListItem = (line) => {
  const m = line.match(/^[-*]\s+(.+?)\s*(?:->|→)\s*(.+)$/);
  if (!m) return false;
  // 两侧都只能是标识符 / 版本号 / 反引号代码，不得含散文词
  const ident = /^[`\w.*\-]+(\s+[`\w.*\-]+)*$/;
  return ident.test(m[1]) && ident.test(m[2]);
};

// 列表项整行是工具调用语法：`@@read_file(path)` / `@@search_code(pattern[, path])`
const isToolSyntaxListItem = (line) => /^[-*]\s+`@@\w+\([^)]*\)`$/.test(line);

// 版本约束与示例配置：`pnpm 9.x → Node >=18` / `version = "1.85.0"`
const isVersionConstraint = (line) =>
  /^[-*]\s+[\w.\-]+(\s+[\w.\-]+)*\s*(?:->|→)\s*[\w.\-]+(\s*[<>=]+\s*[\w.\-]+)+$/.test(line) ||
  /^\w+\s*=\s*"[^"]*"$/.test(line);

// 以中文标点收尾的行，主体已是中文（仅结尾枚举了英文阶段名）
const endsWithChinesePunct = (line) => /[，。；：、）】」]$/.test(line);

// doctest 代码行：`use agent_lib::...;` / `assert_eq!(...)`
const isDoctestCode = (line) =>
  /^use\s+[\w:]+;$/.test(line) ||
  /^assert(_eq|_ne)?!\(.*\);$/.test(line);

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    const st = statSync(p);
    if (st.isDirectory()) walk(p, out);
    else if (p.endsWith('.rs')) out.push(p);
  }
  return out;
}

const files = walk(ROOT);
const offenders = [];

for (const file of files) {
  const rel = relative(join(process.cwd(), 'src-tauri'), file);
  const lines = readFileSync(file, 'utf8').split(/\r?\n/);

  // 标记处于原始字符串（r#"..."#）内部的行 —— 那是提示词模板文本，不是注释
  const inRawString = new Array(lines.length).fill(false);
  let rawOpen = false;
  for (let i = 0; i < lines.length; i++) {
    const t = lines[i];
    if (!rawOpen && /r#"/.test(t)) { rawOpen = true; inRawString[i] = true; continue; }
    if (rawOpen) {
      inRawString[i] = true;
      if (/"#/.test(t)) rawOpen = false;
    }
  }

  lines.forEach((raw, i) => {
    if (inRawString[i]) return;
    const line = raw.trim();
    if (!line.startsWith('//')) return;
    if (isSeparator(line) || isRustdocLinkOnly(line)) return;

    // 注释里去掉首尾的分隔线装饰（仅空白与制表符 ──），保留代码字符
    const body = line
      .replace(/^\/[/!]+\s?/, '')
      .replace(/^[\s─]+/, '')
      .replace(/[\s─]+$/, '')
      .trim();
    if (body === '') return;
    if (CJK.test(body)) return;
    if (hasCode(body)) return;
    if (isIdentifierOnly(body)) return;
    if (isCodeOnly(body)) return;
    if (isMappingListItem(body)) return;
    if (isToolSyntaxListItem(body)) return;
    if (isVersionConstraint(body)) return;
    if (endsWithChinesePunct(body)) return;
    if (isDoctestCode(body)) return;

    offenders.push({ file: rel, line: i + 1, text: raw.trim().slice(0, 110) });
  });
}

if (offenders.length === 0) {
  console.log(`OK — ${files.length} 个 .rs 文件中未发现英文注释残留。`);
  process.exit(0);
}

console.log(`发现 ${offenders.length} 处疑似英文注释（${files.length} 个文件）：\n`);
const byFile = new Map();
for (const o of offenders) {
  if (!byFile.has(o.file)) byFile.set(o.file, []);
  byFile.get(o.file).push(o);
}
for (const [file, list] of [...byFile.entries()].sort((a, b) => b[1].length - a[1].length)) {
  console.log(`${file}  (${list.length})`);
  for (const o of list.slice(0, 6)) console.log(`   ${o.line}: ${o.text}`);
  if (list.length > 6) console.log(`   ... 还有 ${list.length - 6} 处`);
  console.log('');
}
process.exit(1);
