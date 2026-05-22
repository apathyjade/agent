<#
.SYNOPSIS
  AI 开发流程 — 暂存当前所有变更为 checkpoint 提交
.DESCRIPTION
  在 branch-per-session 工作流中，每次 AI 完成修改后调用此脚本。
  自动 stage 所有变更并以 "checkpoint:" 前缀提交到当前分支。

.PARAMETER Description
  必填。本次 checkpoint 的描述，概括 AI 本轮完成的内容。
.PARAMETER Type
  可选。提交类型前缀，默认 "checkpoint"。可用值：checkpoint, wip, feat, fix, refactor
.PARAMETER AllowEmpty
  开关。工作区干净时是否允许空提交（默认不允许）。

.EXAMPLE
  # 基本用法
  .\scripts\checkpoint.ps1 -Description "实现模型列表排序功能"

  # 指定类型
  .\scripts\checkpoint.ps1 -Type feat -Description "添加模型排序 API"

  # 允许空提交（如仅文档修改后）
  .\scripts\checkpoint.ps1 -Description "更新文档" -AllowEmpty
#>

param(
  [Parameter(Mandatory = $true)]
  [string]$Description,

  [ValidateSet('checkpoint', 'wip', 'feat', 'fix', 'refactor', 'chore', 'docs')]
  [string]$Type = 'checkpoint',

  [switch]$AllowEmpty
)

$ErrorActionPreference = 'Stop'

# ── 检查是否有变更 ──────────────────────────────────────────────
$status = git status --porcelain
if ($status -eq '') {
  if (-not $AllowEmpty) {
    Write-Host 'Worktree clean, nothing to stash.' -ForegroundColor Green
    exit 0
  }
}

# ── Stage 所有变更 ──────────────────────────────────────────────
git add -A
Write-Host 'Staged all changes.' -ForegroundColor Cyan

# ── 构建提交信息 ────────────────────────────────────────────────
$date = Get-Date -Format "yyyy-MM-dd HH:mm"
$subject = "${Type}: ${Description}"

# ── 提交 ────────────────────────────────────────────────────────
$result = git commit --no-verify -m $subject 2>&1
$exitCode = $LASTEXITCODE

if ($exitCode -eq 0) {
  $hash = git rev-parse --short HEAD
  Write-Host "Checkpoint committed: ${hash}" -ForegroundColor Green
  Write-Host "  Subject: ${subject}" -ForegroundColor Gray
  git log --oneline -3
} else {
  Write-Host "Commit failed: $result" -ForegroundColor Red
  exit $exitCode
}
