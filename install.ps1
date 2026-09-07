param(
    [ValidateSet("cli", "gui")]
    [string]$Product = "cli",
    [switch]$NoPathUpdate
)


$ErrorActionPreference = "Stop"

$repo = if ($env:IMG_REPO) { $env:IMG_REPO } else { "liyown/img" }
$version = if ($env:IMG_VERSION) { $env:IMG_VERSION } else { "latest" }
$architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()

if ($architecture -ne "x64") {
    throw "img currently supports Windows x64. Detected: $architecture"
}

if ($Product -eq "gui") {
    if ($version -eq "latest") {
        $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases?per_page=100"
        $release = $releases | Where-Object { -not $_.draft -and -not $_.prerelease -and $_.tag_name -match '^desktop-v\d+\.\d+\.\d+$' } | Sort-Object { [version]($_.tag_name -replace '^desktop-v','') } -Descending | Select-Object -First 1
        if (-not $release) { throw "No desktop release is available." }
        $version = $release.tag_name -replace '^desktop-v',''
    }
    $version = $version -replace '^(desktop-v|v)',''
    if ($version -notmatch '^\d+\.\d+\.\d+$') { throw "Invalid desktop version" }
    $asset = "img-desktop_${version}_windows_x86_64.exe"
    $baseUrl = "https://github.com/$repo/releases/download/desktop-v$version"
    $tempDir = Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory $tempDir | Out-Null
    try {
        $package = Join-Path $tempDir $asset
        $checksum = "$package.sha256"
        if ($env:IMG_LOCAL_PACKAGE_DIR) {
            Copy-Item (Join-Path $env:IMG_LOCAL_PACKAGE_DIR $asset) $package
            Copy-Item (Join-Path $env:IMG_LOCAL_PACKAGE_DIR "$asset.sha256") $checksum
        } else {
            Invoke-WebRequest "$baseUrl/$asset" -OutFile $package
            Invoke-WebRequest "$baseUrl/$asset.sha256" -OutFile $checksum
        }
        $fields = (Get-Content $checksum -Raw).Trim() -split '\s+'
        if ($fields.Count -ne 2 -or $fields[1] -ne $asset -or (Get-FileHash $package -Algorithm SHA256).Hash -ne $fields[0]) { throw "Desktop checksum verification failed" }
        $process = Start-Process -FilePath $package -ArgumentList '/CLOSEAPPLICATIONS','/NORESTART' -Wait -PassThru
        if ($process.ExitCode -ne 0) { throw "Desktop installer failed: $($process.ExitCode)" }
    } finally { Remove-Item $tempDir -Recurse -Force }
    return
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
    if ($env:IMG_LOCAL_PACKAGE_DIR) {
        Copy-Item (Join-Path $env:IMG_LOCAL_PACKAGE_DIR $asset) $archive
        Copy-Item (Join-Path $env:IMG_LOCAL_PACKAGE_DIR "checksums.txt") $checksums
    } else {
        Invoke-WebRequest -UseBasicParsing -Uri "$baseUrl/$asset" -OutFile $archive
        Invoke-WebRequest -UseBasicParsing -Uri "$baseUrl/checksums.txt" -OutFile $checksums
    }

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

    if (-not $NoPathUpdate) {
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        $pathEntries = if ($userPath) { $userPath -split ";" } else { @() }
        if ($pathEntries -notcontains $installDir) {
            $newUserPath = if ($userPath) { "$userPath;$installDir" } else { $installDir }
            [Environment]::SetEnvironmentVariable("Path", $newUserPath, "User")
        }
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
