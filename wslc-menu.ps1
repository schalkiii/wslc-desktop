#Requires -Version 5.1
<#
.SYNOPSIS
    WSLC 容器交互式菜单启动脚本 (v2 简化版)
.DESCRIPTION
    列出所有容器，用序号选择要启动/停止的容器
    镜像已预设加速前缀，wslc 直接拉取即可
    wslc 不支持的参数: --restart, --privileged, --hostname(用 -h 代替)
#>

param(
    [switch]$Pull
)

$ErrorActionPreference = "Continue"

# ============================================================
# 当前使用的 Docker Hub 加速前缀 (仅用于显示)
# ============================================================
$Script:DockerHubMirrors = @(
    @{ Prefix = "docker.1panel.live"; Desc = "1Panel (推荐)" }
    @{ Prefix = "docker.1ms.run";     Desc = "毫秒镜像" }
    @{ Prefix = "docker.m.daocloud.io"; Desc = "DaoCloud" }
    @{ Prefix = "docker.xuanyuan.me"; Desc = "轩辕镜像" }
)

$Script:MirrorIndex = 0  # 默认 docker.1panel.live

# ============================================================
# 容器定义
# ============================================================
$Containers = @(
    @{ Name="qinglong";              Port="8383";  Desc="青龙面板";                 Image="docker.1panel.live/whyour/qinglong:latest";                  Cmd='wslc run -d -v "C:\docker\qinglong\data:/ql/data" -p 8383:5700 -e QlBaseUrl="/" -e QlPort="5700" --name qinglong -h qinglong docker.1panel.live/whyour/qinglong:latest' }
    @{ Name="cookiecloud";           Port="8082";  Desc="Cookie Cloud";             Image="docker.1panel.live/easychen/cookiecloud:latest";              Cmd='wslc run -d -p 8082:8088 --name cookiecloud -e API_ROOT=/cookie docker.1panel.live/easychen/cookiecloud:latest' }
    @{ Name="cross-seed";            Port="2468";  Desc="Cross-Seed 辅种";          Image="docker.1panel.live/crossseed/cross-seed:latest";              Cmd='wslc run -d --name cross-seed -p 2468:2468 -v "C:\docker\cross-seed\config:/config" docker.1panel.live/crossseed/cross-seed:latest daemon' }
    @{ Name="network-panel";         Port="8080";  Desc="Net Panel 刷流"; Image="docker.1panel.live/netart/network-panel:latest";       Cmd='wslc run -d --name network-panel -p 8080:80 docker.1panel.live/netart/network-panel:latest' }
    @{ Name="openspeedtest";         Port="4000";  Desc="OpenSpeedTest 测速";       Image="docker.1panel.live/openspeedtest/latest";                     Cmd='wslc run --name openspeedtest -d -p 4000:3000 -p 4001:3001 docker.1panel.live/openspeedtest/latest' }
    @{ Name="bili_tool_web";         Port="2233";  Desc="B站工具箱";                Image="ghcr.nju.edu.cn/raywangqvq/bili_tool_web";                   Cmd='wslc run -d --name bili_tool_web -t -v "C:\docker\bili_tool_web\Logs:/app/Logs" -v "C:\docker\bili_tool_web\config:/app/config" -p 2233:8080 -e TZ=Asia/Shanghai -e DailyTaskConfig__Cron="0 0 15 * * ?" ghcr.nju.edu.cn/raywangqvq/bili_tool_web' }
    @{ Name="jackett";               Port="9117";  Desc="Jackett 索引器";           Image="docker.1panel.live/linuxserver/jackett:latest";                                  Cmd='wslc run -d --name jackett -e PUID=1000 -e PGID=1000 -e TZ=Etc/UTC -e AUTO_UPDATE=true -p 9117:9117 -v "C:\docker\jackett\config:/config" -v "C:\docker\jackett\downloads:/downloads" -v "C:\docker\mi-gpt\resolv.conf:/etc/resolv.conf" docker.1panel.live/linuxserver/jackett:latest' }
    @{ Name="home-assistant";        Port="8123";  Desc="Home Assistant";           Image="docker.1panel.live/homeassistant/home-assistant";              Cmd='wslc run -d --name home-assistant -e TZ=Asia/Shanghai -v "C:\docker\home_assistant\config:/config" -p 8123:8123 docker.1panel.live/homeassistant/home-assistant' }
    @{ Name="IYUUPlus";              Port="8787";  Desc="IYUUPlus 辅种";            Image="docker.1panel.live/iyuucn/iyuuplus";                          Cmd='wslc run -d -v "C:\docker\IYUU\db:/IYUU/db" -v "C:\Users\Schal\AppData\Local\qBittorrent\BT_backup:/BT_backup" -p 8787:8787 --name IYUUPlus docker.1panel.live/iyuucn/iyuuplus' }
    @{ Name="IYUUPlus-dev";          Port="8780";  Desc="IYUUPlus 开发版";          Image="docker.1panel.live/iyuucn/iyuuplus-dev:latest";               Cmd='wslc run -itd -v "C:\docker\iyuu-dev\iyuu:/iyuu" -v "C:\docker\iyuu-dev\data:/data" -p 8780:8780 --name IYUUPlus-dev docker.1panel.live/iyuucn/iyuuplus-dev:latest' }
    @{ Name="elmmb";                 Port="3002";  Desc="饿了么点赞";               Image="docker.1panel.live/luobook/elmmb:latest";                     Cmd='wslc run -id --name elmmb -h elmmb -p 3002:3000 -v "C:\docker\elmmb:/etc/lb/Config" docker.1panel.live/luobook/elmmb:latest' }
    @{ Name="github-rss-aggregator"; Port="5000";  Desc="GitHub RSS 聚合";          Image="docker.1panel.live/library/python:3.11";                      Cmd='wslc run -d --name github-rss-aggregator --network bridge -p 5000:5000 -e TZ=Asia/Shanghai -e http_proxy=http://<proxy-host>:7890 -e https_proxy=http://<proxy-host>:7890 -e all_proxy=http://<proxy-host>:7890 -v "C:\docker\github-rss-aggregator:/app" -w /app docker.1panel.live/library/python:3.11 bash -c "apt-get update && apt-get install -y git curl && rm -rf /tmp/repo && git clone https://github.com/NOwin111/GitHub-RSS-Aggregator.git /tmp/repo && cp -r /tmp/repo/* /app/ && pip install flask feedparser requests && python github_rss_aggregator.py"' }
    @{ Name="qdtoday";               Port="8923";  Desc="QD今日签到";               Image="docker.1panel.live/qdtoday/qd";                               Cmd='wslc run -d -p 8923:80 -v "C:\docker\qdtoday\config:/usr/src/app/config" --name qdtoday docker.1panel.live/qdtoday/qd' }
    @{ Name="quark-auto-save";       Port="5005";  Desc="夸克自动转存";             Image="registry.cn-shenzhen.aliyuncs.com/cp0204/quark-auto-save:latest"; Cmd='wslc run -d -p 5005:5005 -e WEBUI_USERNAME=admin -e WEBUI_PASSWORD=<your-password> -v "C:\docker\quark-auto-save\config:/app/config" -v "C:\docker\quark-auto-save\media:/media" --name quark-auto-save registry.cn-shenzhen.aliyuncs.com/cp0204/quark-auto-save:latest' }
    @{ Name="rabbitpro";             Port="5701";  Desc="RabbitPro";                Image="docker.1panel.live/ht944/rabbitpro:latest";                   Cmd='wslc run --name rabbitpro -p 5701:1234 -d -v "C:\docker\rabbit\data:/Rabbit/data" -it docker.1panel.live/ht944/rabbitpro:latest' }
    @{ Name="peerbanhelper";         Port="9898";  Desc="PeerBanHelper 封禁";       Image="registry.cn-hangzhou.aliyuncs.com/ghostchu/peerbanhelper";   Cmd='wslc run -d --name peerbanhelper -p 9898:9898 -v "C:\docker\peerbanhelper:/app/data/" registry.cn-hangzhou.aliyuncs.com/ghostchu/peerbanhelper' }
    @{ Name="postgresql_mp";         Port="5433";  Desc="PostgreSQL (MoviePilot)";  Image="docker.1panel.live/library/postgres";                         Cmd='wslc run -d --name postgresql_mp -p 5433:5432 -e POSTGRES_DB=moviepilot -e POSTGRES_USER=moviepilot -e POSTGRES_PASSWORD="<your-password>" -v "C:\docker\postgresql_mp:/var/lib/postgresql" docker.1panel.live/library/postgres' }
    @{ Name="redis_mp";              Port="6379";  Desc="Redis (MoviePilot)";       Image="docker.1panel.live/library/redis";                            Cmd='wslc run --name redis_mp -p 6379:6379 -v "C:\docker\redis\data:/data" -d docker.1panel.live/library/redis redis-server --save 600 1 --requirepass "<your-password>"' }
    @{ Name="smartdns";              Port="host";  Desc="SmartDNS";                 Image="docker.1panel.live/pymumu/smartdns:latest";                   Cmd='wslc run -d --name smartdns --network host -p 9053:9053/udp -p 6080:6080 -v "C:\docker\smartdns\data\etc\smartdns:/etc/smartdns" -v "C:\docker\smartdns\data\var\lib\smartdns:/var/lib/smartdns" -v "C:\docker\smartdns\data\var\log\smartdns:/var/log/smartdns" docker.1panel.live/pymumu/smartdns:latest' }
    @{ Name="pt-accelerator";        Port="host";  Desc="PT加速器";                 Image="docker.1panel.live/eternalcurse/pt-accelerator:latest";      Cmd='wslc run -d --name pt-accelerator --network host -v "C:\Windows\System32\drivers\etc\hosts:/etc/hosts" -v "C:\docker\PT-Accelerator\config:/app/config" -v "C:\docker\PT-Accelerator\logs:/app/logs" -e TZ=Asia/Shanghai docker.1panel.live/eternalcurse/pt-accelerator:latest' }
    @{ Name="seedcross";             Port="host";  Desc="SeedCross";                Image="docker.1panel.live/ccf2012/seedcross:latest";                 Cmd='wslc run -d --name seedcross --network host -v "C:\docker\seedcross\db:/code/seedcross\db" -p 8019:8019 docker.1panel.live/ccf2012/seedcross:latest' }
    @{ Name="seedhound";             Port="--";     Desc="ReseedHound 自动补种";     Image="ghcr.io/schalkiii/reseedhound:latest";                       Cmd='wslc run -d --name seedhound -e SEEDHOUND_MODE=schedule -v "C:\docker\seedhound:/app" ghcr.io/schalkiii/reseedhound:latest' }
    @{ Name="mtranserver";           Port="8989";  Desc="MT翻译服务"; Image="docker.1panel.live/xxnuo/mtranserver:latest";                Cmd='wslc run -d --name mtranserver -p 8989:8989 -v "C:\docker\mtranserver\models:/app/models" -v "C:\docker\mtranserver\config.ini:/app/config.ini" docker.1panel.live/xxnuo/mtranserver:latest' }
    @{ Name="pansou-app";            Port="8111";  Desc="Pansou 网盘搜索";          Image="ghcr.nju.edu.cn/fish2018/pansou-web:latest";                 Cmd='wslc run -d --name pansou-app -p 8111:80 -e DOMAIN=localhost -e PANSOU_PORT=8888 -e PANSOU_HOST=127.0.0.1 -e SOCKS5_PROXY=socks5://<proxy-host>:7890 -e HTTP_PROXY=http://<proxy-host>:7890 -e HTTPS_PROXY=https://<proxy-host>:7890 -v "pansou-data:/app/data" -v "pansou-logs:/app/logs" ghcr.nju.edu.cn/fish2018/pansou-web:latest' }
    @{ Name="Reseed-Puppy-Dev";      Port="8091";  Desc="Reseed Puppy Dev";         Image="docker.1panel.live/szzhoubanxian/reseed-puppy:dev";           Cmd='wslc run -d --name Reseed-Puppy-Dev -v "C:\docker\reseed-puppy-dev\database:/reseed-puppy/database" -v "C:\CommonTools\qBittorrent_4.6.7_portable\Profile\qBittorrent\data\BT_backup:/reseed-puppy/public/qb" -p 8091:1997 docker.1panel.live/szzhoubanxian/reseed-puppy:dev' }
    @{ Name="reseed-puppy";          Port="8081";  Desc="Reseed Puppy PHP";         Image="docker.1panel.live/szzhoubanxian/reseed-puppy:latest";        Cmd='wslc run -d --name reseed-puppy -v "C:\docker\reseed-puppy-php\database:/reseed-puppy-php/database" -v "C:\CommonTools\qBittorrent_4.6.7_portable\Profile\qBittorrent\data\BT_backup:/reseed-puppy-php/public/torrents" -p 8081:1919 docker.1panel.live/szzhoubanxian/reseed-puppy:latest' }
    @{ Name="pt-invite-watcher";     Port="8003";  Desc="PT邀请监控";               Image="docker.1panel.live/helloworldz1024/pt-invite-watcher:latest"; Cmd='wslc run -d --name pt-invite-watcher -p 8003:8080 -v "C:\docker\pt_invite_watcher\data:/data" -e PTIW_DB_PATH="/data/ptiw.db" docker.1panel.live/helloworldz1024/pt-invite-watcher:latest' }
)

# ============================================================
# 辅助函数
# ============================================================

function Stop-AndRemove {
    param([string]$Name)
    Write-Host "  停止并移除旧容器 $Name ..." -ForegroundColor DarkGray
    wslc stop $Name 2>$null
    wslc remove $Name 2>$null
}

function Invoke-WslcCmd {
    param([string]$CmdText)
    Invoke-Expression $CmdText
}

# ============================================================
# 镜像拉取 — 简化版：直接拉取加速镜像名
# ============================================================

function Pull-Image {
    param([string]$Image)

    # 本地已存在则跳过
    $localCheck = wslc image list 2>$null
    foreach ($line in $localCheck) {
        $repo = ($line -split '\s+')[0]
        if ($repo -eq $Image) {
            Write-Host "  镜像已存在: $Image" -ForegroundColor DarkGray
            return $true
        }
    }

    # 多源降级拉取
    Write-Host "  Pull: $Image" -ForegroundColor Yellow
    if (Try-Pull $Image) { return $true }

    # 失败: 尝试替换前缀重试
    $fallbackList = Get-FallbackImages $Image
    if ($fallbackList.Count -gt 0) {
        Write-Host "  直拉失败，尝试 $($fallbackList.Count) 个替代源..." -ForegroundColor DarkYellow
        foreach ($fb in $fallbackList) {
            Write-Host "    Try: $fb" -ForegroundColor Yellow
            if (Try-Pull $fb) { return $true }
        }
    }
    Write-Host "  所有源均失败: $Image" -ForegroundColor Red
    return $false
}

function Try-Pull {
    param([string]$Img)
    $proc = Start-Process -FilePath 'wslc' -ArgumentList 'pull', $Img -NoNewWindow -Wait -PassThru
    return ($proc.ExitCode -eq 0)
}

function Get-FallbackImages {
    param([string]$Image)
    $result = @()
    $dhMirrors = @(
        'docker.1ms.run',
        'docker.m.daocloud.io',
        'docker.xuanyuan.me',
        'dockerhub.icu',
        'hub.rat.dev',
        'docker.registry.cyou',
        'docker-cf.13231238.xyz'
    )
    $ghcrMirrors = @(
        'ghcr.nju.edu.cn'
    )

    # Docker Hub 镜像: 替换前缀
    foreach ($m in $dhMirrors) {
        if ($Image -match '^docker\.1panel\.live/') {
            $rest = $Image -replace '^docker\.1panel\.live/', ''
            if ($m -ne 'docker.1panel.live') {
                $result += "$m/$rest"
            }
        }
    }

    # ghcr.io: 尝试 nju 代理和直连
    if ($Image -match '^ghcr\.nju\.edu\.cn/') {
        $rest = $Image -replace '^ghcr\.nju\.edu\.cn/', ''
        $result += "ghcr.io/$rest"
    } elseif ($Image -match '^ghcr\.io/') {
        $rest = $Image -replace '^ghcr\.io/', ''
        foreach ($m in $ghcrMirrors) {
            $result += "$m/$rest"
        }
    }

    return $result
}

# ============================================================
# 菜单函数
# ============================================================
function Show-StartMenu {
    param(
        [array]$Items,
        [string]$Title
    )

    $pageSize = 30
    $page = 0
    $selected = [System.Collections.Generic.List[int]]::new()

    do {
        Clear-Host
        Write-Host "=============================================" -ForegroundColor Cyan
        Write-Host "  $Title" -ForegroundColor Cyan
        Write-Host "=============================================" -ForegroundColor Cyan

        $runningList = wslc list 2>$null

        $start = $page * $pageSize
        $end = [Math]::Min($start + $pageSize, $Items.Count)

        for ($i = $start; $i -lt $end; $i++) {
            $num = $i + 1
            $item = $Items[$i]
            $isSelected = $selected.Contains($i)
            $isRunning = ($runningList | Where-Object { ($_ -split "\s+")[1] -eq $item.Name })

            $marker = if ($isSelected) { ">>" } else { "  " }
            $status = if ($isRunning) { "[ON] " } else { "[OFF]" }
            $selColor = if ($isSelected) { "Yellow" } else { "White" }
            $statusColor = if ($isRunning) { "Green" } else { "DarkGray" }

            $label = "{0} {1,3}. {2,-26}" -f $marker, $num, $item.Name
            Write-Host $label -ForegroundColor $selColor -NoNewline
            Write-Host " $status" -ForegroundColor $statusColor -NoNewline
            Write-Host " $($item.Desc)" -ForegroundColor $selColor
        }

        Write-Host ""
        Write-Host "---------------------------------------------" -ForegroundColor DarkGray
        $totalPages = [Math]::Ceiling($Items.Count / $pageSize)
        Write-Host "页 $($page+1)/$totalPages  |  已选: $($selected.Count) 个" -NoNewline

        if ($selected.Count -gt 0) {
            Write-Host "  |  选中: " -NoNewline
            $selNames = $selected | ForEach-Object { $Items[$_].Name }
            Write-Host ($selNames -join ", ") -ForegroundColor Yellow
        } else {
            Write-Host ""
        }

        Write-Host "---------------------------------------------" -ForegroundColor DarkGray
        Write-Host "[1-$($Items.Count)]选择/取消  [a]全选  [n]全不选  [Enter]确认执行"
        Write-Host "[<]上页  [>]下页  [q]退出"
        $currentMirror = $Script:DockerHubMirrors[$Script:MirrorIndex]
        Write-Host "Docker Hub: $($currentMirror.Prefix) ($($currentMirror.Desc))  |  拉取镜像: $(if($Script:Pull){'是'}else{'否'})"
        Write-Host "---------------------------------------------" -ForegroundColor DarkGray

        $key = Read-Host "请输入"

        switch -Regex ($key) {
            '^\d+$' {
                $idx = [int]$key - 1
                if ($idx -ge 0 -and $idx -lt $Items.Count) {
                    if ($selected.Contains($idx)) { $selected.Remove($idx) | Out-Null }
                    else { $selected.Add($idx) | Out-Null }
                }
            }
            '^[aA]$' {
                $selected.Clear()
                for ($i = 0; $i -lt $Items.Count; $i++) { $selected.Add($i) | Out-Null }
            }
            '^[nN]$' { $selected.Clear() }
            '^<$' { if ($page -gt 0) { $page-- } }
            '^>$' { if (($page+1) * $pageSize -lt $Items.Count) { $page++ } }
            '^[qQ]$' { return }
            '^$' {
                if ($selected.Count -gt 0) {
                    Clear-Host
                    Write-Host "=============================================" -ForegroundColor Cyan
                    Write-Host "  正在处理 $($selected.Count) 个容器..." -ForegroundColor Cyan
                    Write-Host "=============================================`n" -ForegroundColor Cyan

                    $okCount = 0
                    $failCount = 0
                    foreach ($idx in ($selected | Sort-Object)) {
                        $item = $Items[$idx]
                        Write-Host "[$($idx+1)] $($item.Name) - $($item.Desc)" -ForegroundColor Cyan

                        Stop-AndRemove $item.Name

                        if ($Script:Pull) {
                            $pullOk = Pull-Image $item.Image
                            if (-not $pullOk) {
                                Write-Host "  镜像拉取失败，跳过启动`n" -ForegroundColor Red
                                $failCount++
                                continue
                            }
                        }

                        try {
                            Invoke-WslcCmd $item.Cmd
                            Write-Host "  OK`n" -ForegroundColor Green
                            $okCount++
                        } catch {
                            Write-Host "  FAIL: $_`n" -ForegroundColor Red
                            $failCount++
                        }
                    }

                    Write-Host "=============================================" -ForegroundColor $(if ($failCount -eq 0) { 'Green' } else { 'Yellow' })
                    Write-Host "  完成！成功: $okCount  失败: $failCount" -ForegroundColor $(if ($failCount -eq 0) { 'Green' } else { 'Yellow' })
                    Write-Host "=============================================" -ForegroundColor $(if ($failCount -eq 0) { 'Green' } else { 'Yellow' })
                    Write-Host "`n按 Enter 返回..." -ForegroundColor DarkGray
                    Read-Host
                    $selected.Clear()
                }
            }
        }
    } while ($true)
}

function Show-StopMenu {
    $runningList = wslc list 2>$null
    $runningNames = $runningList | ForEach-Object { ($_ -split "\s+")[1] } | Where-Object { $_ }
    $stopItems = $Containers | Where-Object { $runningNames -contains $_.Name }

    if ($stopItems.Count -eq 0) {
        Write-Host "`n  没有正在运行的容器" -ForegroundColor Yellow
        Start-Sleep -Seconds 2
        return
    }

    $selected = [System.Collections.Generic.List[int]]::new()
    $page = 0
    $pageSize = 30

    do {
        Clear-Host
        Write-Host "=============================================" -ForegroundColor Cyan
        Write-Host "  选择要停止的容器 (共 $($stopItems.Count) 个运行中)" -ForegroundColor Cyan
        Write-Host "=============================================" -ForegroundColor Cyan

        $start = $page * $pageSize
        $end = [Math]::Min($start + $pageSize, $stopItems.Count)

        for ($i = $start; $i -lt $end; $i++) {
            $num = $i + 1
            $item = $stopItems[$i]
            $isSelected = $selected.Contains($i)
            $marker = if ($isSelected) { ">>" } else { "  " }
            $selColor = if ($isSelected) { "Yellow" } else { "Green" }
            $line = "{0} {1,3}. {2,-26} {3}" -f $marker, $num, $item.Name, $item.Desc
            Write-Host $line -ForegroundColor $selColor
        }

        Write-Host ""
        Write-Host "[1-$($stopItems.Count)]选择  [a]全选  [n]全不选  [Enter]确认停止  [q]返回" -ForegroundColor DarkGray
        $key = Read-Host "请输入"

        switch -Regex ($key) {
            '^\d+$' {
                $idx = [int]$key - 1
                if ($idx -ge 0 -and $idx -lt $stopItems.Count) {
                    if ($selected.Contains($idx)) { $selected.Remove($idx) | Out-Null }
                    else { $selected.Add($idx) | Out-Null }
                }
            }
            '^[aA]$' { $selected.Clear(); for ($i=0; $i -lt $stopItems.Count; $i++) { $selected.Add($i) | Out-Null } }
            '^[nN]$' { $selected.Clear() }
            '^[qQ]$' { return }
            '^$' {
                if ($selected.Count -gt 0) {
                    Clear-Host
                    foreach ($idx in ($selected | Sort-Object)) {
                        $item = $stopItems[$idx]
                        Write-Host "停止 $($item.Name) ..." -NoNewline -ForegroundColor Yellow
                        wslc stop $item.Name 2>$null
                        wslc remove $item.Name 2>$null
                        Write-Host " OK" -ForegroundColor Green
                    }
                    Write-Host "`n完成！按 Enter 返回..." -ForegroundColor Green
                    Read-Host
                    return
                }
            }
        }
    } while ($true)
}

# ============================================================
# 镜像源切换 (简化版：仅切换 Docker Hub 加速前缀)
# ============================================================
function Show-MirrorMenu {
    do {
        Clear-Host
        Write-Host "=============================================" -ForegroundColor Cyan
        Write-Host "  Docker Hub 加速镜像源" -ForegroundColor Cyan
        Write-Host "=============================================" -ForegroundColor Cyan
        Write-Host ""
        Write-Host "  当前源: $($Script:DockerHubMirrors[$Script:MirrorIndex].Prefix) ($($Script:DockerHubMirrors[$Script:MirrorIndex].Desc))" -ForegroundColor Green
        Write-Host ""
        Write-Host "  注意: 容器 Cmd 中的镜像已预设加速前缀，此处仅控制未来手动 pull 行为" -ForegroundColor DarkGray
        Write-Host ""
        Write-Host "  选择 Docker Hub 加速源:" -ForegroundColor White
        for ($i = 0; $i -lt $Script:DockerHubMirrors.Count; $i++) {
            $m = $Script:DockerHubMirrors[$i]
            $active = if ($Script:MirrorIndex -eq $i) { " ← 当前" } else { "" }
            $color = if ($Script:MirrorIndex -eq $i) { 'Yellow' } else { 'White' }
            Write-Host "  $($i+1). $($m.Prefix.PadRight(28)) $($m.Desc)$active" -ForegroundColor $color
        }
        Write-Host ""
        Write-Host "  q. 返回" -ForegroundColor White
        Write-Host ""

        $choice = Read-Host "选择"

        switch ($choice) {
            'q' { return }
            default {
                $idx = [int]$choice - 1
                if ($idx -ge 0 -and $idx -lt $Script:DockerHubMirrors.Count) {
                    $Script:MirrorIndex = $idx
                    $m = $Script:DockerHubMirrors[$idx]
                    Write-Host "  已切换为: $($m.Prefix) ($($m.Desc))" -ForegroundColor Green
                    Start-Sleep -Seconds 1
                    return
                }
            }
        }
    } while ($true)
}

# ============================================================
# 容器日志查看
# ============================================================
function Show-LogMenu {
    $page = 0
    $pageSize = 30

    do {
        Clear-Host
        Write-Host "=============================================" -ForegroundColor Cyan
        Write-Host "  查看容器日志 (共 $($Containers.Count) 个容器)" -ForegroundColor Cyan
        Write-Host "=============================================" -ForegroundColor Cyan

        $runningList = wslc list 2>$null

        $start = $page * $pageSize
        $end = [Math]::Min($start + $pageSize, $Containers.Count)

        for ($i = $start; $i -lt $end; $i++) {
            $num = $i + 1
            $item = $Containers[$i]
            $isRunning = ($runningList | Where-Object { ($_ -split "\s+")[1] -eq $item.Name })
            $status = if ($isRunning) { "[ON] " } else { "[OFF]" }
            $statusColor = if ($isRunning) { "Green" } else { "DarkGray" }

            $label = "  {0,3}. {1,-26}" -f $num, $item.Name
            Write-Host $label -ForegroundColor White -NoNewline
            Write-Host " $status" -ForegroundColor $statusColor -NoNewline
            Write-Host " $($item.Desc)" -ForegroundColor White
        }

        Write-Host ""
        Write-Host "---------------------------------------------" -ForegroundColor DarkGray
        $totalPages = [Math]::Ceiling($Containers.Count / $pageSize)
        Write-Host "页 $($page+1)/$totalPages" -ForegroundColor DarkGray
        Write-Host "[1-$($Containers.Count)]查看日志  [<]上页  [>]下页  [q]返回" -ForegroundColor DarkGray
        $key = Read-Host "请输入容器序号"

        switch -Regex ($key) {
            '^\d+$' {
                $idx = [int]$key - 1
                if ($idx -ge 0 -and $idx -lt $Containers.Count) {
                    Show-ContainerLog -Name $Containers[$idx].Name -Desc $Containers[$idx].Desc
                }
            }
            '^<$' { if ($page -gt 0) { $page-- } }
            '^>$' { if (($page+1) * $pageSize -lt $Containers.Count) { $page++ } }
            '^[qQ]$' { return }
        }
    } while ($true)
}

function Show-ContainerLog {
    param([string]$Name, [string]$Desc)

    do {
        Clear-Host
        Write-Host "=============================================" -ForegroundColor Cyan
        Write-Host "  日志: $Name - $Desc" -ForegroundColor Cyan
        Write-Host "=============================================" -ForegroundColor Cyan
        Write-Host "  1. 查看最近 200 行 (带时间戳)" -ForegroundColor White
        Write-Host "  2. 查看最近 N 行 (自定义)" -ForegroundColor White
        Write-Host "  3. 实时跟随日志 (-f, 按 Ctrl+C 停止)" -ForegroundColor White
        Write-Host "  4. 查看全部日志" -ForegroundColor White
        Write-Host "  q. 返回" -ForegroundColor White
        Write-Host ""
        $choice = Read-Host "请选择"

        switch ($choice) {
            '1' {
                Write-Host "`n--- 最近 200 行 ---`n" -ForegroundColor DarkGray
                wslc logs -t -n 200 $Name
                Write-Host "`n按 Enter 返回..." -ForegroundColor DarkGray
                Read-Host
            }
            '2' {
                $lines = Read-Host "显示行数"
                if ($lines -match '^\d+$') {
                    Write-Host "`n--- 最近 $lines 行 ---`n" -ForegroundColor DarkGray
                    wslc logs -t -n $lines $Name
                    Write-Host "`n按 Enter 返回..." -ForegroundColor DarkGray
                    Read-Host
                }
            }
            '3' {
                Write-Host "`n--- 实时跟随 (Ctrl+C 停止) ---`n" -ForegroundColor DarkGray
                try { wslc logs -f -t -n 50 $Name } catch {}
                Write-Host "`n已停止跟随，按 Enter 返回..." -ForegroundColor DarkGray
                Read-Host
            }
            '4' {
                Write-Host "`n--- 全部日志 ---`n" -ForegroundColor DarkGray
                wslc logs -t $Name
                Write-Host "`n按 Enter 返回..." -ForegroundColor DarkGray
                Read-Host
            }
            'q' { return }
            'Q' { return }
        }
    } while ($true)
}

# ============================================================
# 主菜单
# ============================================================
function Show-MainMenu {
    do {
        Clear-Host
        Write-Host "=============================================" -ForegroundColor Cyan
        Write-Host "  WSLC 容器管理工具  (共 $($Containers.Count) 个容器)" -ForegroundColor Cyan
        Write-Host "=============================================" -ForegroundColor Cyan
        Write-Host ""
        Write-Host "  1. 启动容器（选择并启动）" -ForegroundColor White
        Write-Host "  2. 停止容器（选择并停止）" -ForegroundColor White
        Write-Host "  3. 查看运行状态" -ForegroundColor White
        Write-Host "  4. 拉取镜像（选择并拉取+启动）" -ForegroundColor White
        $currentMirror = $Script:DockerHubMirrors[$Script:MirrorIndex]
        Write-Host "  5. 镜像加速源 (当前: $($currentMirror.Prefix))" -ForegroundColor White
        Write-Host "  6. 切换是否拉取镜像 (当前: $(if($Script:Pull){'是'}else{'否'}))" -ForegroundColor White
        Write-Host "  7. 查看容器日志" -ForegroundColor White
        Write-Host "  0. 退出" -ForegroundColor White
        Write-Host ""

        $runningList = wslc list 2>$null
        $runningCount = ($runningList | Where-Object { $_ -match '^\S' -and $_ -notmatch '容器 ID' -and $_ -ne '' }).Count
        Write-Host "  当前运行中: $runningCount / $($Containers.Count) 个容器" -ForegroundColor Green
        Write-Host ""

        $choice = Read-Host "请选择"

        switch ($choice) {
            '1' { Show-StartMenu -Items $Containers -Title "选择要启动的容器" }
            '2' { Show-StopMenu }
            '3' {
                Clear-Host
                Write-Host "=============================================" -ForegroundColor Cyan
                Write-Host "  容器运行状态" -ForegroundColor Cyan
                Write-Host "=============================================`n" -ForegroundColor Cyan
                wslc list 2>$null
                Write-Host "`n---------------------------------------------" -ForegroundColor DarkGray
                Write-Host "镜像列表:" -ForegroundColor Cyan
                wslc image list 2>$null
                Write-Host "`n按 Enter 返回..." -ForegroundColor DarkGray
                Read-Host
            }
            '4' {
                $Script:Pull = $true
                Show-StartMenu -Items $Containers -Title "选择要拉取镜像的容器 (拉取后自动启动)"
            }
            '5' { Show-MirrorMenu }
            '6' {
                $Script:Pull = -not $Script:Pull
                Write-Host "  拉取镜像: $(if($Script:Pull){'开启'}else{'关闭'})" -ForegroundColor Green
                Start-Sleep -Seconds 1
            }
            '7' { Show-LogMenu }
            '0' { return }
        }
    } while ($true)
}

# 启动主菜单
Show-MainMenu
