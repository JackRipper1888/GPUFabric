param(
    [string]$BaseUrl = "https://oss.gpunexus.com/client",
    [string]$InstallDir = "$env:USERPROFILE\AppData\Local\Programs\gpuf-c",
    [string]$PackageName = "v1.0.2-windows-gpuf-c.tar.gz",
    [string]$DownloadDir = "C:\gpuf"
)

# check if running as administrator
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "错误：请以管理员身份运行此脚本" -ForegroundColor Red
    exit 1
}

$ErrorActionPreference = 'Stop'

function Parse-Version([string]$v) {
    try { return [version]$v } catch { return $null }
}

function Get-ExpectedVersionFromPackageName([string]$Name) {
    try {
        $m = [regex]::Match($Name, "^v?([0-9]+\.[0-9]+\.[0-9]+)")
        if ($m.Success) {
            return $m.Groups[1].Value
        }
    } catch {
    }
    return $null
}

function Get-InstalledVersionMarker([string]$Dir) {
    try {
        $marker = Join-Path $Dir ".gpuf_version"
        if (Test-Path $marker) {
            $v = (Get-Content -Path $marker -ErrorAction SilentlyContinue | Select-Object -First 1)
            if ($v) {
                $m = [regex]::Match($v, "([0-9]+\.[0-9]+\.[0-9]+)")
                if ($m.Success) {
                    return $m.Groups[1].Value
                }
            }
        }
    } catch {
    }
    return $null
}

function Get-InstalledGpufVersion([string]$ExePath) {
    if (-not (Test-Path $ExePath)) {
        return $null
    }

    try {
        $out = & $ExePath --version 2>&1
        $s = ($out | Out-String)
        $m = [regex]::Match($s, "([0-9]+\.[0-9]+\.[0-9]+)")
        if ($m.Success) {
            return $m.Groups[1].Value
        }
    } catch {
    }

    return $null
}

function Ensure-InstallDirOnPath([string]$Dir) {
    $currentPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($currentPath -notlike "*$Dir*") {
        [Environment]::SetEnvironmentVariable('Path', "$currentPath;$Dir", 'User')
        $env:Path += ";$Dir"
    }
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
        Write-Host "警告：文件名中未找到 md5 前缀（跳过 md5 前缀检查）：$([System.IO.Path]::GetFileName($Path))" -ForegroundColor Yellow
        return
    }

    $md5 = (Get-FileHash -Algorithm MD5 -Path $Path).Hash.ToLower()
    if ($md5.Substring(0, 6) -ne $prefix) {
        Write-Host "错误：$Path 的 md5 前缀不匹配" -ForegroundColor Red
        Write-Host "期望前缀：$prefix" -ForegroundColor Yellow
        Write-Host "实际 md5：      $md5" -ForegroundColor Yellow
        exit 1
    }

    Write-Host "md5 前缀匹配正确：$md5" -ForegroundColor Green
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
        Write-Host "错误：提取的文件不是有效的 Windows 可执行文件：$Path" -ForegroundColor Red
        exit 1
    }

    $is64 = [Environment]::Is64BitOperatingSystem
    if (-not $is64 -and $machine -eq 0x8664) {
        Write-Host "错误：此包包含 x64 可执行文件，但您的 Windows 似乎是 32 位" -ForegroundColor Red
        Write-Host "提示：安装 32 位构建或使用 64 位 Windows" -ForegroundColor Yellow
        exit 1
    }

    $arch = $env:PROCESSOR_ARCHITECTURE
    $arch2 = $env:PROCESSOR_ARCHITEW6432
    $isArm = ($arch -eq 'ARM64' -or $arch2 -eq 'ARM64')
    if (-not $isArm -and $machine -eq 0xAA64) {
        Write-Host "错误：此包包含 ARM64 可执行文件，但您的 Windows 不是 ARM64" -ForegroundColor Red
        Write-Host "提示：安装 x64 构建" -ForegroundColor Yellow
        exit 1
    }
}

function Write-DownloadProgress([int64]$Done, [int64]$Total) {
    if (-not $script:__gpuf_lastProgressLen) {
        $script:__gpuf_lastProgressLen = 0
    }

    $width = 50
    try {
        $w = $Host.UI.RawUI.WindowSize.Width
        if ($w -gt 40) {
            $width = [math]::Max(10, [math]::Min(70, $w - 40))
        }
    } catch {
    }

    $pct = 0
    if ($Total -gt 0) {
        $pct = [math]::Min(100, [math]::Floor(($Done * 100.0) / $Total))
    }

    $filled = [math]::Floor(($pct * $width) / 100)
    $empty = $width - $filled

    $fillChar = [string][char]0x2588
    $emptyChar = [string][char]0x2591
    $bar = (($fillChar * $filled) + ($emptyChar * $empty))

    $doneMb = [math]::Round($Done / 1MB, 2)
    if ($Total -gt 0) {
        $totalMb = [math]::Round($Total / 1MB, 2)
        $line = "下载中 [$bar] $pct% ($doneMb/$totalMb MB)"
    } else {
        $line = "下载中 [$bar] $doneMb MB"
    }

    $pad = ""
    if ($script:__gpuf_lastProgressLen -gt $line.Length) {
        $pad = (' ' * ($script:__gpuf_lastProgressLen - $line.Length))
    }
    $script:__gpuf_lastProgressLen = $line.Length

    Write-Host -NoNewline ("`r" + $line + $pad)
}

function Complete-DownloadProgress {
    if (-not $script:__gpuf_lastProgressLen) {
        $script:__gpuf_lastProgressLen = 0
    }
    if ($script:__gpuf_lastProgressLen -gt 0) {
        Write-Host ""
    }
    $script:__gpuf_lastProgressLen = 0
}

function Get-RemoteContentLength([string]$Url) {
    try {
        $req = [System.Net.HttpWebRequest]::Create($Url)
        $req.Method = 'HEAD'
        $req.AllowAutoRedirect = $true
        $resp = $req.GetResponse()
        try {
            return [int64]$resp.ContentLength
        } finally {
            try { $resp.Close() } catch { }
        }
    } catch {
        return [int64]-1
    }
}

function Download-FileWithProgress([string]$Url, [string]$OutFile) {
    $req = [System.Net.HttpWebRequest]::Create($Url)
    $req.Method = 'GET'
    $req.AllowAutoRedirect = $true

    $resp = $req.GetResponse()
    try {
        $total = $resp.ContentLength
    } catch {
        $total = -1
    }

    $inStream = $resp.GetResponseStream()
    $outStream = [System.IO.File]::Open($OutFile, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write, [System.IO.FileShare]::ReadWrite)

    try {
        $buffer = New-Object byte[] (1024 * 1024)
        $done = [int64]0
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $lastUpdateMs = [int64]0
        while (($read = $inStream.Read($buffer, 0, $buffer.Length)) -gt 0) {
            $outStream.Write($buffer, 0, $read)
            $done += $read

            if (($sw.ElapsedMilliseconds - $lastUpdateMs) -ge 500) {
                Write-DownloadProgress $done $total

                $lastUpdateMs = $sw.ElapsedMilliseconds
            }
        }
    } finally {
        try { $outStream.Close() } catch { }
        try { $inStream.Close() } catch { }
        try { $resp.Close() } catch { }
        Complete-DownloadProgress
    }
}

function Download-FilePreferCurl([string]$Url, [string]$OutFile) {
    $curl = Get-Command curl.exe -ErrorAction SilentlyContinue
    if ($curl) {
        try {
            $total = Get-RemoteContentLength $Url

            if ($total -gt 0 -and (Test-Path $OutFile)) {
                try {
                    $existingLen = (Get-Item $OutFile).Length
                    if ($existingLen -ge $total) {
                        if ($existingLen -gt $total) {
                            Remove-Item -Path $OutFile -Force -ErrorAction SilentlyContinue
                        } else {
                            Write-DownloadProgress $existingLen $total
                            Complete-DownloadProgress
                            return
                        }
                    }
                } catch {
                }
            }

            $args = @(
                '-L',
                '-C', '-',
                '--fail',
                '--retry', '5',
                '--retry-delay', '2',
                '--silent',
                '--show-error',
                '-o', $OutFile,
                $Url
            )

            $p = Start-Process -FilePath $curl.Source -ArgumentList $args -NoNewWindow -PassThru
            while (-not $p.HasExited) {
                $done = [int64]0
                try {
                    if (Test-Path $OutFile) {
                        $done = (Get-Item $OutFile).Length
                    }
                } catch {
                }

                Write-DownloadProgress $done $total
                Start-Sleep -Milliseconds 500
            }

            $done = [int64]0
            try {
                if (Test-Path $OutFile) {
                    $done = (Get-Item $OutFile).Length
                }
            } catch {
            }
            Write-DownloadProgress $done $total
            Complete-DownloadProgress

            if ($total -gt 0 -and $done -ge $total) {
                return
            }

            if ($p.ExitCode -ne 0) {
                throw "curl.exe exited with code $($p.ExitCode)"
            }

            return
        } catch {
            Write-Host "警告：curl.exe 下载失败，回退到直接下载：$_" -ForegroundColor Yellow
        }
    } else {
        Write-Host "警告：未找到 curl.exe，回退到直接下载" -ForegroundColor Yellow
    }

    Download-FileWithProgress $Url $OutFile
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
    Write-Host "错误：Windows 需要 Vulkan 运行时或 CUDA 版本 >= 13.0" -ForegroundColor Red
    if ($hasVulkan) {
        Write-Host "检测到 Vulkan" -ForegroundColor Green
    } else {
        Write-Host "未检测到 Vulkan（未找到 vulkan-1.dll）" -ForegroundColor Yellow
    }
    if ($cudaVersionStr) {
        Write-Host "检测到 CUDA：$cudaVersionStr（需要 >= 13.0）" -ForegroundColor Yellow
    } else {
        Write-Host "未检测到 CUDA（未找到 nvidia-smi/nvcc/注册表）" -ForegroundColor Yellow
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
        Write-Host "错误：无法写入 DownloadDir：$DownloadDir" -ForegroundColor Red
        Write-Host "提示：使用 -DownloadDir C:\\gpuf 或您可以写入的目录" -ForegroundColor Yellow
        exit 1
    }

    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }

    $expectedVer = Get-ExpectedVersionFromPackageName $PackageName
    $installedExe = Join-Path $InstallDir "gpuf-c.exe"
    $installedVer = Get-InstalledVersionMarker $InstallDir
    if (-not $installedVer) {
        $installedVer = Get-InstalledGpufVersion $installedExe
    }
    if ($expectedVer -and $installedVer -and (Test-Path $installedExe) -and ((Parse-Version $installedVer) -eq (Parse-Version $expectedVer))) {
        Ensure-InstallDirOnPath $InstallDir
        Write-Host "gpuf-c 已安装且是最新版本（版本 $installedVer）。跳过下载。" -ForegroundColor Green
        exit 0
    }

    if (-not $expectedVer) {
        Write-Host "警告：无法从 PackageName 解析期望版本：$PackageName（将重新安装）" -ForegroundColor Yellow
    } else {
        $markerPath = Join-Path $InstallDir ".gpuf_version"
        if (Test-Path $markerPath) {
            $markerVer = Get-InstalledVersionMarker $InstallDir
            if ((-not (Test-Path $installedExe)) -and $markerVer -and ((Parse-Version $markerVer) -eq (Parse-Version $expectedVer))) {
                Write-Host "警告：版本标记指示最新（$markerVer）但 gpuf-c.exe 缺失；将重新安装" -ForegroundColor Yellow
            } elseif ($markerVer -and ((Parse-Version $markerVer) -ne (Parse-Version $expectedVer))) {
                Write-Host "警告：已安装版本标记（$markerVer）!= 期望（$expectedVer），将重新安装" -ForegroundColor Yellow
            }
        } elseif ($installedVer) {
            Write-Host "警告：检测到已安装版本（$installedVer）但期望（$expectedVer），将重新安装" -ForegroundColor Yellow
        } elseif (Test-Path $installedExe) {
            Write-Host "警告：gpuf-c.exe 存在但无法确定版本（标记缺失且 --version 解析失败），将重新安装" -ForegroundColor Yellow
        }
    }

    Write-Host "下载中：$pkgUrl" -ForegroundColor Yellow
    Write-Host "下载路径：$archivePath" -ForegroundColor Yellow
    if (Test-Path $archivePath) {
        Remove-Item -Path $archivePath -Force -ErrorAction SilentlyContinue
    }
    $tmpArchivePath = "$archivePath.part"
    Download-FilePreferCurl $pkgUrl $tmpArchivePath
    if (Test-Path $archivePath) {
        Remove-Item -Path $archivePath -Force -ErrorAction SilentlyContinue
    }
    try {
        Move-Item -Path $tmpArchivePath -Destination $archivePath -Force
    } catch {
        Write-Host "错误：无法完成归档移动到 $archivePath" -ForegroundColor Red
        Write-Host "提示：目标文件可能被其他进程锁定；归档保留在：$tmpArchivePath" -ForegroundColor Yellow
        throw
    }

    # Extract (.tar.gz) using tar.exe (available on most Windows 10/11)
    $tar = Get-Command tar -ErrorAction SilentlyContinue
    if (-not $tar) {
        Write-Host "错误：未找到 tar 命令。请安装 tar/bsdtar 或使用基于 zip 的包。" -ForegroundColor Red
        exit 1
    }

    # Clean up old installation files before extracting new version
    Write-Host "清理旧安装文件..." -ForegroundColor Yellow
    if (Test-Path $InstallDir) {
        try {
            # Remove all files in InstallDir but keep the directory
            Get-ChildItem -Path $InstallDir -Recurse | Remove-Item -Force -Recurse -ErrorAction SilentlyContinue
            Write-Host "旧文件已删除" -ForegroundColor Green
        } catch {
            Write-Host "警告：删除一些旧文件失败：$_" -ForegroundColor Yellow
        }
    }

    Write-Host "解压到：$InstallDir" -ForegroundColor Yellow
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
            Write-Host "错误：在 $InstallDir 中解压后未找到 .exe" -ForegroundColor Red
            Write-Host "提示：归档可能包含意外的布局或不是 Windows 包" -ForegroundColor Yellow
            Write-Host "解压文件（前 50 个）：" -ForegroundColor Yellow
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
    Ensure-InstallDirOnPath $InstallDir

    try {
        $expectedVer = Get-ExpectedVersionFromPackageName $PackageName
        if ($expectedVer) {
            Set-Content -Path (Join-Path $InstallDir ".gpuf_version") -Value $expectedVer -Encoding Ascii -Force
        }
    } catch {
    }

    Remove-Item -Path $archivePath -Force -ErrorAction SilentlyContinue

    Write-Host "gpuf-c (llama.cpp) 安装成功！" -ForegroundColor Green
    Write-Host "安装目录：$InstallDir" -ForegroundColor Yellow
    Write-Host "请重启终端以使 PATH 更改生效。" -ForegroundColor Yellow

} catch {
    Write-Host "安装失败：$_" -ForegroundColor Red
    exit 1
}
