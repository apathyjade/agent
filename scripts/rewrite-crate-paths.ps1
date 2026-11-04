param(
    [Parameter(Mandatory = $true)][string[]]$Map,
    [string[]]$Roots = @('src-tauri/src', 'src-tauri/tests')
)
$ErrorActionPreference = 'Stop'
$utf8 = New-Object System.Text.UTF8Encoding($false)
$pairs = foreach ($entry in $Map) {
    $i = $entry.IndexOf('=')
    [pscustomobject]@{ From = $entry.Substring(0, $i); To = $entry.Substring($i + 1) }
}
foreach ($root in $Roots) {
    Get-ChildItem -Path $root -Recurse -Filter *.rs -File | ForEach-Object {
        $path = $_.FullName
        $text = [System.IO.File]::ReadAllText($path, $utf8)
        $new = $text
        foreach ($p in $pairs) {
            $new = [regex]::Replace($new, [regex]::Escape($p.From) + '(?![\w])', $p.To)
        }
        if ($new -ne $text) { [System.IO.File]::WriteAllText($path, $new, $utf8) }
    }
}
