<#
.SYNOPSIS
  AI development workflow - Finish current session and suggest merge
.DESCRIPTION
  Displays diff stats, commit history vs master, and guides squash-merge.
  Run at the end of each AI development session.

.PARAMETER Squash
  Switch. Directly squash-merge to master (use with caution).

.EXAMPLE
  .\scripts\session-finish.ps1
  .\scripts\session-finish.ps1 -Squash
#>

param(
  [switch]$Squash
)

$ErrorActionPreference = 'Stop'

# Get current branch info
$branch = git rev-parse --abbrev-ref HEAD
if ($branch -eq 'master') {
  Write-Host 'ERROR: Already on master branch.' -ForegroundColor Red
  exit 1
}

Write-Host "================================================" -ForegroundColor Cyan
Write-Host "  Session Branch: $branch" -ForegroundColor White
Write-Host "================================================" -ForegroundColor Cyan

# Diff stats against master
Write-Host "`n[Change stats (diff from master)]" -ForegroundColor Cyan
git diff master --stat

# Commit history
Write-Host "`n[Commit history]" -ForegroundColor Cyan
$log = git log master..HEAD --oneline
if (-not $log) {
  Write-Host "  (no difference from master)" -ForegroundColor Gray
} else {
  $log
}

# Commit count
$count = git rev-list --count master..HEAD
Write-Host "`nCommits to merge: $count" -ForegroundColor Yellow

# Squash-merge
if ($Squash) {
  Write-Host "`nRunning squash-merge to master..." -ForegroundColor Yellow

  git checkout master
  git pull origin master 2>$null

  if ($count -eq 1) {
    git merge --ff-only $branch
  } else {
    git merge --squash $branch
    git commit --no-verify
  }

  if ($LASTEXITCODE -eq 0) {
    Write-Host "Merge complete!" -ForegroundColor Green
    git log --oneline -3

    Write-Host "`nDelete session branch? (y/n): " -NoNewline
    $ans = Read-Host
    if ($ans -eq 'y') {
      git branch -D $branch
      Write-Host "Branch '$branch' deleted." -ForegroundColor Green
    }
  } else {
    Write-Host "Merge conflict detected. Please resolve manually." -ForegroundColor Red
  }
} else {
  # Recommended actions
  Write-Host "`n[Recommended actions]" -ForegroundColor Cyan

  Write-Host "`n  1. Review changes:"
  Write-Host "     git diff master --stat"

  Write-Host "`n  2. Review diffs:"
  Write-Host "     git diff master"

  Write-Host "`n  3. Squash-merge to master:"
  Write-Host "     git checkout master"
  Write-Host "     git merge --squash $branch"
  Write-Host "     git commit -m 'feat: <description>'"

  Write-Host "`n  4. Delete branch (optional):"
  Write-Host "     git branch -D $branch"
}
