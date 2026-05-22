<#
.SYNOPSIS
  AI development workflow - Start a new session branch
.DESCRIPTION
  Creates/switches to an ai/<description> branch from master.
  Run at the start of each AI development session.

.PARAMETER Description
  Short branch description (alphanumeric, dots, hyphens, underscores).
  Example: "add-model-sort", "fix-provider-crash"

.EXAMPLE
  .\scripts\session-start.ps1 -Description "add-model-sort"
#>

param(
  [Parameter(Mandatory = $true)]
  [ValidatePattern('^[a-zA-Z0-9._-]+$')]
  [string]$Description
)

# Check worktree is clean
$status = git status --porcelain
if ($status) {
  Write-Host 'ERROR: Worktree has uncommitted changes:' -ForegroundColor Red
  git status --short
  exit 1
}

$branch = "ai/$Description"

# Check if branch already exists
$existing = git branch --list $branch
if ($existing) {
  Write-Host "Branch '$branch' exists, switching to it." -ForegroundColor Yellow
  git checkout $branch
  exit 0
}

# Create branch from master (from origin/master if reachable, else local)
Write-Host "Creating branch '$branch' from master..." -ForegroundColor Cyan

git fetch origin master 2>$null | Out-Null
if ($LASTEXITCODE -eq 0) {
  git checkout -b $branch origin/master 2>$null | Out-Null
  if ($LASTEXITCODE -eq 0) {
    Write-Host "Switched to new branch: $branch (from origin/master)" -ForegroundColor Green
    git log --oneline -3
    exit 0
  }
}

# Fallback: from local master
Write-Host "Using local master to create branch..." -ForegroundColor Yellow
git checkout master
git checkout -b $branch

Write-Host "Switched to new branch: $branch" -ForegroundColor Green
git log --oneline -3
