# clean-target.ps1 —— 编译前执行，删除 target 里的旧产物（保留当前在用的，不会触发全量重编）
#   .\clean-target.ps1          实际删除
#   .\clean-target.ps1 -DryRun  只列出将要删除的内容
#
# 规则：Cargo 每个编译单元放在 target/<profile>/build/<包名>/<哈希>/。
# 改依赖、改 feature、升级 rustc 后会生成新哈希的目录，旧的不会自动删除。
# 本脚本把"同一个包、同一种目标、同一份源码、同样的 feature 和 profile"的多个目录视为同一单元的新旧版本，
# 只保留最近编译过的那个；增量编译缓存每个 crate 只保留最近 3 个（build / test / check 各一）。
# 另外通过 cargo metadata 找出已经不存在的包（项目改名、删掉的依赖），把它们的产物、增量缓存和 exe/pdb 全部删除。
# 万一误删，下次编译时 Cargo 只会重新编译那一个单元，不会出错。
# 注意：不要在 cargo 正在编译时运行。
param([switch]$DryRun)
$ErrorActionPreference = 'Stop'

$target = Join-Path $PSScriptRoot 'target'
if (-not (Test-Path -LiteralPath $target)) { Write-Host 'target 目录不存在，无需清理。'; exit 0 }

# 当前依赖图里的所有包名、本项目自己的 crate 名（cargo metadata 不会编译，约 1 秒）
$allPkgs = $null; $myCrates = $null
try {
    Push-Location $PSScriptRoot
    $meta = cargo metadata --format-version 1 --offline 2>$null | ConvertFrom-Json
    if ($LASTEXITCODE -eq 0 -and $meta) {
        $allPkgs = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
        $myCrates = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
        foreach ($p in $meta.packages) { [void]$allPkgs.Add($p.name) }
        foreach ($p in ($meta.packages | Where-Object { $meta.workspace_members -contains $_.id })) {
            foreach ($t in $p.targets) { [void]$myCrates.Add($t.name); [void]$myCrates.Add($t.name.Replace('-', '_')) }
        }
    }
} catch { } finally { Pop-Location }
if (-not $allPkgs) { Write-Host 'cargo metadata 失败，跳过"已不存在的包"检查。' -ForegroundColor Yellow }

function Get-Newest([string]$dir) {
    $t = [datetime]::MinValue
    foreach ($sub in 'fingerprint', 'run') {
        $p = Join-Path $dir $sub
        if ([IO.Directory]::Exists($p)) {
            foreach ($f in [IO.Directory]::GetFiles($p)) { $w = [IO.File]::GetLastWriteTimeUtc($f); if ($w -gt $t) { $t = $w } }
        }
    }
    if ($t -eq [datetime]::MinValue) { $t = [IO.Directory]::GetLastWriteTimeUtc($dir) }
    $t
}
function Get-Size([string]$path) {
    if ([IO.File]::Exists($path)) { return ([IO.FileInfo]$path).Length }
    $s = [long]0
    foreach ($f in [IO.Directory]::EnumerateFiles($path, '*', 'AllDirectories')) { $s += ([IO.FileInfo]$f).Length }
    $s
}

$victims = New-Object System.Collections.Generic.List[string]

# profile 根目录：target/<profile> 以及交叉编译时的 target/<triple>/<profile>
$profiles = @(Get-ChildItem -LiteralPath $target -Directory) + @(Get-ChildItem -LiteralPath $target -Directory | Get-ChildItem -Directory -ErrorAction SilentlyContinue)
foreach ($prof in $profiles) {
    $buildDir = Join-Path $prof.FullName 'build'
    if (-not [IO.Directory]::Exists($buildDir)) { continue }

    # ---------- 已不存在的本项目产物：target/<profile>/旧名字.exe / .pdb / .d ----------
    if ($myCrates) {
        foreach ($f in [IO.Directory]::GetFiles($prof.FullName)) {
            if ($f -match '\.(exe|pdb|d)$' -and -not $myCrates.Contains([IO.Path]::GetFileNameWithoutExtension($f))) { $victims.Add($f) }
        }
    }

    # ---------- 编译单元 ----------
    foreach ($pkgDir in [IO.Directory]::GetDirectories($buildDir)) {
        if ($allPkgs -and -not $allPkgs.Contains([IO.Path]::GetFileName($pkgDir))) { $victims.Add($pkgDir); continue }   # 包已不在依赖图里
        $units = foreach ($h in [IO.Directory]::GetDirectories($pkgDir)) {
            if ([IO.Path]::GetFileName($h) -notmatch '^[0-9a-f]{16}$') { continue }
            $fpDir = Join-Path $h 'fingerprint'
            $json = if ([IO.Directory]::Exists($fpDir)) { [IO.Directory]::GetFiles($fpDir, '*.json') | Select-Object -First 1 }
            if (-not $json) { continue }   # 不认识的目录不动
            $kind = [IO.Path]::GetFileNameWithoutExtension($json)
            $fp = Get-Content -LiteralPath $json -Raw | ConvertFrom-Json
            $fpFile = Join-Path $fpDir $kind
            $u = [pscustomobject]@{
                Dir = $h; Kind = $kind; Time = (Get-Newest $h); Key = $null
                Fingerprint = if ([IO.File]::Exists($fpFile)) { ([IO.File]::ReadAllText($fpFile)).Trim() } else { '' }
                Parent = $null
            }
            if ($kind.StartsWith('run-')) {
                # 构建脚本的"运行"单元：归属于它依赖的那个构建脚本编译单元
                $d = @($fp.deps)[0]
                if ($d) { $u.Parent = '{0:x16}' -f [uint64]$d[3] }
                $u.Key = "$kind|$($u.Parent)"
            } else {
                $out = Join-Path $h 'out'
                $files = if ([IO.Directory]::Exists($out)) { [IO.Directory]::GetFiles($out) } else { @() }
                $full = [bool]($files | Where-Object { $_ -match '\.(rlib|exe|dll|lib|so|dylib)$' })
                $meta = [bool]($files | Where-Object { $_ -match '\.rmeta$' })
                $u.Key = "$kind|$($fp.target)|$($fp.path)|$($fp.features)|$($fp.profile)|$($fp.compile_kind)|$($meta -and -not $full)"
            }
            $u
        }
        $units = @($units)
        if ($units.Count -lt 2) { continue }

        $dead = @{}
        foreach ($g in ($units | Where-Object { -not $_.Kind.StartsWith('run-') } | Group-Object Key)) {
            $g.Group | Sort-Object Time -Descending | Select-Object -Skip 1 | ForEach-Object { $dead[$_.Dir] = $_ }
        }
        $aliveFp = @($units | Where-Object { -not $_.Kind.StartsWith('run-') -and -not $dead.ContainsKey($_.Dir) } | ForEach-Object Fingerprint)
        $deadFp = @($dead.Values | ForEach-Object Fingerprint)
        $runs = @($units | Where-Object { $_.Kind.StartsWith('run-') })
        foreach ($g in ($runs | Group-Object Key)) {
            $sorted = @($g.Group | Sort-Object Time -Descending)
            for ($i = 0; $i -lt $sorted.Count; $i++) {
                $r = $sorted[$i]
                $orphan = $r.Parent -and ($deadFp -contains $r.Parent) -and -not ($aliveFp -contains $r.Parent)
                if ($i -gt 0 -or $orphan) { $dead[$r.Dir] = $r }
            }
        }
        foreach ($d in $dead.Keys) { $victims.Add($d) }
    }

    # ---------- 增量编译缓存：已不存在的 crate 全删，其余每个 crate 保留最近 3 个 ----------
    $incr = Join-Path $prof.FullName 'incremental'
    if ([IO.Directory]::Exists($incr)) {
        Get-ChildItem -LiteralPath $incr -Directory |
            Group-Object { $_.Name.Substring(0, [Math]::Max(0, $_.Name.LastIndexOf('-'))) } |
            ForEach-Object {
                $keep = if ($myCrates -and -not $myCrates.Contains($_.Name)) { 0 } else { 3 }
                $_.Group | Sort-Object LastWriteTimeUtc -Descending | Select-Object -Skip $keep | ForEach-Object { $victims.Add($_.FullName) }
            }
    }
}

# ---------- 删除 ----------
$total = [long]0; $n = 0
foreach ($v in $victims) {
    $size = Get-Size $v
    $rel = $v.Substring($target.Length + 1)
    if ($DryRun) { Write-Host ('将删除 {0,8:N1} MB  {1}' -f ($size / 1MB), $rel) -ForegroundColor Yellow }
    else {
        try { Remove-Item -LiteralPath $v -Recurse -Force } catch { Write-Host "无法删除（可能被占用）：$rel" -ForegroundColor Yellow; continue }
    }
    $total += $size; $n++
}
if ($n -eq 0) { Write-Host '没有旧产物需要清理。' -ForegroundColor Green }
elseif ($DryRun) { Write-Host ('（演练）可清理 {0} 个旧目录，共 {1:N1} MB' -f $n, ($total / 1MB)) -ForegroundColor Cyan }
else { Write-Host ('已清理 {0} 个旧目录，释放 {1:N1} MB' -f $n, ($total / 1MB)) -ForegroundColor Cyan }
