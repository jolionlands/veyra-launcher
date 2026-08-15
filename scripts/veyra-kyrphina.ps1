[CmdletBinding()]
param(
    [ValidateSet('Chat', 'Settings', 'Doctor', 'Install', 'StartLlama', 'StartGenie')]
    [string]$Mode = 'Chat',
    [string]$Query = '',
    [string]$Profile = 'ai',
    [string]$RepoRoot = (Join-Path $env:USERPROFILE 'Development\kyrphina'),
    [string]$ProviderLabel = '',
    [string]$BaseUrl = '',
    [string]$Model = '',
    [string]$FallbackBaseUrl = '',
    [string]$FallbackModel = '',
    [string]$ApiKeyEnv = '',
    [int]$TimeoutMs = 60000
)

$ErrorActionPreference = 'Stop'

function Resolve-KyrphinaPath {
    param([Parameter(Mandatory = $true)][string]$Path)

    $resolved = Resolve-Path -LiteralPath $Path -ErrorAction Stop
    return $resolved.ProviderPath
}

function Start-Panel {
    param(
        [Parameter(Mandatory = $true)][string]$ScriptPath,
        [Parameter(Mandatory = $true)][string]$SessionPath
    )

    Start-Process -FilePath 'powershell.exe' -WindowStyle Hidden -ArgumentList @(
        '-NoProfile',
        '-NonInteractive',
        '-ExecutionPolicy', 'Bypass',
        '-STA',
        '-WindowStyle', 'Hidden',
        '-File', $ScriptPath,
        '-Path', $SessionPath
    ) | Out-Null
}

function Get-PanelDir {
    $base = if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { $env:TEMP }
    $panelDir = Join-Path $base 'kyrphina\panel'
    New-Item -ItemType Directory -Force -Path $panelDir | Out-Null
    return $panelDir
}

function Test-PanelAlive {
    param([Parameter(Mandatory = $true)][string]$PanelDir)

    $pidPath = Join-Path $PanelDir 'panel.pid'
    if (-not (Test-Path -LiteralPath $pidPath)) {
        return $false
    }

    try {
        $panelPid = [int]((Get-Content -LiteralPath $pidPath -Raw).Trim())
        if ($panelPid -le 0) {
            return $false
        }
        $process = Get-Process -Id $panelPid -ErrorAction SilentlyContinue
        return [bool]$process
    } catch {
        return $false
    }
}

function Convert-ToChatCompletionsUrl {
    param([Parameter(Mandatory = $true)][string]$Url)

    $value = $Url.Trim()
    if (-not $value) {
        return ''
    }
    if ($value -notmatch '^[a-zA-Z][a-zA-Z0-9+.-]*://') {
        $value = "http://$value"
    }
    $value = $value.TrimEnd('/')
    if ($value -notmatch '/v1/chat/completions$') {
        if ($value -match '/v1$') {
            $value = "$value/chat/completions"
        } else {
            $value = "$value/v1/chat/completions"
        }
    }
    return $value
}

function New-Session {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$ProfileName,
        [string]$InitialQuery = '',
        [string]$ProviderLabel = '',
        [string]$BaseUrl = '',
        [string]$Model = '',
        [string]$FallbackBaseUrl = '',
        [string]$FallbackModel = ''
    )

    $profileName = $ProfileName.Trim().ToLowerInvariant()
    if (-not $profileName) {
        $profileName = 'ai'
    }

    $isLlama = $profileName -eq 'llama'
    $skillsRoot = Join-Path $Root 'skills'
    $iniPath = Join-Path $Root 'plugin\askai.ini'

    $query = $InitialQuery.Trim()
    $query = $query -replace '(?i)^(kyrphina|veyra|ai|ask|chat|llama)\s+', ''

    if ($BaseUrl -and $Model) {
        $label = if ($ProviderLabel) { $ProviderLabel } else { $profileName }
        $model = $Model
        $endpoint = Convert-ToChatCompletionsUrl -Url $BaseUrl
        $fallbackModel = $FallbackModel
        $fallbackEndpoint = if ($FallbackBaseUrl) { Convert-ToChatCompletionsUrl -Url $FallbackBaseUrl } else { '' }
        $prompt = 'You are a helpful assistant. Be direct and concise. Use tools when they would answer better; otherwise respond directly.'
    } elseif ($isLlama) {
        $label = 'llama'
        $model = 'llama:LFM2.5-1.2B-Instruct-Q4_K_M'
        $endpoint = 'http://127.0.0.1:8080/v1/chat/completions'
        $fallbackModel = ''
        $fallbackEndpoint = ''
        $prompt = 'You are a helpful local assistant. Use tools when they would answer better; otherwise respond directly.'
    } else {
        $label = 'ai'
        $model = 'genie:MiniCPM5-1B'
        $endpoint = 'http://127.0.0.1:8910/v1/chat/completions'
        $fallbackModel = 'llama:LFM2.5-1.2B-Instruct-Q4_K_M'
        $fallbackEndpoint = 'http://127.0.0.1:8080/v1/chat/completions'
        $prompt = 'You are Kyrphina, a fast local assistant running on-device. Be direct and concise. Use exactly one tool when it clearly helps.'
    }

    return [ordered]@{
        profile = $profileName
        label = $label
        model = $model
        fallback_model = $fallbackModel
        endpoint = $endpoint
        fallback_endpoint = $fallbackEndpoint
        initial_query = $query
        system_prompt = $prompt
        ini_path = $iniPath
        tools_enabled = $true
        tools_list = @()
        runners_dir = (Join-Path $skillsRoot '_runners')
        manifests_dir = (Join-Path $skillsRoot '_manifests')
        max_tool_calls = 3
        timeout_ms = $TimeoutMs
        api_key_env = $ApiKeyEnv
        session_name = ''
        session_path = ''
        initial_messages = @()
        mailbox_ts = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    }
}

function Write-JsonFile {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)]$Value
    )

    $json = $Value | ConvertTo-Json -Depth 20 -Compress
    [IO.File]::WriteAllText($Path, $json, [Text.Encoding]::UTF8)
}

function Show-TextPanel {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$Title,
        [Parameter(Mandatory = $true)][string]$Body
    )

    $showScript = Join-Path $Root 'panel\show_answer.ps1'
    if (-not (Test-Path -LiteralPath $showScript)) {
        return
    }

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("veyra_kyrphina_{0}.txt" -f ([Guid]::NewGuid().ToString('N')))
    [IO.File]::WriteAllText($tmp, "$Title`n$Body", [Text.Encoding]::UTF8)
    Start-Panel -ScriptPath $showScript -SessionPath $tmp
}

function Test-TcpPort {
    param(
        [string]$HostName = '127.0.0.1',
        [Parameter(Mandatory = $true)][int]$Port,
        [int]$TimeoutMs = 750
    )

    try {
        $tcp = [Net.Sockets.TcpClient]::new()
        $async = $tcp.BeginConnect($HostName, $Port, $null, $null)
        $ok = $async.AsyncWaitHandle.WaitOne($TimeoutMs)
        $tcp.Close()
        return [bool]$ok
    } catch {
        return $false
    }
}

function Start-KyrphinaBackend {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$ProfileName
    )

    $profileName = $ProfileName.Trim().ToLowerInvariant()
    if ($profileName -eq 'llama') {
        if (Test-TcpPort -Port 8080) {
            return 'llama'
        }

        $llamaScript = Join-Path $Root 'scripts\start-llama-server.ps1'
        $logDir = Join-Path $Root 'zig-core\test\results'
        & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $llamaScript -Background -LogDir $logDir | Out-Null
        return 'llama'
    }

    if (Test-TcpPort -Port 8910) {
        return 'ai'
    }

    $genieError = $null
    try {
        $genieScript = Join-Path $Root 'scripts\start-genie-server.ps1'
        & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $genieScript -Background | Out-Null
        if (Test-TcpPort -Port 8910 -TimeoutMs 1500) {
            return 'ai'
        }
    } catch {
        $genieError = $_.Exception.Message
    }

    try {
        if (-not (Test-TcpPort -Port 8080)) {
            $llamaScript = Join-Path $Root 'scripts\start-llama-server.ps1'
            $logDir = Join-Path $Root 'zig-core\test\results'
            & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $llamaScript -Background -LogDir $logDir | Out-Null
        }
        return 'llama'
    } catch {
        $body = "Genie failed: $genieError`nllama.cpp failed: $($_.Exception.Message)"
        Show-TextPanel -Root $Root -Title 'Kyrphina backend failed' -Body $body
        return ''
    }
}

function Start-ProviderBackend {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$ProviderBaseUrl
    )

    if (-not $ProviderBaseUrl.Trim()) {
        return
    }

    try {
        $url = Convert-ToChatCompletionsUrl -Url $ProviderBaseUrl
        $uri = [Uri]$url
    } catch {
        return
    }

    $isLocal = $uri.Host -in @('127.0.0.1', 'localhost', '::1')
    if (-not $isLocal) {
        return
    }

    if ($uri.Port -eq 8910) {
        if (-not (Test-TcpPort -Port 8910)) {
            $genieScript = Join-Path $Root 'scripts\start-genie-server.ps1'
            & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $genieScript -Background | Out-Null
        }
        return
    }

    if ($uri.Port -eq 8080) {
        if (-not (Test-TcpPort -Port 8080)) {
            $llamaScript = Join-Path $Root 'scripts\start-llama-server.ps1'
            $logDir = Join-Path $Root 'zig-core\test\results'
            & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $llamaScript -Background -LogDir $logDir | Out-Null
        }
    }
}

function Invoke-CapturedScript {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$ScriptPath,
        [string[]]$ExtraArgs = @(),
        [Parameter(Mandatory = $true)][string]$Title
    )

    if (-not (Test-Path -LiteralPath $ScriptPath)) {
        Show-TextPanel -Root $Root -Title $Title -Body "Missing script: $ScriptPath"
        return
    }

    $output = & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $ScriptPath @ExtraArgs 2>&1 | Out-String
    Show-TextPanel -Root $Root -Title $Title -Body $output.Trim()
}

$root = Resolve-KyrphinaPath -Path $RepoRoot

switch ($Mode) {
    'Chat' {
        $chatScript = Join-Path $root 'panel\chat_panel.ps1'
        $activeProfile = $Profile
        if ($BaseUrl -and $Model) {
            $primaryError = $null
            try {
                Start-ProviderBackend -Root $root -ProviderBaseUrl $BaseUrl
            } catch {
                $primaryError = $_.Exception.Message
            }
            if ($FallbackBaseUrl) {
                try {
                    Start-ProviderBackend -Root $root -ProviderBaseUrl $FallbackBaseUrl
                } catch {
                    if ($primaryError) {
                        Show-TextPanel -Root $root -Title 'Kyrphina backend failed' -Body "Primary failed: $primaryError`nFallback failed: $($_.Exception.Message)"
                        return
                    }
                }
            } elseif ($primaryError) {
                Show-TextPanel -Root $root -Title 'Kyrphina backend failed' -Body $primaryError
                return
            }
        } else {
            $activeProfile = Start-KyrphinaBackend -Root $root -ProfileName $Profile
            if (-not $activeProfile) {
                return
            }
        }
        $panelDir = Get-PanelDir
        $mailbox = Join-Path $panelDir 'mailbox.json'
        $session = New-Session -Root $root -ProfileName $activeProfile -InitialQuery $Query -ProviderLabel $ProviderLabel -BaseUrl $BaseUrl -Model $Model -FallbackBaseUrl $FallbackBaseUrl -FallbackModel $FallbackModel
        Write-JsonFile -Path $mailbox -Value $session
        if (-not (Test-PanelAlive -PanelDir $panelDir)) {
            Start-Panel -ScriptPath $chatScript -SessionPath $mailbox
        }
    }
    'Settings' {
        $settingsScript = Join-Path $root 'panel\settings_panel.ps1'
        $tmp = Join-Path ([IO.Path]::GetTempPath()) ("veyra_kyrphina_settings_{0}.json" -f ([Guid]::NewGuid().ToString('N')))
        Write-JsonFile -Path $tmp -Value @{ ini_path = (Join-Path $root 'plugin\askai.ini') }
        Start-Panel -ScriptPath $settingsScript -SessionPath $tmp
    }
    'Doctor' {
        Invoke-CapturedScript -Root $root -ScriptPath (Join-Path $root 'scripts\doctor.ps1') -Title 'Kyrphina Doctor'
    }
    'Install' {
        Invoke-CapturedScript -Root $root -ScriptPath (Join-Path $root 'scripts\install.ps1') -ExtraArgs @('-Restart') -Title 'Kyrphina Install'
    }
    'StartLlama' {
        $script = Join-Path $root 'scripts\start-llama-server.ps1'
        $logDir = Join-Path $root 'zig-core\test\results'
        Start-Process -FilePath 'powershell.exe' -WindowStyle Hidden -ArgumentList @(
            '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
            '-WindowStyle', 'Hidden', '-File', $script, '-Background', '-LogDir', $logDir
        ) | Out-Null
    }
    'StartGenie' {
        $script = Join-Path $root 'scripts\start-genie-server.ps1'
        Start-Process -FilePath 'powershell.exe' -WindowStyle Hidden -ArgumentList @(
            '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
            '-WindowStyle', 'Hidden', '-File', $script
        ) | Out-Null
    }
}
