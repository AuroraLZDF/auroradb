# 将 Windows 构建产物上传到 GitHub 发行版，并把 windows-x86_64 条目合并进 latest.json
#
# 用法（在仓库根目录执行）:
#   .\tools\publish\windows.ps1                          # 读 tauri.conf.json 的版本，默认发到 origin 同名仓库
#   .\tools\publish\windows.ps1 -Tag v2.0.0 -Notes "..." # 指定 tag 与发行说明
#   .\tools\publish\windows.ps1 -Repo owner/repo
#
# 前置: 先执行 .\tools\build\windows.ps1 生成并签名 NSIS 安装包
# 令牌来源: 环境变量 GITHUB_TOKEN / GH_TOKEN，或 %USERPROFILE%\.config\auroradb\github.token
# 依赖: curl.exe（Windows 10 1803+ 自带）；PowerShell 5.1 无 Invoke-RestMethod -Form，故用 curl 上传

param(
    [string]$Tag,
    [string]$Notes,
    [string]$Repo,
    [string]$BundleDir,
    [switch]$Yes   # 跳过交互确认（CI 用）
)

$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$Api = "https://api.github.com/repos"
$UploadApi = "https://uploads.github.com/repos"
$MaxSize = 2GB

$script:LastCode = ""

function Fail([string]$Message) {
    Write-Host "错误: $Message" -ForegroundColor Red
    exit 1
}

function Show-NetworkHint([string]$Url) {
    Write-Host "  ✗ 无法连接 GitHub（$Url）" -ForegroundColor Red
    Write-Host "    这不是令牌问题。先确认本机能访问 api.github.com："
    Write-Host "      curl.exe -sS -o NUL -w ""%{http_code}"" https://api.github.com/"
    Write-Host "    返回 000 或 TLS 握手失败，通常是代理/VPN 未放行 github.com，切换可用节点后重试。"
}

function Invoke-Curl([string[]]$CurlArgs) {
    # curl 失败信息写在 stderr，PS 5.1 在 Stop 策略下会转成 NativeCommandError，这里只依据退出码判断
    $Previous = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $Output = & curl.exe @CurlArgs
    $ExitCode = $LASTEXITCODE
    $ErrorActionPreference = $Previous
    if ($ExitCode -ne 0) {
        Show-NetworkHint $CurlArgs[-1]
        Fail "curl 执行失败（退出码 $ExitCode）"
    }
    return ($Output | Out-String).Trim()
}

function Read-Utf8([string]$Path) {
    # PS 5.1 的 Get-Content 默认按 ANSI 解码，接口响应里的中文会变成乱码并破坏 JSON
    return [IO.File]::ReadAllText($Path, [Text.Encoding]::UTF8)
}

function Get-GitOutput([string[]]$GitArgs) {
    # git 的警告/错误写在 stderr，PS 5.1 在 Stop 策略下会转成 NativeCommandError，这里只依据退出码
    $Previous = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $Output = & git -C $Root @GitArgs 2>$null
    $ExitCode = $LASTEXITCODE
    $ErrorActionPreference = $Previous
    if ($ExitCode -ne 0) { return $null }
    return (($Output | Out-String).Trim())
}

# 调用 GitHub API：响应写入临时文件，HTTP 状态码放进 $script:LastCode
function Invoke-Api([string]$Method, [string]$Url, [string[]]$ExtraArgs = @()) {
    $CurlArgs = @(
        "-sS", "-X", $Method, "-o", $Tmp, "-w", "%{http_code}",
        "-H", "Authorization: Bearer $Token",
        "-H", "Accept: application/vnd.github+json",
        "-H", "X-GitHub-Api-Version: 2022-11-28"
    ) + $ExtraArgs + @($Url)
    $script:LastCode = Invoke-Curl $CurlArgs
}

if (-not (Get-Command "curl.exe" -ErrorAction SilentlyContinue)) {
    Fail "未找到 curl.exe（Windows 10 1803 及以上版本自带）"
}

$Token = $env:GITHUB_TOKEN
if (-not $Token) { $Token = $env:GH_TOKEN }
if (-not $Token) {
    $TokenPath = Join-Path $env:USERPROFILE ".config\auroradb\github.token"
    if (Test-Path $TokenPath) {
        $Token = (Get-Content $TokenPath -Raw).Trim()
        Write-Host "  GitHub 令牌: $TokenPath"
    }
}
if (-not $Token) {
    Fail "未设置 GITHUB_TOKEN，或把令牌写入 $env:USERPROFILE\.config\auroradb\github.token"
}

$ConfigPath = Join-Path $Root "src-tauri\tauri.conf.json"
if (-not (Test-Path $ConfigPath)) { Fail "未找到 $ConfigPath" }
$Version = (Read-Utf8 $ConfigPath | ConvertFrom-Json).version
if (-not $Tag) { $Tag = "v$Version" }
if (-not $Notes) { $Notes = "AuroraDB $Version 版本发布" }

if (-not $Repo) {
    $Previous = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $RemoteUrl = (& git -C $Root remote get-url origin 2>$null | Select-Object -First 1)
    $ErrorActionPreference = $Previous
    $Match = [regex]::Match("$RemoteUrl", "[:/]([^/:]+)/([^/]+?)(?:\.git)?/?$")
    if (-not $Match.Success) { Fail "无法从 origin 解析仓库地址: $RemoteUrl" }
    $Repo = "$($Match.Groups[1].Value)/$($Match.Groups[2].Value)"
}
if ($Repo -notmatch "^[^/]+/[^/]+$") { Fail "-Repo 需为 owner/repo 格式，当前为 $Repo" }

$ApiUrl = "$Api/$Repo"
if (-not $BundleDir) { $BundleDir = Join-Path $Root "src-tauri\target\release\bundle" }
$NsisDir = Join-Path $BundleDir "nsis"

$Installers = @(Get-ChildItem -Path $NsisDir -Filter "*-setup.exe" -File -ErrorAction SilentlyContinue)
if ($Installers.Count -eq 0) {
    Fail "未找到 NSIS 安装包（$NsisDir\*-setup.exe），请先执行 .\tools\build\windows.ps1"
}
$Installer = $Installers[0]
$SigPath = "$($Installer.FullName).sig"
if (-not (Test-Path $SigPath)) {
    Fail "未找到 $($Installer.Name).sig，请确认构建时已签名"
}
# 记录安装包 SHA-256：写入 latest.json 的 windows-x86_64 条目，供人工与工具核对下载完整性
# （Tauri 更新器只读 url/signature，多出的字段会被忽略，不影响旧客户端）
$InstallerSha256 = (Get-FileHash -Path $Installer.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
if (-not $InstallerSha256 -or $InstallerSha256.Length -ne 64) {
    Fail "计算 $($Installer.Name) 的 SHA-256 失败"
}
if ($Installer.Length -gt $MaxSize) {
    Fail "$($Installer.Name) 超过 GitHub 单附件 2GB 上限"
}

# 发布前人工确认：上传后发行版与 latest.json 立即对用户生效，先让发版人核对代码状态
Write-Host ""
Write-Host "========================================"
Write-Host "  发布前确认"
Write-Host "========================================"
Write-Host "  版本: $Version"
Write-Host "  标签: $Tag"
Write-Host "  平台: windows"

$Branch = Get-GitOutput @("rev-parse", "--abbrev-ref", "HEAD")
if (-not $Branch) { $Branch = "unknown" }
$Commit = Get-GitOutput @("log", "-1", "--oneline")
Write-Host "  分支: $Branch"
Write-Host "  提交: $Commit"

if (Get-GitOutput @("status", "--porcelain")) {
    Write-Host "  ! 工作区有未提交的改动，确认这些改动不影响本次发版" -ForegroundColor Yellow
}
else {
    Write-Host "  OK 工作区干净" -ForegroundColor Green
}

$Upstream = Get-GitOutput @("rev-parse", "--abbrev-ref", "--symbolic-full-name", '@{u}')
if ($Upstream) {
    $Ahead = 0
    [void][int]::TryParse((Get-GitOutput @("rev-list", "--count", "$Upstream..HEAD")), [ref]$Ahead)
    if ($Ahead -gt 0) {
        Write-Host "  ! 有 $Ahead 个提交未推送到 $Upstream" -ForegroundColor Yellow
        Write-Host "    推送: git push origin $Branch"
    }
    else {
        Write-Host "  OK 代码已推送到 $Upstream" -ForegroundColor Green
    }
}
else {
    Write-Host "  ! 当前分支未设置上游分支，无法判断是否已推送" -ForegroundColor Yellow
}

$TagCommit = Get-GitOutput @("rev-list", "-n1", $Tag)
if ($TagCommit) {
    $HeadCommit = Get-GitOutput @("rev-parse", "HEAD")
    if ($HeadCommit -and $TagCommit -ne $HeadCommit) {
        $TagShort = $TagCommit.Substring(0, 7)
        $HeadShort = $HeadCommit.Substring(0, 7)
        Write-Host "  ! 本地 tag $Tag 指向 $TagShort，不是当前 HEAD $HeadShort" -ForegroundColor Yellow
    }
    if (Get-GitOutput @("ls-remote", "--tags", "origin", "refs/tags/$Tag")) {
        Write-Host "  OK tag $Tag 已推送到远程" -ForegroundColor Green
    }
    else {
        Write-Host "  ! 本地 tag $Tag 尚未推送到 origin" -ForegroundColor Yellow
        Write-Host "    推送: git push origin $Tag"
    }
}
else {
    Write-Host "  ! 本地不存在 tag $Tag" -ForegroundColor Yellow
    Write-Host "    创建并推送: git tag $Tag; git push origin $Tag"
}

Write-Host ""
if ($Yes) {
    Write-Host "  -Yes: 跳过交互确认（CI 用）" -ForegroundColor Yellow
}
else {
    Write-Host "  注意: 继续后将上传安装包并更新 latest.json，对终端用户立即可见。"
    $Confirm = Read-Host "  确认继续？输入 yes 继续，其他任意输入取消"
    if ($Confirm -ne "yes") {
        Write-Host "已取消发布"
        exit 0
    }
}
Write-Host ""

Write-Host "  发布仓库: $Repo  版本: $Tag"

$Tmp = [IO.Path]::GetTempFileName()

# 0. 校验发布仓库可访问，并取默认分支 / 规范仓库名
Invoke-Api "GET" $ApiUrl
if ($script:LastCode -ne "200") {
    Write-Host (Read-Utf8 $Tmp)
    Fail "无法访问发布仓库 $Repo (HTTP $($script:LastCode))，请确认仓库公开且令牌有 Contents: Read and write 权限"
}
$RepoInfo = Read-Utf8 $Tmp | ConvertFrom-Json
$Branch = if ($RepoInfo.default_branch) { $RepoInfo.default_branch } else { "main" }
if ($RepoInfo.full_name) {
    $Repo = $RepoInfo.full_name
    $ApiUrl = "$Api/$Repo"
}
$DownloadPrefix = "https://github.com/$Repo/releases/download/$Tag/"

# latest.json 的固定入口是 /releases/latest，若已存在更高版本的发行版，
# 重新发布旧 tag 会让所有客户端读到回退后的清单，这里直接中止
Invoke-Api "GET" "$ApiUrl/releases/latest"
if ($script:LastCode -eq "200") {
    $LatestTagName = (Read-Utf8 $Tmp | ConvertFrom-Json).tag_name
    if ($LatestTagName -and $LatestTagName -ne $Tag) {
        $LatestVersion = $null
        $OursVersion = $null
        [void][version]::TryParse("$LatestTagName".TrimStart("v"), [ref]$LatestVersion)
        [void][version]::TryParse("$Tag".TrimStart("v"), [ref]$OursVersion)
        if ($LatestVersion -and $OursVersion -and $LatestVersion -gt $OursVersion) {
            Fail "线上最新发行版是 $LatestTagName，高于本次 $Tag；latest.json 通过 /releases/latest 分发，发布旧版本会让客户端清单回退"
        }
    }
}

# 1. 查找已有发行版（不存在返回 404），否则创建
Invoke-Api "GET" "$ApiUrl/releases/tags/$Tag"
$ReleaseId = ""
if ($script:LastCode -eq "200") {
    $ReleaseId = (Read-Utf8 $Tmp | ConvertFrom-Json).id
    Write-Host "  发行版已存在 (id=$ReleaseId)，追加附件" -ForegroundColor Green
}
elseif ($script:LastCode -ne "404") {
    Write-Host (Read-Utf8 $Tmp)
    Fail "查询发行版失败 (HTTP $($script:LastCode))"
}

if (-not $ReleaseId) {
    $BodyPath = [IO.Path]::GetTempFileName()
    $Body = @{
        tag_name         = $Tag
        name             = "AuroraDB $Version"
        body             = $Notes
        target_commitish = $Branch
        draft            = $false
        prerelease       = $false
    } | ConvertTo-Json -Depth 5
    $Utf8NoBom = New-Object Text.UTF8Encoding($false)
    [IO.File]::WriteAllText($BodyPath, $Body, $Utf8NoBom)
    Invoke-Api "POST" "$ApiUrl/releases" @("-H", "Content-Type: application/json", "--data-binary", "@$BodyPath")
    Remove-Item $BodyPath -Force -ErrorAction SilentlyContinue
    if ($script:LastCode -ne "201") {
        Write-Host (Read-Utf8 $Tmp)
        Fail "创建发行版失败 (HTTP $($script:LastCode))"
    }
    $ReleaseId = (Read-Utf8 $Tmp | ConvertFrom-Json).id
    Write-Host "  已创建发行版 AuroraDB $Version (id=$ReleaseId)" -ForegroundColor Green
}
if (-not $ReleaseId) { Fail "无法解析发行版 id" }

# 2. 列出现有附件，同名附件先删除，重复发布时覆盖旧产物
$Existing = @{}
Invoke-Api "GET" "$ApiUrl/releases/$ReleaseId/assets?per_page=100"
if ($script:LastCode -eq "200") {
    # PS 5.1 的 ConvertFrom-Json 把 JSON 数组当作单个对象输出，直接 @() 包管道会得到
    # 一个「元素是数组」的 1 元数组，导致所有附件都匹配不到、重复上传报 422。
    $Assets = Read-Utf8 $Tmp | ConvertFrom-Json
    foreach ($Item in $Assets) {
        if ($Item.name) { $Existing[$Item.name] = $Item.id }
    }
}

# 先下载线上 latest.json：合并需要旧内容，且同名附件会在上传前被删除
$LatestExisting = Join-Path ([IO.Path]::GetTempPath()) "auroradb-latest-existing.json"
$ExistingDoc = $null
$LatestAssetId = $Existing["latest.json"]
if ($LatestAssetId) {
    Invoke-Curl @(
        "-sS", "-L", "-o", $LatestExisting, "-H", "Authorization: Bearer $Token",
        "-H", "Accept: application/octet-stream", "$ApiUrl/releases/assets/$LatestAssetId"
    ) | Out-Null
    try {
        $RawLatest = (Read-Utf8 $LatestExisting).Trim()
        if ($RawLatest -and $RawLatest -ne "null") { $ExistingDoc = $RawLatest | ConvertFrom-Json }
    }
    catch {
        Fail "线上 latest.json 不是合法 JSON，中止发布"
    }
    if (-not $ExistingDoc) { Fail "下载线上 latest.json 失败，中止发布（避免丢失其他平台的更新条目）" }
    Invoke-Api "DELETE" "$ApiUrl/releases/assets/$LatestAssetId"
    if ($script:LastCode -ne "204") {
        Fail "删除旧 latest.json 附件失败 (HTTP $($script:LastCode))"
    }
}
Remove-Item $LatestExisting -Force -ErrorAction SilentlyContinue

# 3. 上传安装包与更新签名
$Uploaded = 0
foreach ($File in @($Installer, (Get-Item $SigPath))) {
    if ($Existing.ContainsKey($File.Name)) {
        Invoke-Api "DELETE" "$ApiUrl/releases/assets/$($Existing[$File.Name])"
        if ($script:LastCode -ne "204") { Fail "删除同名旧附件 $($File.Name) 失败 (HTTP $($script:LastCode))" }
        Write-Host "  - 已删除同名旧附件 $($File.Name)" -ForegroundColor Yellow
    }

    $EncodedName = [Uri]::EscapeDataString($File.Name)
    $script:LastCode = Invoke-Curl @(
        "-sS", "--retry", "3", "--retry-delay", "2", "--retry-all-errors",
        "--connect-timeout", "30", "--max-time", "1800",
        "-X", "POST", "-o", $Tmp, "-w", "%{http_code}",
        "-H", "Authorization: Bearer $Token",
        "-H", "Accept: application/vnd.github+json",
        "-H", "X-GitHub-Api-Version: 2022-11-28",
        "-H", "Content-Type: application/octet-stream",
        "--data-binary", "@$($File.FullName)",
        "$UploadApi/$Repo/releases/$ReleaseId/assets?name=$EncodedName"
    )
    if ($script:LastCode -ne "201") {
        Write-Host (Read-Utf8 $Tmp)
        Fail "$($File.Name) 上传失败 (HTTP $($script:LastCode))"
    }
    $Uploaded++
    Write-Host ("  OK {0} ({1} MB)" -f $File.Name, [math]::Round($File.Length / 1MB, 1)) -ForegroundColor Green
}

Write-Host "  完成: $Uploaded 个文件已发布到 $Tag" -ForegroundColor Green

# 4. 合并 latest.json：只保留本 tag 的平台条目，其余平台（如 Linux）原样保留
$Platforms = [ordered]@{}
$PreviousDoc = $null
if ($ExistingDoc -and $ExistingDoc.platforms) {
    foreach ($Entry in $ExistingDoc.platforms.PSObject.Properties) {
        if ($Entry.Value.url -and $Entry.Value.url -like "*/download/$Tag/*") {
            $Platforms[$Entry.Name] = $Entry.Value
        }
    }
    if ($ExistingDoc.version -and $ExistingDoc.version -ne $Version) {
        $PreviousDoc = @{ version = $ExistingDoc.version; platforms = $ExistingDoc.platforms }
    }
}

$Signature = (Read-Utf8 $SigPath).Trim()
if (-not $Signature) { Fail "签名文件为空: $SigPath" }

$Platforms["windows-x86_64"] = [ordered]@{
    signature = $Signature
    url       = "$DownloadPrefix$($Installer.Name)"
    sha256    = $InstallerSha256
}

$Doc = [ordered]@{
    version   = $Version
    notes     = $Notes
    pub_date  = [DateTime]::UtcNow.ToString("yyyy-MM-dd'T'HH:mm:ss'Z'")
    platforms = $Platforms
}
if ($PreviousDoc) { $Doc.Add("previous", $PreviousDoc) }
# Linux 先发布时清单里有 deb 条目，Windows 单独重发时不能把它丢掉
if ($ExistingDoc -and $ExistingDoc.deb) { $Doc.Add("deb", $ExistingDoc.deb) }

$LatestPath = Join-Path ([IO.Path]::GetTempPath()) "auroradb-latest.json"
$Utf8NoBom = New-Object Text.UTF8Encoding($false)
[IO.File]::WriteAllText($LatestPath, ($Doc | ConvertTo-Json -Depth 10), $Utf8NoBom)

$script:LastCode = Invoke-Curl @(
    "-sS", "--retry", "3", "--retry-delay", "2", "--retry-all-errors",
    "--connect-timeout", "30", "--max-time", "300",
    "-X", "POST", "-o", $Tmp, "-w", "%{http_code}",
    "-H", "Authorization: Bearer $Token",
    "-H", "Accept: application/vnd.github+json",
    "-H", "X-GitHub-Api-Version: 2022-11-28",
    "-H", "Content-Type: application/octet-stream",
    "--data-binary", "@$LatestPath",
    "$UploadApi/$Repo/releases/$ReleaseId/assets?name=latest.json"
)
if ($script:LastCode -ne "201") {
    Write-Host (Read-Utf8 $Tmp)
    Fail "自动更新清单上传失败 (HTTP $($script:LastCode))"
}
Remove-Item $LatestPath -Force -ErrorAction SilentlyContinue

$LatestUrl = "https://github.com/$Repo/releases/latest/download/latest.json"
Write-Host "  自动更新清单已上传: $LatestUrl" -ForegroundColor Green
Write-Host ""
Write-Host "发布完成: https://github.com/$Repo/releases/tag/$Tag" -ForegroundColor Green
