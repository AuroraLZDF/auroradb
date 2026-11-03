# 将 Windows 构建产物上传到 Gitee 发行版，并把 windows-x86_64 条目合并进 latest.json
#
# 用法（在仓库根目录执行）:
#   .\tools\publish\windows.ps1                          # 读 tauri.conf.json 的版本，默认发到 <owner>/auroradb-releases
#   .\tools\publish\windows.ps1 -Tag v2.0.0 -Notes "..." # 指定 tag 与发行说明
#   .\tools\publish\windows.ps1 -Repo owner/auroradb-releases
#
# 前置: 先执行 .\tools\build\windows.ps1 生成并签名 NSIS 安装包
# 令牌来源: 环境变量 GITEE_TOKEN，或 %USERPROFILE%\.config\auroradb\gitee.token
# 依赖: curl.exe（Windows 10 1803+ 自带）；PowerShell 5.1 无 Invoke-RestMethod -Form，故用 curl 上传

param(
    [string]$Tag,
    [string]$Notes,
    [string]$Repo,
    [string]$BundleDir
)

$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$Api = "https://gitee.com/api/v5/repos"
$MaxSize = 100 * 1024 * 1024

function Fail([string]$Message) {
    Write-Host "错误: $Message" -ForegroundColor Red
    exit 1
}

function Invoke-Curl([string[]]$CurlArgs) {
    # curl 失败信息写在 stderr，PS 5.1 在 Stop 策略下会转成 NativeCommandError，这里只依据退出码判断
    $Previous = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $Output = & curl.exe @CurlArgs
    $ExitCode = $LASTEXITCODE
    $ErrorActionPreference = $Previous
    if ($ExitCode -ne 0) { Fail "curl 执行失败（退出码 $ExitCode）" }
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

if (-not (Get-Command "curl.exe" -ErrorAction SilentlyContinue)) {
    Fail "未找到 curl.exe（Windows 10 1803 及以上版本自带）"
}

$Token = $env:GITEE_TOKEN
if (-not $Token) {
    $TokenPath = Join-Path $env:USERPROFILE ".config\auroradb\gitee.token"
    if (Test-Path $TokenPath) {
        $Token = (Get-Content $TokenPath -Raw).Trim()
        Write-Host "  Gitee 令牌: $TokenPath"
    }
}
if (-not $Token) {
    Fail "未设置 GITEE_TOKEN，或把令牌写入 $env:USERPROFILE\.config\auroradb\gitee.token"
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
    $Match = [regex]::Match("$RemoteUrl", "gitee\.com[:/]+([^/]+)/([^/]+?)(?:\.git)?/?$")
    if (-not $Match.Success) { Fail "无法从 origin 解析仓库地址: $RemoteUrl" }
    $Repo = "$($Match.Groups[1].Value)/auroradb-releases"
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
        Write-Host "  ! 本地 tag $Tag 尚未推送到远程" -ForegroundColor Yellow
        Write-Host "    推送: git push origin $Tag"
    }
}
else {
    Write-Host "  ! 本地不存在 tag $Tag" -ForegroundColor Yellow
    Write-Host "    创建并推送: git tag $Tag; git push origin $Tag"
}

Write-Host ""
Write-Host "  注意: 继续后将上传安装包并更新 latest.json，对终端用户立即可见。"
$Confirm = Read-Host "  确认继续？输入 yes 继续，其他任意输入取消"
if ($Confirm -ne "yes") {
    Write-Host "已取消发布"
    exit 0
}
Write-Host ""

Write-Host "  发布仓库: $Repo  版本: $Tag"

$Tmp = [IO.Path]::GetTempFileName()

# 0. 校验发布仓库可访问，并取默认分支（写入 latest.json 用）
$Code = Invoke-Curl @("-sS", "-o", $Tmp, "-w", "%{http_code}", "$ApiUrl`?access_token=$Token")
if ($Code -ne "200") {
    Fail "无法访问发布仓库 $Repo (HTTP $Code)，请确认仓库存在且令牌有 projects 权限"
}
$RepoInfo = Read-Utf8 $Tmp | ConvertFrom-Json
$Branch = if ($RepoInfo.default_branch) { $RepoInfo.default_branch } else { "main" }

# 1. 查找已有发行版（Gitee 在不存在时返回 200 + body null），不存在则创建
$Code = Invoke-Curl @("-sS", "-o", $Tmp, "-w", "%{http_code}", "$ApiUrl/releases/tags/$Tag`?access_token=$Token")
$ReleaseId = ""
if ($Code -eq "200") {
    $Raw = (Read-Utf8 $Tmp).Trim()
    if ($Raw -and $Raw -ne "null") { $ReleaseId = (ConvertFrom-Json $Raw).id }
}

if ($ReleaseId) {
    Write-Host "  发行版已存在 (id=$ReleaseId)，追加附件" -ForegroundColor Green
}
else {
    $Code = Invoke-Curl @(
        "-sS", "-o", $Tmp, "-w", "%{http_code}",
        "-X", "POST", "$ApiUrl/releases?access_token=$Token",
        "--data-urlencode", "tag_name=$Tag",
        "--data-urlencode", "name=AuroraDB $Version",
        "--data-urlencode", "body=$Notes",
        "--data-urlencode", "target_commitish=$Branch"
    )
    if ($Code -ne "201") {
        Write-Host (Read-Utf8 $Tmp)
        Fail "创建发行版失败 (HTTP $Code)"
    }
    $ReleaseId = (Read-Utf8 $Tmp | ConvertFrom-Json).id
    Write-Host "  已创建发行版 AuroraDB $Version (id=$ReleaseId)" -ForegroundColor Green
}
if (-not $ReleaseId) { Fail "无法解析发行版 id" }

# 2. 列出现有附件，同名附件先删除，重复发布时覆盖旧产物
$Existing = @{}
$Code = Invoke-Curl @("-sS", "-o", $Tmp, "-w", "%{http_code}", "$ApiUrl/releases/$ReleaseId/attach_files?access_token=$Token")
if ($Code -eq "200") {
    foreach ($Item in @(Read-Utf8 $Tmp | ConvertFrom-Json)) {
        if ($Item.name) { $Existing[$Item.name] = $Item.id }
    }
}

# 3. 上传安装包与更新签名
$Uploaded = 0
foreach ($File in @($Installer, (Get-Item $SigPath))) {
    if ($File.Length -gt $MaxSize) {
        Fail "$($File.Name) 超过 Gitee 单附件上限 100MB"
    }
    if ($Existing.ContainsKey($File.Name)) {
        $Code = Invoke-Curl @(
            "-sS", "-o", $Tmp, "-w", "%{http_code}",
            "-X", "DELETE", "$ApiUrl/releases/$ReleaseId/attach_files/$($Existing[$File.Name])?access_token=$Token"
        )
        if ($Code -ne "204" -and $Code -ne "200") { Fail "删除同名旧附件 $($File.Name) 失败 (HTTP $Code)" }
        Write-Host "  - 已删除同名旧附件 $($File.Name)" -ForegroundColor Yellow
    }

    $Code = Invoke-Curl @(
        "-sS", "--retry", "3", "--retry-delay", "2", "--retry-all-errors",
        "--connect-timeout", "30", "--max-time", "600",
        "-X", "POST", "-o", $Tmp, "-w", "%{http_code}",
        "$ApiUrl/releases/$ReleaseId/attach_files?access_token=$Token",
        "-F", "file=@$($File.FullName)"
    )
    if ($Code -ne "201") {
        Write-Host (Read-Utf8 $Tmp)
        Fail "$($File.Name) 上传失败 (HTTP $Code)"
    }
    $Uploaded++
    Write-Host ("  OK {0} ({1} MB)" -f $File.Name, [math]::Round($File.Length / 1MB, 1)) -ForegroundColor Green
}

Write-Host "  完成: $Uploaded 个文件已发布到 $Tag" -ForegroundColor Green

# 4. 合并 latest.json：只保留本 tag 的平台条目，其余平台（如 Linux）原样保留
$Code = Invoke-Curl @("-sS", "-o", $Tmp, "-w", "%{http_code}", "$ApiUrl/contents/latest.json?access_token=$Token&ref=$Branch")
$Sha = ""
$ExistingDoc = $null
if ($Code -eq "200") {
    $Resp = Read-Utf8 $Tmp | ConvertFrom-Json
    $Sha = $Resp.sha
    if ($Resp.content) {
        $Decoded = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($Resp.content)).Trim()
        if ($Decoded -and $Decoded -ne "null") { $ExistingDoc = $Decoded | ConvertFrom-Json }
    }
}

$Platforms = [ordered]@{}
if ($ExistingDoc -and $ExistingDoc.platforms) {
    foreach ($Entry in $ExistingDoc.platforms.PSObject.Properties) {
        if ($Entry.Value.url -and $Entry.Value.url -like "*/download/$Tag/*") {
            $Platforms[$Entry.Name] = $Entry.Value
        }
    }
}
$Platforms["windows-x86_64"] = [ordered]@{
    signature = (Read-Utf8 $SigPath).Trim()
    url = "https://gitee.com/$Repo/releases/download/$Tag/$($Installer.Name)"
    sha256 = $InstallerSha256
}

$Doc = [ordered]@{
    version = $Version
    notes = $Notes
    pub_date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = $Platforms
}
if ($ExistingDoc -and $ExistingDoc.version -and $ExistingDoc.version -ne $Version) {
    $Doc["previous"] = [ordered]@{
        version = $ExistingDoc.version
        platforms = $ExistingDoc.platforms
    }
}
# Linux 先发布时清单里有 deb 条目，Windows 后发布不能把它丢掉
if ($ExistingDoc -and $ExistingDoc.deb -and $ExistingDoc.deb.url -like "*/download/$Tag/*") {
    $Doc["deb"] = $ExistingDoc.deb
}

$Json = $Doc | ConvertTo-Json -Depth 10
$B64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Json))

# 文件已存在时必须走 PUT 并带上当前 sha，否则 Gitee 拒绝覆盖
$Method = if ($Sha) { "PUT" } else { "POST" }
$CurlArgs = @(
    "-sS", "-o", $Tmp, "-w", "%{http_code}",
    "-X", $Method, "$ApiUrl/contents/latest.json",
    "--data-urlencode", "access_token=$Token",
    "--data-urlencode", "content=$B64",
    "--data-urlencode", "message=chore(release): latest.json $Tag (windows-x86_64)",
    "--data-urlencode", "branch=$Branch"
)
if ($Sha) { $CurlArgs += @("--data-urlencode", "sha=$Sha") }
$Code = Invoke-Curl $CurlArgs
if ($Code -ne "200" -and $Code -ne "201") {
    Write-Host (Read-Utf8 $Tmp)
    Fail "写入 latest.json 失败 (HTTP $Code)"
}

$Kept = @($Platforms.Keys | Where-Object { $_ -ne "windows-x86_64" })
$KeptText = if ($Kept.Count -gt 0) { "（保留 $($Kept -join ', ')）" } else { "（无其他平台）" }
Write-Host "  清单平台: windows-x86_64$KeptText" -ForegroundColor Green
Write-Host "  发行版: https://gitee.com/$Repo/releases/tag/$Tag" -ForegroundColor Green

Remove-Item $Tmp -Force -ErrorAction SilentlyContinue
