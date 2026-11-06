# Run Rust unit tests on Windows.
#
# `cargo test --lib` builds `agent_lib-<hash>.exe`, which is not covered by
# Cargo's `rustc-link-arg-tests` but still imports comctl32 v6
# (TaskDialogIndirect) through Tauri/rfd. Without an embedded manifest the
# loader picks v5 and the process exits with STATUS_ENTRYPOINT_NOT_FOUND.
#
# Build, embed tests.manifest with mt.exe, then run the harness.

$ErrorActionPreference = 'Stop'

cargo test --manifest-path src-tauri/Cargo.toml --lib --no-run
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$exe = Get-ChildItem 'src-tauri/target/debug/deps/agent_lib-*.exe' |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
if (-not $exe) {
    Write-Error 'agent_lib test executable not found'
    exit 1
}

$mt = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin\*\x64\mt.exe' -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending |
    Select-Object -First 1
if (-not $mt) {
    Write-Error 'mt.exe not found (install the Windows SDK)'
    exit 1
}

& $mt.FullName -nologo -manifest 'src-tauri/tests.manifest' -outputresource:"$($exe.FullName);#1"
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

& $exe.FullName @args
exit $LASTEXITCODE
