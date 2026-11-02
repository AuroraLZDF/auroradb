# AuroraDB MySQL Client Windows 打包脚本
# 与 Linux 侧 tools/build/linux.sh 的步骤保持一致：
# 版本校验 -> 前端依赖 -> 前端构建 -> Rust 环境 -> Tauri CLI -> NSIS 打包
#
# 用法:
#   .\tools\build\windows.ps1                 # 在仓库根目录执行
#   .\tools\build\windows.ps1 -Bundles nsis   # 指定打包格式（Windows 仅支持 nsis/msi）
#
# 发布请使用: .\tools\publish\windows.ps1（构建与发布分离）
#
# 前置条件:
#   1. Node.js 20+、Rust (MSVC toolchain)、VS Build Tools（含 Windows SDK）
#   2. 更新签名私钥（与 Linux 共用同一把）: %USERPROFILE%\.config\auroradb\updater.key
#      或设置环境变量 TAURI_SIGNING_PRIVATE_KEY（私钥文件内容）

param(
    [string]$Bundles = "nsis"
)

$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
Set-Location $Root

$TotalSteps = 6

function Fail([string]$Message) {
    Write-Host "错误: $Message" -ForegroundColor Red
    exit 1
}

function Require-Command([string]$Name, [string]$Hint) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        Write-Host "错误: 未找到 $Name" -ForegroundColor Red
        Write-Host $Hint
        exit 1
    }
}

Write-Host "========================================"
Write-Host "  AuroraDB MySQL Client Windows 打包脚本"
Write-Host "========================================"
Write-Host ""

# ---------- 1. 校验版本号一致 ----------
Write-Host "[1/$TotalSteps] 校验版本号一致性..." -ForegroundColor Yellow
Require-Command "node" "请安装 Node.js 20 及以上版本: https://nodejs.org/"
& node "$Root\tools\check-version.mjs"
if ($LASTEXITCODE -ne 0) {
    Fail "版本号校验未通过，请以 src-tauri\tauri.conf.json 为准统一"
}
Write-Host "  OK 版本号一致" -ForegroundColor Green

# ---------- 2. 检查前端依赖 ----------
Write-Host "[2/$TotalSteps] 检查前端依赖..." -ForegroundColor Yellow
Require-Command "npm" "请安装 Node.js（自带 npm）"
Push-Location "$Root\src-ui"
try {
    if (-not (Test-Path "node_modules")) {
        Write-Host "  安装前端依赖..."
        & npm ci
        if ($LASTEXITCODE -ne 0) { Fail "npm ci 失败" }
    }
}
finally {
    Pop-Location
}
Write-Host "  OK 前端依赖就绪" -ForegroundColor Green

# ---------- 3. 构建前端 ----------
Write-Host "[3/$TotalSteps] 构建前端..." -ForegroundColor Yellow
Push-Location "$Root\src-ui"
try {
    # vite/vue-tsc 会把警告写到 stderr，PS 5.1 在 $ErrorActionPreference="Stop" 下
    # 会将其转成 NativeCommandError 终止脚本，因此这里临时放宽策略，改由退出码判断。
    $ErrorActionPreference = "Continue"
    & npm run build
    $BuildExitCode = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    if ($BuildExitCode -ne 0) { Fail "前端构建失败" }
}
finally {
    Pop-Location
}
Write-Host "  OK 前端构建完成" -ForegroundColor Green

# ---------- 4. 检查 Rust 环境 ----------
Write-Host "[4/$TotalSteps] 检查 Rust 环境..." -ForegroundColor Yellow
Require-Command "rustc" "请安装 Rust: https://rustup.rs/ （需 MSVC toolchain 与 VS Build Tools）"
Require-Command "cargo" "请安装 Rust: https://rustup.rs/"
Write-Host "  OK Rust 环境正常" -ForegroundColor Green

# ---------- 5. 检查 Tauri CLI ----------
Write-Host "[5/$TotalSteps] 检查 Tauri CLI..." -ForegroundColor Yellow
# 优先使用项目内的 @tauri-apps/cli（第 2 步已随前端依赖安装，版本与 package.json 一致），
# 避免在受限环境（如开启 Smart App Control 的机器）因 cargo install 被策略拦截而无法打包。
$LocalTauriJs = Join-Path $Root "src-ui\node_modules\@tauri-apps\cli\tauri.js"
if (Test-Path $LocalTauriJs) {
    $TauriCmd = "node"
    $TauriPrefixArgs = @($LocalTauriJs)
    Write-Host "  使用项目内 Tauri CLI: $LocalTauriJs"
}
else {
    # `cargo tauri` 未安装时会把错误写到 stderr，PS 5.1 在 $ErrorActionPreference="Stop" 下
    # 会将其转成 NativeCommandError 直接终止脚本，因此这里临时放宽策略，只依据退出码判断。
    $ErrorActionPreference = "Continue"
    & cargo tauri --version *> $null
    $HasGlobalTauri = ($LASTEXITCODE -eq 0)
    $ErrorActionPreference = "Stop"

    if ($HasGlobalTauri) {
        $TauriCmd = "cargo"
        $TauriPrefixArgs = @("tauri")
        Write-Host "  使用全局 Tauri CLI"
    }
    else {
        Write-Host "  安装 Tauri CLI..."
        & cargo install tauri-cli --locked
        if ($LASTEXITCODE -ne 0) { Fail "Tauri CLI 安装失败" }
        $TauriCmd = "cargo"
        $TauriPrefixArgs = @("tauri")
    }
}
Write-Host "  OK Tauri CLI 就绪" -ForegroundColor Green

# ---------- 6. 构建 NSIS 安装包 ----------
Write-Host "[6/$TotalSteps] 构建 Windows 安装包..." -ForegroundColor Yellow
Write-Host "  打包格式: $Bundles"

# 更新包签名：与 Linux 使用同一把私钥，否则用户收到的更新包验签失败
if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
    $KeyPath = Join-Path $env:USERPROFILE ".config\auroradb\updater.key"
    if (Test-Path $KeyPath) {
        $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $KeyPath -Raw).Trim()
        Write-Host "  更新签名私钥: $KeyPath"
    }
    else {
        Fail "未找到更新签名私钥，请从 Linux 机器复制 ~/.config/auroradb/updater.key 到 $KeyPath，或设置环境变量 TAURI_SIGNING_PRIVATE_KEY"
    }
}

$SignPassword = $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
if (-not $SignPassword) {
    $PassPath = Join-Path $env:USERPROFILE ".config\auroradb\updater.password"
    if (Test-Path $PassPath) {
        $Password = (Get-Content $PassPath -Raw)
        # 空文件表示私钥未设密码；Windows 环境变量无法表示「已设置但为空」，
        # 此时不设置变量，交由下方的加密检测给出交互提示。
        if ($null -ne $Password -and $Password.Trim() -ne "") {
            $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $Password.Trim()
            $SignPassword = $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
            Write-Host "  更新签名密码: $PassPath"
        }
    }
}

# 私钥经 minisign 加密时，未提供密码会让 Tauri CLI 交互式索要，一键构建会停在这里
if (-not $SignPassword) {
    $KeyComment = ""
    try {
        $KeyComment = [Text.Encoding]::UTF8.GetString(
            [Convert]::FromBase64String($env:TAURI_SIGNING_PRIVATE_KEY))
    }
    catch {
        # 私钥格式无法解码时交给 Tauri CLI 报错，这里不做判断
    }
    if ($KeyComment -match "encrypted secret key") {
        Write-Host "  私钥已加密但未提供密码（$PassPath 不存在或为空文件）" -ForegroundColor Yellow
        Write-Host "  构建时会出现 Password: 提示，密码为空则直接按回车；" -ForegroundColor Yellow
        Write-Host "  也可把密码写入 $PassPath 或设置环境变量 TAURI_SIGNING_PRIVATE_KEY_PASSWORD 免交互" -ForegroundColor Yellow
    }
}

# 清掉上一次构建的产物，避免旧版本安装包混入发布目录
$BundleDir = Join-Path $Root "src-tauri\target\release\bundle"
$NsisDir = Join-Path $BundleDir "nsis"
if (Test-Path $NsisDir) {
    Write-Host "  清理旧安装包..."
    Get-ChildItem -Path $NsisDir -File | Remove-Item -Force -ErrorAction SilentlyContinue
}

Push-Location "$Root\src-tauri"
try {
    # Tauri CLI 会把 Info/Warn/Running/Finished 等日志写到 stderr，PS 5.1 在
    # $ErrorActionPreference="Stop" 下会将其转成 NativeCommandError 终止脚本，
    # 因此这里临时放宽策略，改由退出码判断成败。
    $ErrorActionPreference = "Continue"
    & $TauriCmd @TauriPrefixArgs build --bundles $Bundles
    $TauriExitCode = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    if ($TauriExitCode -ne 0) { Fail "打包失败" }
}
finally {
    Pop-Location
}

Write-Host ""
Write-Host "========================================" -ForegroundColor Green
Write-Host "    打包成功!" -ForegroundColor Green
Write-Host "========================================" -ForegroundColor Green
Write-Host ""

if (Test-Path $NsisDir) {
    $Installers = Get-ChildItem -Path $NsisDir -Filter *.exe -File
    if ($Installers.Count -gt 0) {
        Write-Host "安装包位置:" -ForegroundColor Green
        foreach ($Item in $Installers) {
            $SizeMb = [math]::Round($Item.Length / 1MB, 1)
            Write-Host ("  {0} ({1} MB)" -f $Item.FullName, $SizeMb)
        }
        Write-Host ""
    }
    else {
        Write-Host "未生成 NSIS 安装包，请检查上方构建输出" -ForegroundColor Yellow
    }
}

Write-Host "注意: 当前版本未做代码签名，安装时 SmartScreen 会提示未知发布者，"
Write-Host "      需点击「更多信息 -> 仍要运行」。企业终端可能被组策略直接拦截。"
Write-Host ""
Write-Host "下一步: 发布到 Gitee 发行版"
Write-Host "  .\tools\publish\windows.ps1"
Write-Host ""
