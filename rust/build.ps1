param([ValidateSet('check','host','kindle','host-test')][string]$Mode = 'check')
$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    $zigPath = python -c "import ziglang,os;print(os.path.join(os.path.dirname(ziglang.__file__),'zig.exe' if os.name=='nt' else 'zig'))"
    if ($LASTEXITCODE -ne 0) { throw '缺少 Python ziglang 包' }
    $env:CARGO_ZIGBUILD_ZIG_PATH = $zigPath
    if ($Mode -eq 'check') {
        cargo-zigbuild check --workspace --locked --target x86_64-pc-windows-gnu
    } else {
        switch ($Mode) {
            'host' { cargo-zigbuild zigbuild --release --locked --target x86_64-pc-windows-gnu -p kindlia-app }
            'kindle' { cargo-zigbuild zigbuild --release --locked --target armv7-unknown-linux-musleabihf -p kindlia-app }
            'host-test' { cargo-zigbuild test --locked --target x86_64-pc-windows-gnu --workspace }
        }
    }
    if ($LASTEXITCODE -ne 0) { throw "构建失败: $LASTEXITCODE" }
    if ($Mode -eq 'kindle') {
        $sysroot = rustc --print sysroot
        $objcopy = Get-ChildItem "$sysroot/lib/rustlib/*/bin/*objcopy*" | Select-Object -First 1
        if ($objcopy) {
            $binary = 'target/armv7-unknown-linux-musleabihf/release/kindlia'
            & $objcopy.FullName --strip-debug $binary "$binary.stripped"
            if ($LASTEXITCODE -ne 0) { throw '去除调试信息失败' }
            Move-Item -LiteralPath "$binary.stripped" -Destination $binary -Force
        } else { Write-Warning '未找到 objcopy，保留调试信息' }
    }
} finally { Pop-Location }
