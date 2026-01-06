param(
  [string]$CudaPath = "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.4",
  [string]$TargetDir = "dist\\windows-cuda12",
  [string]$Target = "x86_64-pc-windows-msvc"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $CudaPath)) {
  throw "CUDA path not found: $CudaPath"
}

$CudaBin = Join-Path $CudaPath "bin"
$CudaLib = Join-Path $CudaPath "lib\\x64"

if (-not (Test-Path $CudaBin)) {
  throw "CUDA bin directory not found: $CudaBin"
}

if (-not (Test-Path $CudaLib)) {
  throw "CUDA lib directory not found: $CudaLib"
}

$env:CUDA_PATH = $CudaPath
$env:Path = "$CudaBin;" + $env:Path

if ($env:LIB) {
  $env:LIB = "$CudaLib;" + $env:LIB
} else {
  $env:LIB = $CudaLib
}

Write-Host "Building gpuf-c (CUDA 12) ..."
Write-Host "  CUDA_PATH=$env:CUDA_PATH"

cargo build --release --bin gpuf-c --features cuda --target $Target

$ExePath = Join-Path "target" "$Target\\release\\gpuf-c.exe"
if (-not (Test-Path $ExePath)) {
  throw "gpuf-c.exe not found at: $ExePath"
}

New-Item -ItemType Directory -Force -Path $TargetDir | Out-Null
Copy-Item -Force $ExePath (Join-Path $TargetDir "gpuf-c.exe")

$Dlls = @(
  "cublas64_12.dll",
  "cublasLt64_12.dll",
  "cudart64_12.dll"
)

foreach ($dll in $Dlls) {
  $src = Join-Path $CudaBin $dll
  if (Test-Path $src) {
    Copy-Item -Force $src (Join-Path $TargetDir $dll)
  } else {
    Write-Warning "CUDA DLL not found (skipping): $src"
  }
}

Write-Host "Done. Output: $TargetDir"
