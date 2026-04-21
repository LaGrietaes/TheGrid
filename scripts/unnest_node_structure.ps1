# Un-nest double thegrid-workspace folder structure on node branch
# This moves files from thegrid-workspace/thegrid-workspace/* to thegrid-workspace/*

param(
    [switch]$DryRun = $false
)

$repoRoot = "S:\LAG TheGrid\TheGrid"
Set-Location $repoRoot

Write-Host "Un-nesting node branch structure..." -ForegroundColor Cyan

# Get list of files in the nested structure
$nestedFiles = git ls-tree -r node --name-only | Where-Object { $_ -match '^thegrid-workspace/thegrid-workspace/' }
$fileCount = @($nestedFiles).Count

if ($fileCount -eq 0) {
    Write-Host "✓ No nested files found. Structure is already correct." -ForegroundColor Green
    exit 0
}

Write-Host "Found $fileCount files to un-nest" -ForegroundColor Yellow

# Create mapping of moves
$moves = @()
foreach ($file in $nestedFiles) {
    $target = $file -replace '^thegrid-workspace/thegrid-workspace/', 'thegrid-workspace/'
    $moves += @{ src = $file; dst = $target }
}

if ($DryRun) {
    Write-Host "`nDRY RUN - Files that would be moved:" -ForegroundColor Cyan
    $moves | ForEach-Object { Write-Host "  $($_.src) -> $($_.dst)" }
    Write-Host "`nTo execute, run without -DryRun flag" -ForegroundColor Yellow
    exit 0
}

# Create a temporary branch for restructuring
Write-Host "Creating temporary restructuring branch..." -ForegroundColor Cyan
git checkout -b temp/restructure-node node
if ($LASTEXITCODE -ne 0) {
    Write-Host "✗ Failed to create temp branch" -ForegroundColor Red
    exit 1
}

# Use git filter-branch to restructure
Write-Host "Restructuring via git filter-branch..." -ForegroundColor Cyan

$filterScript = {
    param([string]$treeish)
    $tree = git cat-file -p $treeish
    $newTree = $tree -replace 'thegrid-workspace/thegrid-workspace/', 'thegrid-workspace/'
    echo $newTree | git mktree
}

# Alternative: use git ls-files and move logic
foreach ($move in $moves) {
    $srcPath = $move.src
    $dstPath = $move.dst
    
    # Get file content from git index
    $content = git show "HEAD:$srcPath" 2>&1
    if ($LASTEXITCODE -eq 0) {
        # Stage the file at new location
        $content | git hash-object -w --stdin > $null
        git update-index --add --cacheinfo 100644 $(git hash-object -w --stdin <<< $content) $dstPath
    }
}

# Remove old nested files from index
foreach ($move in $moves) {
    git rm --cached $move.src 2>&1 | Out-Null
}

# Commit the restructuring
git commit -m "fix(node): unnest double thegrid-workspace folder structure" 2>&1 | Select-Object -First 5

Write-Host "✓ Restructuring complete on temp/restructure-node branch" -ForegroundColor Green
Write-Host "Next steps: Review changes, then merge back to node or rebase" -ForegroundColor Cyan

# Show summary
git log --oneline temp/restructure-node -n 3
