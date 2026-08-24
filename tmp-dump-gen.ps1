$ErrorActionPreference = 'Stop'
$f = Get-ChildItem -Recurse -Filter generated.rs -Path 'target\debug\build' |
    Where-Object { $_.FullName -match 'codegen-tests' } |
    Sort-Object LastWriteTime | Select-Object -Last 1
$c = Get-Content $f.FullName
function Show($pat) {
    Write-Output "===== $pat ====="
    for ($i = 0; $i -lt $c.Count; $i++) {
        if ($c[$i] -match $pat) {
            $s = [Math]::Max(0, $i - 2)
            $e = [Math]::Min($c.Count - 1, $i + 14)
            Write-Output ("{0}:{1}" -f ($s + 1), ($e + 1))
            $c[$s..$e] | ForEach-Object { $_ }
        }
    }
}
Show 'pub struct Address'
Show 'pub struct Outer_Child_Leaf '
Show 'fn decode_inner'
