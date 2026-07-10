$ErrorActionPreference = "Stop"

$repo = if ($env:IMG_REPO) { $env:IMG_REPO } else { "liyown/img" }
$version = if ($env:IMG_VERSION) { $env:IMG_VERSION } else { "latest" }
$architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()

if ($architecture -ne "x64") {
    throw "img currently supports Windows x64. Detected: $architecture"
}

$asset = "img_windows_amd64.zip"
if ($version -eq "latest") {
    $baseUrl = "https://github.com/$repo/releases/latest/download"
} else {
    $tag = if ($version.StartsWith("v")) { $version } else { "v$version" }
    $baseUrl = "https://github.com/$repo/releases/download/$tag"
}

$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("img-install-" + [guid]::NewGuid().ToString("N"))
$archive = Join-Path $tempDir $asset
$checksums = Join-Path $tempDir "checksums.txt"
$expanded = Join-Path $tempDir "expanded"

try {
    New-Item -ItemType Directory -Path $tempDir | Out-Null
    Write-Host "Downloading img..."
    Invoke-WebRequest -UseBasicParsing -Uri "$baseUrl/$asset" -OutFile $archive
    Invoke-WebRequest -UseBasicParsing -Uri "$baseUrl/checksums.txt" -OutFile $checksums

    $checksumLine = Get-Content $checksums | Where-Object { $_ -match "\s+$([regex]::Escape($asset))$" } | Select-Object -First 1
    if (-not $checksumLine) {
        throw "Checksum for $asset was not found."
    }
    $expected = ($checksumLine -split "\s+")[0].ToLowerInvariant()
    $actual = (Get-FileHash -Algorithm SHA256 -Path $archive).Hash.ToLowerInvariant()
    if ($actual -ne $expected) {
        throw "Download verification failed."
    }

    Expand-Archive -Path $archive -DestinationPath $expanded -Force
    $source = Join-Path $expanded "img.exe"
    if (-not (Test-Path $source)) {
        throw "The release package does not contain img.exe."
    }

    $installDir = if ($env:IMG_INSTALL_DIR) {
        $env:IMG_INSTALL_DIR
    } else {
        Join-Path $env:LOCALAPPDATA "Programs\img"
    }
    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    Copy-Item -Force $source (Join-Path $installDir "img.exe")

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $pathEntries = if ($userPath) { $userPath -split ";" } else { @() }
    if ($pathEntries -notcontains $installDir) {
        $newUserPath = if ($userPath) { "$userPath;$installDir" } else { $installDir }
        [Environment]::SetEnvironmentVariable("Path", $newUserPath, "User")
    }
    if (($env:Path -split ";") -notcontains $installDir) {
        $env:Path = "$installDir;$env:Path"
    }

    Write-Host "img installed successfully."
    & (Join-Path $installDir "img.exe") version
} finally {
    if (Test-Path $tempDir) {
        Remove-Item -Recurse -Force $tempDir
    }
}
