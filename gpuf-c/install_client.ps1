param(
    [string]$BaseUrl = "https://oss.gpunexus.com/client",
    [string]$InstallDir = "$env:USERPROFILE\AppData\Local\Programs\gpuf-c",
    [string]$PackageName = "v1.0.1-windows-gpuf-c.tar.gz",
    [string]$DownloadDir = "C:\gpuf"
)

# check if running as administrator
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "error: please run this script as administrator" -ForegroundColor Red
    exit 1
}

$ErrorActionPreference = 'Stop'

function Parse-Version([string]$v) {
    try { return [version]$v } catch { return $null }
}

function Get-CudaVersion {
    # Prefer nvcc if available
    $nvcc = Get-Command nvcc -ErrorAction SilentlyContinue
    if ($nvcc) {
        $out = & nvcc --version 2>$null
        $m = [regex]::Match(($out | Out-String), "release\s+([0-9]+\.[0-9]+)")
        if ($m.Success) { return $m.Groups[1].Value }
    }

    # Prefer nvidia-smi (works even without CUDA Toolkit)
    $smi = Get-Command nvidia-smi -ErrorAction SilentlyContinue
    if ($smi) {
        $out = & nvidia-smi 2>$null
        $m = [regex]::Match(($out | Out-String), "CUDA Version:\s*([0-9]+\.[0-9]+)")
        if ($m.Success) { return $m.Groups[1].Value }
    }

    return $null
}

function Has-Vulkan {
    $dll1 = Join-Path $env:WINDIR "System32\vulkan-1.dll"
    $dll2 = Join-Path $env:WINDIR "SysWOW64\vulkan-1.dll"
    return (Test-Path $dll1) -or (Test-Path $dll2)
}

function Get-Md5PrefixFromFileName([string]$Path) {
    $name = [System.IO.Path]::GetFileName($Path)
    $m = [regex]::Match($name, "^([0-9a-fA-F]{6})-")
    if ($m.Success) { return $m.Groups[1].Value.ToLower() }
    return $null
}

function Verify-Md5PrefixIfPossible([string]$Path) {
    $prefix = Get-Md5PrefixFromFileName $Path
    if (-not $prefix) {
        Write-Host "warning: md5 prefix not found in filename (skip md5 prefix check): $([System.IO.Path]::GetFileName($Path))" -ForegroundColor Yellow
        return
    }

    $md5 = (Get-FileHash -Algorithm MD5 -Path $Path).Hash.ToLower()
    if ($md5.Substring(0, 6) -ne $prefix) {
        Write-Host "error: md5 prefix mismatch for $Path" -ForegroundColor Red
        Write-Host "expected prefix: $prefix" -ForegroundColor Yellow
        Write-Host "actual md5:      $md5" -ForegroundColor Yellow
        exit 1
    }

    Write-Host "md5 prefix match ok: $md5" -ForegroundColor Green
}

function Get-PeMachine([string]$Path) {
    try {
        $fs = [System.IO.File]::Open($Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
        try {
            $br = New-Object System.IO.BinaryReader($fs)
            $mz = $br.ReadUInt16()
            if ($mz -ne 0x5A4D) { return $null }
            $fs.Seek(0x3C, [System.IO.SeekOrigin]::Begin) | Out-Null
            $peOffset = $br.ReadInt32()
            if ($peOffset -lt 0) { return $null }
            $fs.Seek($peOffset, [System.IO.SeekOrigin]::Begin) | Out-Null
            $peSig = $br.ReadUInt32()
            if ($peSig -ne 0x00004550) { return $null }
            return $br.ReadUInt16()
        } finally {
            $fs.Close()
        }
    } catch {
        return $null
    }
}

function Assert-ExeCompatible([string]$Path) {
    $machine = Get-PeMachine $Path
    if (-not $machine) {
        Write-Host "error: extracted file is not a valid Windows executable: $Path" -ForegroundColor Red
        exit 1
    }

    $is64 = [Environment]::Is64BitOperatingSystem
    if (-not $is64 -and $machine -eq 0x8664) {
        Write-Host "error: this package contains an x64 executable but your Windows appears to be 32-bit" -ForegroundColor Red
        Write-Host "hint: install a 32-bit build or use a 64-bit Windows" -ForegroundColor Yellow
        exit 1
    }

    $arch = $env:PROCESSOR_ARCHITECTURE
    $arch2 = $env:PROCESSOR_ARCHITEW6432
    $isArm = ($arch -eq 'ARM64' -or $arch2 -eq 'ARM64')
    if (-not $isArm -and $machine -eq 0xAA64) {
        Write-Host "error: this package contains an ARM64 executable but your Windows is not ARM64" -ForegroundColor Red
        Write-Host "hint: install the x64 build" -ForegroundColor Yellow
        exit 1
    }
}

$hasVulkan = Has-Vulkan
$cudaVersionStr = Get-CudaVersion
$cudaVersion = $null
if ($cudaVersionStr) { $cudaVersion = Parse-Version $cudaVersionStr }

$cudaOk = $false
if ($cudaVersion) {
    $cudaOk = $cudaVersion -ge (Parse-Version "13.0")
}

if (-not $hasVulkan -and -not $cudaOk) {
    Write-Host "error: Windows requires Vulkan runtime OR CUDA version >= 13.0" -ForegroundColor Red
    if ($hasVulkan) {
        Write-Host "Vulkan detected" -ForegroundColor Green
    } else {
        Write-Host "Vulkan not detected (vulkan-1.dll not found)" -ForegroundColor Yellow
    }
    if ($cudaVersionStr) {
        Write-Host "CUDA detected: $cudaVersionStr (require >= 13.0)" -ForegroundColor Yellow
    } else {
        Write-Host "CUDA not detected (nvidia-smi/nvcc/registry not found)" -ForegroundColor Yellow
    }
    exit 1
}

$pkgUrl = "$BaseUrl/$PackageName"
$archivePath = Join-Path $DownloadDir $PackageName

try {
    if (-not (Test-Path $DownloadDir)) {
        New-Item -ItemType Directory -Path $DownloadDir -Force | Out-Null
    }

    try {
        $testPath = Join-Path $DownloadDir (".gpuf_write_test_" + [Guid]::NewGuid().ToString("N"))
        Set-Content -Path $testPath -Value "1" -Encoding Ascii -Force
        Remove-Item -Path $testPath -Force -ErrorAction SilentlyContinue
    } catch {
        Write-Host "error: cannot write to DownloadDir: $DownloadDir" -ForegroundColor Red
        Write-Host "hint: use -DownloadDir C:\\gpuf or a directory you can write to" -ForegroundColor Yellow
        exit 1
    }

    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }

    Write-Host "Downloading: $pkgUrl" -ForegroundColor Yellow
    Write-Host "DownloadPath: $archivePath" -ForegroundColor Yellow
    if (Test-Path $archivePath) {
        Remove-Item -Path $archivePath -Force -ErrorAction SilentlyContinue
    }
    (New-Object System.Net.WebClient).DownloadFile($pkgUrl, $archivePath)

    # Extract (.tar.gz) using tar.exe (available on most Windows 10/11)
    $tar = Get-Command tar -ErrorAction SilentlyContinue
    if (-not $tar) {
        Write-Host "error: tar command not found. Please install tar/bsdtar or use a zip-based package." -ForegroundColor Red
        exit 1
    }

    Write-Host "Extracting to: $InstallDir" -ForegroundColor Yellow
    & tar -xzf $archivePath -C $InstallDir

    # Expect gpuf-c.exe inside root of the archive.
    # But releases may contain a top-level folder, so search recursively.
    $exe = Join-Path $InstallDir "gpuf-c.exe"
    if (-not (Test-Path $exe)) {
        $candidate = Get-ChildItem -Path $InstallDir -Recurse -Filter "gpuf-c.exe" -File -ErrorAction SilentlyContinue | Select-Object -First 1
        if (-not $candidate) {
            $candidate = Get-ChildItem -Path $InstallDir -Recurse -Filter "*gpuf-c*.exe" -File -ErrorAction SilentlyContinue | Select-Object -First 1
        }
        if (-not $candidate) {
            $candidate = Get-ChildItem -Path $InstallDir -Recurse -Filter "*.exe" -File -ErrorAction SilentlyContinue | Select-Object -First 1
        }
        if (-not $candidate) {
            Write-Host "error: no .exe found after extraction in $InstallDir" -ForegroundColor Red
            Write-Host "hint: archive may contain unexpected layout or is not a Windows package" -ForegroundColor Yellow
            Write-Host "extracted files (top 50):" -ForegroundColor Yellow
            Get-ChildItem -Path $InstallDir -Recurse -Force -ErrorAction SilentlyContinue | Select-Object -First 50 FullName
            exit 1
        }

        Assert-ExeCompatible $candidate.FullName

        Verify-Md5PrefixIfPossible $candidate.FullName

        $srcDir = $candidate.DirectoryName
        Copy-Item -Path $candidate.FullName -Destination $exe -Force

        # Copy adjacent runtime DLLs next to gpuf-c.exe (required for CUDA builds on Windows)
        $dlls = Get-ChildItem -Path $srcDir -Filter "*.dll" -File -ErrorAction SilentlyContinue
        foreach ($d in $dlls) {
            Copy-Item -Path $d.FullName -Destination (Join-Path $InstallDir $d.Name) -Force
        }

        # Copy common ancillary files if present
        $extras = @("ca-cert.pem", "read.txt")
        foreach ($e in $extras) {
            $p = Join-Path $srcDir $e
            if (Test-Path $p) {
                Copy-Item -Path $p -Destination (Join-Path $InstallDir $e) -Force
            }
        }
    }

    # add to PATH
    $currentPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($currentPath -notlike "*$InstallDir*") {
        [Environment]::SetEnvironmentVariable('Path', "$currentPath;$InstallDir", 'User')
        $env:Path += ";$InstallDir"
    }

    Remove-Item -Path $archivePath -Force -ErrorAction SilentlyContinue

    Write-Host "gpuf-c (llama.cpp) installed successfully!" -ForegroundColor Green
    Write-Host "InstallDir: $InstallDir" -ForegroundColor Yellow
    Write-Host "Please restart terminal to make PATH changes take effect." -ForegroundColor Yellow

} catch {
    Write-Host "installation failed: $_" -ForegroundColor Red
    exit 1
}
