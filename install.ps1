# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
# Copyright (c) 2026 2youg1 and the sprawling contributors
#
# One command that leaves `sprawling` on your PATH:
#
#     irm https://raw.githubusercontent.com/2youg1/sprawling/main/install.ps1 | iex
#
# This script fetches and unpacks. **Where the binary goes, and what
# happens to PATH, is decided by `sprawling install`** - the binary's own
# installer, which runs at the end. Two installers choosing a directory
# would be two authorities for one rule, and the one a person can re-run
# later is the one that has to win.
#
# Three facts about this repository's releases that the obvious script
# gets wrong:
#
#   * Every release so far is a pre-release, and GitHub's `releases/latest`
#     endpoint excludes those - it answers 404 here. The list endpoint is
#     asked for its newest entry instead.
#   * The tag and the archive carry different versions: tag
#     `v0.0.3-Pre-alpha-260903` ships `sprawling-0.0.3-windows-x86_64.zip`.
#     An asset is chosen by its platform suffix, never by a name built out
#     of the tag.
#   * Only the platforms the release workflow builds are installable. A
#     platform with no archive is reported with the list the release does
#     carry, rather than guessed at.
#
# `sprawling` is the short name GitHub still resolves to this repository,
# whose canonical path is `2youg1/sprawling-agents`. Set SPRAWLING_REPO if
# that ever stops being true.

$ErrorActionPreference = 'Stop'

$repo = if ($env:SPRAWLING_REPO) { $env:SPRAWLING_REPO } else { '2youg1/sprawling' }
$api = "https://api.github.com/repos/$repo/releases"
$releases = "https://github.com/$repo/releases"

function Die($message) {
    Write-Error "sprawling install: $message"
    exit 1
}

# Windows PowerShell 5.1 still negotiates SSL3/TLS1.0 by default on some
# builds, and GitHub answers those with a closed connection rather than
# with an error a reader can act on. PowerShell 7 already defaults to the
# system's choice, so this is set rather than forced.
if ([Net.ServicePointManager]::SecurityProtocol -notmatch 'Tls12') {
    [Net.ServicePointManager]::SecurityProtocol =
        [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
}

$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    'AMD64' { 'x86_64' }
    'ARM64' { 'aarch64' }
    'x86'   { Die 'no 32-bit archive is built; build from source with `just dist`' }
    default { Die "no archive is built for $($env:PROCESSOR_ARCHITECTURE)" }
}
$suffix = "-windows-$arch.zip"

Write-Host "sprawling: asking $repo what it has for windows-$arch"
try {
    $release = if ($env:SPRAWLING_VERSION) {
        Invoke-RestMethod -Uri "$api/tags/$($env:SPRAWLING_VERSION)" -UseBasicParsing
    } else {
        # `-1` is what the list endpoint answers with when asked for one
        # entry; PowerShell unwraps a single-element array, so the value
        # is the release either way.
        @(Invoke-RestMethod -Uri "${api}?per_page=1" -UseBasicParsing)[0]
    }
} catch {
    Die "cannot reach $api ($($_.Exception.Message)); download from $releases instead"
}
if (-not $release) { Die "no release found; see $releases" }

$asset = $release.assets | Where-Object { $_.name.EndsWith($suffix) } | Select-Object -First 1
if (-not $asset) {
    Write-Host "sprawling: $($release.tag_name) carries no archive ending $suffix. What it does carry:"
    $release.assets | ForEach-Object { Write-Host "  $($_.name)" }
    Die "download one of those from $releases, or build from source with ``just dist``"
}

# The digest the release publishes, in the form `sha256:<hex>`. Absent
# means the bytes cannot be checked, and an unverified download is not
# installed here.
if (-not $asset.digest -or -not $asset.digest.StartsWith('sha256:')) {
    Die ("$($release.tag_name) publishes no sha256 for $($asset.name), so the bytes " +
         "cannot be checked. Download it yourself from $releases if you accept that.")
}
$expected = $asset.digest.Substring(7)

$work = Join-Path ([IO.Path]::GetTempPath()) "sprawling-install-$([Guid]::NewGuid())"
New-Item -ItemType Directory -Path $work -Force | Out-Null
try {
    $archive = Join-Path $work 'archive.zip'
    Write-Host "sprawling: downloading $($release.tag_name)"
    # The progress bar costs more than the download on Windows PowerShell:
    # Invoke-WebRequest redraws it per chunk, and on a several-megabyte
    # file that is most of the wall clock.
    $progress = $ProgressPreference
    $ProgressPreference = 'SilentlyContinue'
    try {
        Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $archive -UseBasicParsing
    } finally {
        $ProgressPreference = $progress
    }

    $got = (Get-FileHash -Path $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($got -ne $expected) {
        Die ("the archive does not match the sha256 the release publishes.`n" +
             "  expected $expected`n  received $got`nNothing was installed.")
    }

    Expand-Archive -Path $archive -DestinationPath $work -Force
    $binary = Get-ChildItem -Path $work -Filter 'sprawling.exe' -Recurse -File |
        Select-Object -First 1
    if (-not $binary) { Die 'the archive holds no file named sprawling.exe' }

    # The binary places itself. Everything this script knows about
    # directories and PATH ends here, and what it prints below comes from
    # the installer a person can run again by hand.
    & $binary.FullName install
    if ($LASTEXITCODE -ne 0) { Die "sprawling install exited with $LASTEXITCODE" }
} finally {
    Remove-Item -Path $work -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host 'Next: run `sprawling up` to raise a city and open it in your browser.'
Write-Host 'It needs a model to call before it can do anything - `sprawling help` lists the rest.'
