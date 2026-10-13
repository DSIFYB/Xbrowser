[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$missing = [System.Collections.Generic.List[string]]::new()

function Write-Check {
    param(
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] [bool] $Passed,
        [Parameter(Mandatory)] [string] $Detail
    )

    if ($Passed) {
        Write-Host "[OK]   $Name - $Detail" -ForegroundColor Green
    }
    else {
        Write-Host "[MISS] $Name - $Detail" -ForegroundColor Red
        $script:missing.Add($Name)
    }
}

Write-Host 'Axiom Browser - проверка среды сборки (только чтение)'

function Get-WorkingRustTool {
    param([Parameter(Mandatory)] [string] $ToolName)

    $candidates = [System.Collections.Generic.List[string]]::new()
    $command = Get-Command $ToolName -ErrorAction SilentlyContinue

    $toolchainRoot = Join-Path $env:USERPROFILE '.rustup\toolchains'
    if (Test-Path -LiteralPath $toolchainRoot -PathType Container) {
        $toolchains = Get-ChildItem -LiteralPath $toolchainRoot -Directory -ErrorAction SilentlyContinue |
            Sort-Object Name -Descending
        foreach ($toolchain in $toolchains) {
            $directExe = Join-Path $toolchain.FullName "bin\$ToolName.exe"
            if ((Test-Path -LiteralPath $directExe -PathType Leaf) -and -not $candidates.Contains($directExe)) {
                $candidates.Add($directExe)
            }
        }
    }
    if ($command -and -not $candidates.Contains($command.Source)) { $candidates.Add($command.Source) }

    foreach ($candidate in $candidates) {
        try {
            $versionOutput = (& $candidate --version 2>&1 | Out-String).Trim()
            if ($LASTEXITCODE -eq 0 -and $versionOutput) {
                return [pscustomobject]@{ Path = $candidate; Version = $versionOutput }
            }
        }
        catch { }
    }
    return $null
}

$cargo = Get-WorkingRustTool 'cargo'
$rustc = Get-WorkingRustTool 'rustc'
Write-Check 'Cargo' ($null -ne $cargo) $(if ($cargo) { "$($cargo.Version) ($($cargo.Path))" } else { 'не найден или не запускается; проверьте Rust toolchain' })
Write-Check 'rustc' ($null -ne $rustc) $(if ($rustc) { "$($rustc.Version) ($($rustc.Path))" } else { 'не найден или не запускается; проверьте Rust toolchain' })

$vswhereCandidates = @(
    (Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'),
    (Join-Path $env:ProgramFiles 'Microsoft Visual Studio\Installer\vswhere.exe')
) | Select-Object -Unique
$vswherePath = $vswhereCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
$vsInstall = $null
$linker = $null

if ($vswherePath) {
    $vsInstall = & $vswherePath -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null | Select-Object -First 1
    if ($vsInstall) {
        $linker = Get-ChildItem -LiteralPath (Join-Path $vsInstall 'VC\Tools\MSVC') -Filter link.exe -File -Recurse -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -match '\\bin\\Hostx64\\x64\\link\.exe$' } |
            Sort-Object FullName -Descending | Select-Object -First 1
    }
}

Write-Check 'MSVC linker' ($null -ne $linker) $(if ($linker) { $linker.FullName } elseif (-not $vswherePath) { 'vswhere.exe не найден; требуется Visual Studio Build Tools с C++ tools' } else { 'MSVC C++ tools/link.exe не найдены через vswhere' })

$sdkRoots = [System.Collections.Generic.List[string]]::new()
foreach ($view in @([Microsoft.Win32.RegistryView]::Registry64, [Microsoft.Win32.RegistryView]::Registry32)) {
    try {
        $baseKey = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine, $view)
        $key = $baseKey.OpenSubKey('SOFTWARE\Microsoft\Windows Kits\Installed Roots')
        if ($key) {
            $root = $key.GetValue('KitsRoot10')
            if ($root -and -not $sdkRoots.Contains([string]$root)) { $sdkRoots.Add([string]$root) }
            $key.Dispose()
        }
        $baseKey.Dispose()
    }
    catch { }
}

$sdkInclude = $null
foreach ($root in $sdkRoots) {
    $includePath = Join-Path $root 'Include'
    if (Test-Path -LiteralPath $includePath -PathType Container) {
        $sdkInclude = Get-ChildItem -LiteralPath $includePath -Directory -ErrorAction SilentlyContinue |
            Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'um\Windows.h') } |
            Sort-Object Name -Descending | Select-Object -First 1
        if ($sdkInclude) { break }
    }
}
Write-Check 'Windows SDK' ($null -ne $sdkInclude) $(if ($sdkInclude) { "Windows.h: $($sdkInclude.FullName)\um\Windows.h" } else { 'Windows 10/11 SDK не найден в реестре 32/64-bit или его заголовки отсутствуют' })

$webViewVersion = $null
foreach ($hive in @([Microsoft.Win32.RegistryHive]::LocalMachine, [Microsoft.Win32.RegistryHive]::CurrentUser)) {
    foreach ($view in @([Microsoft.Win32.RegistryView]::Registry64, [Microsoft.Win32.RegistryView]::Registry32)) {
        $baseKey = $null
        $clientsKey = $null
        try {
            $baseKey = [Microsoft.Win32.RegistryKey]::OpenBaseKey($hive, $view)
            $clientsKey = $baseKey.OpenSubKey('SOFTWARE\Microsoft\EdgeUpdate\Clients')
            if (-not $clientsKey) { continue }
            foreach ($subKeyName in $clientsKey.GetSubKeyNames()) {
                $clientKey = $clientsKey.OpenSubKey($subKeyName)
                if (-not $clientKey) { continue }
                $name = $clientKey.GetValue('name')
                $version = $clientKey.GetValue('pv')
                $clientKey.Dispose()
                if ($name -eq 'Microsoft Edge WebView2 Runtime' -and $version -and $version -ne '0.0.0.0') {
                    $webViewVersion = [string]$version
                    break
                }
            }
        }
        catch { }
        finally {
            if ($clientsKey) { $clientsKey.Dispose() }
            if ($baseKey) { $baseKey.Dispose() }
        }
        if ($webViewVersion) { break }
    }
    if ($webViewVersion) { break }
}

Write-Check 'WebView2 Runtime' ($null -ne $webViewVersion) $(if ($webViewVersion) { "версия $webViewVersion обнаружена в реестре" } else { 'runtime не найден в реестре 32/64-bit для компьютера или текущего пользователя' })

if ($missing.Count -gt 0) {
    Write-Host "`nНе хватает обязательных компонентов: $($missing -join ', '). Установите их вручную и запустите проверку снова." -ForegroundColor Yellow
    exit 1
}

Write-Host "`nСреда готова для сборки и запуска Axiom Browser." -ForegroundColor Green
exit 0
