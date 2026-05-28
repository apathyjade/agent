import { useRef, useCallback, useMemo, useState } from 'react';
import Editor, { OnMount, BeforeMount } from '@monaco-editor/react';
import { Copy, Check } from 'lucide-react';

interface CodeBlockProps {
  language: string;
  value: string;
}

const LANGUAGE_MAP: Record<string, string> = {
  'js': 'javascript',
  'ts': 'typescript',
  'jsx': 'javascript',
  'tsx': 'typescript',
  'py': 'python',
  'rb': 'ruby',
  'sh': 'shell',
  'bash': 'shell',
  'zsh': 'shell',
  'yml': 'yaml',
  'yaml': 'yaml',
  'json': 'json',
  'md': 'markdown',
  'mermaid': 'markdown',
  'html': 'html',
  'css': 'css',
  'scss': 'scss',
  'less': 'less',
  'rs': 'rust',
  'rust': 'rust',
  'go': 'go',
  'java': 'java',
  'kt': 'kotlin',
  'kotlin': 'kotlin',
  'swift': 'swift',
  'c': 'c',
  'cpp': 'cpp',
  'h': 'c',
  'hpp': 'cpp',
  'cs': 'csharp',
  'csharp': 'csharp',
  'php': 'php',
  'sql': 'sql',
  'graphql': 'graphql',
  'dockerfile': 'dockerfile',
  'toml': 'ini',
  'ini': 'ini',
  'xml': 'xml',
  'svg': 'xml',
  'pl': 'perl',
  'lua': 'lua',
  'r': 'r',
  'dart': 'dart',
  'scala': 'scala',
  'tex': 'latex',
  'latex': 'latex',
};

export function CodeBlock({ language, value }: CodeBlockProps) {
  const [copied, setCopied] = useState(false);
  const editorRef = useRef<Parameters<OnMount>[0] | null>(null);
  const monacoRef = useRef<Parameters<BeforeMount>[0] | null>(null);

  const normalizedLang = LANGUAGE_MAP[language.toLowerCase()] || language;

  const lineCount = useMemo(() => value.split('\n').length, [value]);
  const editorHeight = useMemo(() => Math.max(60, Math.min(lineCount * 19 + 26, 600)), [lineCount]);

  const isDark = typeof document !== 'undefined' && document.documentElement.classList.contains('dark');

  const handleCopy = useCallback(async () => {
    await navigator.clipboard.writeText(value);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }, [value]);

  const handleBeforeMount: BeforeMount = useCallback((monaco) => {
    monacoRef.current = monaco;
    // Configure Monaco to match our dark/light theme
    monaco.editor.defineTheme('agentDark', {
      base: 'vs-dark',
      inherit: true,
      rules: [],
      colors: {
        'editor.background': '#1e1e2e',
        'editor.lineHighlightBackground': '#2a2a3e',
        'editorLineNumber.foreground': '#6b6b80',
        'editorLineNumber.activeForeground': '#a0a0b8',
        'editor.selectionBackground': '#3a3d5c',
        'editor.inactiveSelectionBackground': '#2d2f4a',
        'editorCursor.foreground': '#c0c0d0',
        'editor.wordHighlightBackground': '#3a3d5c',
        'editorLineNumber.activeBackground': '#1e1e2e',
      },
    });
    monaco.editor.defineTheme('agentLight', {
      base: 'vs',
      inherit: true,
      rules: [],
      colors: {
        'editor.background': '#fafafa',
        'editor.lineHighlightBackground': '#f0f0f5',
        'editorLineNumber.foreground': '#b0b0c0',
        'editorLineNumber.activeForeground': '#6b6b80',
        'editor.selectionBackground': '#d6d6f0',
        'editor.inactiveSelectionBackground': '#e8e8f5',
        'editorCursor.foreground': '#505060',
        'editorLineNumber.activeBackground': '#fafafa',
      },
    });
  }, []);

  const handleMount: OnMount = useCallback((editor) => {
    editorRef.current = editor;
    // Enable cmd/ctrl+click for selection, disable editing
    editor.updateOptions({ readOnly: true });
  }, []);

  return (
    <div className="relative group rounded-xl overflow-hidden border border-gray-200 dark:border-gray-700 my-3">
      <div className="flex items-center justify-between px-4 py-2 bg-gray-100 dark:bg-gray-800 border-b border-gray-200 dark:border-gray-700">
        <span className="text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wide">
          {language}
        </span>
        <button
          onClick={handleCopy}
          className="flex items-center gap-1.5 text-xs text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
        >
          {copied ? (
            <>
              <Check size={14} className="text-green-500" />
              <span className="text-green-500">已复制</span>
            </>
          ) : (
            <>
              <Copy size={14} />
              <span>复制</span>
            </>
          )}
        </button>
      </div>
      <Editor
        height={editorHeight}
        language={normalizedLang}
        value={value}
        beforeMount={handleBeforeMount}
        onMount={handleMount}
        theme={isDark ? 'agentDark' : 'agentLight'}
        loading={<div className="p-6 text-sm text-gray-400 dark:text-gray-500 text-center">加载编辑器...</div>}
        options={{
          readOnly: true,
          minimap: { enabled: false },
          scrollBeyondLastLine: false,
          fontSize: 13,
          fontFamily: "'JetBrains Mono', 'Fira Code', monospace",
          lineNumbers: 'on',
          lineNumbersMinChars: 3,
          glyphMargin: false,
          folding: true,
          lineDecorationsWidth: 8,
          renderLineHighlight: 'line',
          overviewRulerBorder: false,
          wordWrap: 'off',
          scrollbar: {
            vertical: 'hidden',
            horizontal: 'hidden',
          },
          padding: { top: 12, bottom: 12 },
          automaticLayout: true,
          tabSize: 2,
          bracketPairColorization: { enabled: true },
          guides: {
            indentation: true,
            bracketPairs: true,
          },
          smoothScrolling: true,
          cursorBlinking: 'solid',
          cursorStyle: 'line',
          selectionHighlight: true,
          occurrencesHighlight: 'singleFile',
          renderWhitespace: 'selection',
        }}
      />
    </div>
  );
}
