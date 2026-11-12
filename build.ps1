param([ValidateSet('build','test','run','fmt','package')][string]$Action = 'build')
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
$localCargo = Join-Path $PSScriptRoot '.tools/cargo/bin/cargo.exe'
if (Test-Path $localCargo) {
    $env:RUSTUP_HOME = Join-Path $PSScriptRoot '.tools/rustup'
    $env:CARGO_HOME = Join-Path $PSScriptRoot '.tools/cargo'
    $cargo = $localCargo
} else { $cargo = 'cargo' }
switch ($Action) {
    'build' { & $cargo build --release --locked }
    'test' { & $cargo test --locked -- --test-threads=1 }
    'run' { & $cargo run --release --locked }
    'fmt' { & $cargo fmt }
    'package' {
        & $cargo build --release --locked
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        $releaseDir = Join-Path $PSScriptRoot 'dist'
        New-Item -ItemType Directory -Path $releaseDir -Force | Out-Null
        $built = Join-Path $PSScriptRoot 'target/release/qingjie.exe'
        $bytes = [IO.File]::ReadAllBytes($built)
        $pe = [BitConverter]::ToInt32($bytes, 60)
        if ([BitConverter]::ToUInt16($bytes, $pe + 24 + 68) -ne 2) {
            throw '发布文件必须使用 Windows GUI 子系统。'
        }
        $staged = Join-Path $releaseDir 'qingjie.exe.next'
        $destination = Join-Path $releaseDir 'qingjie.exe'
        Copy-Item -LiteralPath $built -Destination $staged -Force
        if (-not ('QingjiePackage.Native' -as [type])) {
            Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace QingjiePackage {
    public static class Native {
        [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
        public static extern bool MoveFileEx(string source, string target, int flags);
    }
}
'@
        }
        if (-not [QingjiePackage.Native]::MoveFileEx($staged, $destination, 9)) {
            $code = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            Remove-Item -LiteralPath $staged -Force
            throw "无法替换发布文件（Windows 错误 $code）。请先退出正在运行的轻截；现有版本已保留。"
        }
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'README.md') -Destination (Join-Path $releaseDir 'README.md') -Force
        # Only retire versioned application executables inside this exact output directory.
        $resolvedRelease = (Resolve-Path -LiteralPath $releaseDir).Path
        Get-ChildItem -LiteralPath $releaseDir -Filter 'qingjie-v*.exe' -File | ForEach-Object {
            if ($_.DirectoryName -ne $resolvedRelease) { throw '发布目录校验失败。' }
            Remove-Item -LiteralPath $_.FullName -Force
        }
        $version = [regex]::Match((Get-Content -LiteralPath (Join-Path $PSScriptRoot 'Cargo.toml') -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value
        $hash = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash
        @{ version=$version; sha256=$hash; file='qingjie.exe' } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $releaseDir 'release.json') -Encoding utf8
        Write-Output "已发布轻截 $version：$destination"
        Write-Output "SHA256: $hash"
    }
}
exit $LASTEXITCODE
