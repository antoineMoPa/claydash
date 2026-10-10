$ErrorActionPreference = 'Stop'
$repo = 'antoineMoPa/claydash'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'Use install.sh on macOS or Linux.'
}
# On ARM64 Windows do not silently choose an emulated x64 build.
$architecture = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
if ($architecture -ne 'AMD64') { throw 'This installer supports Windows x64.' }
$installDir = if ($env:CLAYDASH_INSTALL_DIR) { $env:CLAYDASH_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Claydash\bin' }
$version = if ($env:CLAYDASH_VERSION) { $env:CLAYDASH_VERSION } else { 'latest' }
if ($version -eq 'latest') {
    $base = "https://github.com/$repo/releases/latest/download"
} elseif ($version -match '^v\d+\.\d+\.\d+$') {
    $base = "https://github.com/$repo/releases/download/$version"
} else { throw 'CLAYDASH_VERSION must be latest or vMAJOR.MINOR.PATCH' }
if ($env:CLAYDASH_DOWNLOAD_BASE_URL) { $base = $env:CLAYDASH_DOWNLOAD_BASE_URL }
$archive = 'claydash-x86_64-pc-windows-msvc.zip'
$tempDir = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $tempDir | Out-Null
try {
    # Windows PowerShell 5.1 can otherwise select older TLS defaults.
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -UseBasicParsing "$base/$archive" -OutFile (Join-Path $tempDir $archive)
    Invoke-WebRequest -UseBasicParsing "$base/$archive.sha256" -OutFile (Join-Path $tempDir "$archive.sha256")
    $expected = ((Get-Content (Join-Path $tempDir "$archive.sha256") -First 1) -split '\s+')[0]
    $actual = (Get-FileHash (Join-Path $tempDir $archive) -Algorithm SHA256).Hash
    if ($expected -notmatch '^[a-fA-F0-9]{64}$' -or $expected -ne $actual) { throw 'Checksum verification failed' }
    Expand-Archive -LiteralPath (Join-Path $tempDir $archive) -DestinationPath (Join-Path $tempDir 'expanded')
    $binary = Join-Path $tempDir 'expanded\claydash.exe'
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw 'Archive has no claydash.exe' }
    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    Copy-Item -LiteralPath $binary -Destination (Join-Path $installDir 'claydash.exe') -Force
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($installDir -notin ($userPath -split ';')) {
        $newPath = if ($userPath) { "$userPath;$installDir" } else { $installDir }
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    }
    if ($installDir -notin ($env:Path -split ';')) { $env:Path = "$env:Path;$installDir" }
    Write-Host "Installed $installDir\claydash.exe. Run claydash to open the app."
    Write-Host 'New terminals will also have claydash on PATH.'
} finally { Remove-Item -LiteralPath $tempDir -Recurse -Force }
