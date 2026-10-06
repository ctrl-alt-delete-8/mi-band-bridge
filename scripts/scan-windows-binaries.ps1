# Scan Windows binaries for private details before they go out. Run it on
# Windows, in Windows PowerShell, with files or folders:
#
#   powershell -ExecutionPolicy Bypass -File scripts\scan-windows-binaries.ps1 <file or folder> ...
#
# scripts\build-windows.ps1 runs it over everything the installers carry,
# before it names them. It reads each .exe and .dll as bytes, and as UTF-16
# text at both byte offsets, and exits 1 when one holds any of these:
#
#   - a path in a Windows home folder: C:\Users\<anyone>\
#   - a Gmail address
#   - the name of the Windows user who builds, in its usual spellings
#   - the MAC address of the band this PC pairs with, in its usual forms
#
# The last two come from this PC, not from this file, so the script names no
# person, and can go public. Get-Patterns holds every pattern, and nothing
# else does. A hit is reported by what it is and how many times, never by the
# text it found.

param([Parameter(Mandatory, ValueFromRemainingArguments)][string[]]$Path)

$ErrorActionPreference = 'Stop'

# Every pattern the scan uses, by what it finds.
function Get-Patterns {
    $patterns = [ordered]@{
        'a path in a Windows home folder' = '[a-z]:[\\/]{1,2}users[\\/]{1,2}[^\\/\x00-\x1f"*:<>?|]{1,64}[\\/]'
        'a Gmail address' = '[a-z0-9._%+-]+@gmail\.com'
    }

    # The Windows user who builds: the account name, and the name of its home
    # folder. Each word of three letters or more alone, and the whole name
    # with each usual separator, or none.
    $names = @($env:USERNAME, (Split-Path -Leaf $env:USERPROFILE)) | Where-Object { $_ } | Select-Object -Unique
    $spellings = foreach ($name in $names) {
        $parts = @($name -split '[\s._-]+' | Where-Object { $_ })
        $parts | Where-Object { $_.Length -ge 3 }
        if ($parts.Count -gt 1) {
            foreach ($join in ' ', '', '.', '_', '-') { $parts -join $join }
        }
    }
    $spellings = @($spellings | Where-Object { $_ } | Sort-Object -Unique)
    if ($spellings.Count -gt 0) {
        $patterns['the name of the user who builds'] = ($spellings | ForEach-Object { [regex]::Escape($_) }) -join '|'
    }

    # The band this PC pairs with: [device] mac in BandBridge's config, with
    # colons, dashes or nothing between its bytes, and as an EUI-64.
    $config = Join-Path $env:APPDATA 'BandBridge\config.toml'
    if (Test-Path $config) {
        $line = Select-String -Path $config -Pattern '^\s*mac\s*=\s*"([0-9A-Fa-f:-]+)"' | Select-Object -First 1
        if ($line) {
            $hex = ($line.Matches[0].Groups[1].Value -replace '[:-]', '').ToLowerInvariant()
            if ($hex -match '^[0-9a-f]{12}$') {
                $bytes = for ($i = 0; $i -lt 12; $i += 2) { $hex.Substring($i, 2) }
                $patterns['the MAC address of the band this PC pairs with'] =
                    ($bytes -join '[:-]?') + '|' + $hex.Substring(0, 6) + 'fffe' + $hex.Substring(6)
            }
        }
    }
    $patterns
}

$files = @(foreach ($item in $Path) {
    if (Test-Path $item -PathType Container) {
        Get-ChildItem $item -Recurse -File -Include *.exe, *.dll
    } elseif (Test-Path $item -PathType Leaf) {
        Get-Item $item
    } else {
        throw "There is nothing to scan at $item."
    }
})

$patterns = Get-Patterns
Write-Host "Scanning $($files.Count) files for: $(@($patterns.Keys) -join '; ')."
$found = @{}
foreach ($label in $patterns.Keys) {
    $found[$label] = New-Object System.Text.RegularExpressions.Regex($patterns[$label], 'IgnoreCase, CultureInvariant')
}
$latin1 = [System.Text.Encoding]::GetEncoding(28591)
$hits = 0
foreach ($file in $files) {
    $bytes = [System.IO.File]::ReadAllBytes($file.FullName)
    # UTF-8 and ASCII text through Latin-1, which keeps one character a byte,
    # and UTF-16 text at both byte offsets.
    $texts = @($latin1.GetString($bytes), [System.Text.Encoding]::Unicode.GetString($bytes))
    if ($bytes.Length -gt 1) { $texts += [System.Text.Encoding]::Unicode.GetString($bytes, 1, $bytes.Length - 1) }
    foreach ($label in $patterns.Keys) {
        $count = 0
        foreach ($text in $texts) { $count += $found[$label].Matches($text).Count }
        if ($count -gt 0) {
            Write-Host "  HIT $($file.FullName): $label, $count times"
            $hits += $count
        }
    }
}
Write-Host "Scanned $($files.Count) files: $hits hits."
if ($hits -gt 0) { exit 1 }
exit 0
