param(
    [Parameter(Mandatory = $true)]
    [string]$TargetIp,

    [Parameter(Mandatory = $true)]
    [string]$ApiKey,

    [int]$Port = 5000,
    [string]$Requester = "WORKSTATION-HUB",
    [switch]$SkipAi,
    [switch]$SkipTerminal,
    [int]$TimeoutSec = 6
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$baseUrl = "http://$TargetIp`:$Port"
$headers = @{ "X-Grid-Key" = $ApiKey }

$results = New-Object System.Collections.Generic.List[object]

function Add-Result {
    param(
        [string]$Name,
        [bool]$Ok,
        [string]$Detail
    )

    $results.Add([pscustomobject]@{
        check  = $Name
        status = if ($Ok) { "PASS" } else { "FAIL" }
        detail = $Detail
    })
}

function Invoke-Check {
    param(
        [string]$Name,
        [scriptblock]$Action
    )

    try {
        & $Action
    }
    catch {
        Add-Result -Name $Name -Ok $false -Detail $_.Exception.Message
    }
}

Write-Host "Running THE GRID mesh smoke against $baseUrl" -ForegroundColor Cyan

Invoke-Check -Name "ping" -Action {
    $r = Invoke-RestMethod -Method Get -Uri "$baseUrl/ping" -Headers $headers -TimeoutSec $TimeoutSec
    $ok = ($null -ne $r.ok) -and [bool]$r.ok
    $auth = ($null -ne $r.authorized) -and [bool]$r.authorized
    Add-Result -Name "ping" -Ok ($ok -and $auth) -Detail "ok=$ok authorized=$auth host=$($r.hostname) version=$($r.version)"
}

Invoke-Check -Name "capabilities" -Action {
    try {
        $r = Invoke-RestMethod -Method Get -Uri "$baseUrl/v1/capabilities" -Headers $headers -TimeoutSec $TimeoutSec
        $caps = $r.capabilities
        $detail = "file=$($caps.file_access) term=$($caps.terminal_access) ai=$($caps.ai_access) remote=$($caps.remote_control) rdp=$($caps.rdp_enabled)"
        Add-Result -Name "capabilities" -Ok ([bool]$r.ok) -Detail $detail
    }
    catch {
        # Backward-compatible fallback for older nodes that don't expose /v1/capabilities.
        if ($_.Exception.Message -match "404") {
            $t = Invoke-RestMethod -Method Get -Uri "$baseUrl/telemetry" -Headers $headers -TimeoutSec $TimeoutSec
            $caps = $t.capabilities
            $detail = "legacy_fallback telemetry: file=$($caps.has_file_access) rdp=$($caps.has_rdp) ai_models=$(@($caps.ai_models).Count)"
            Add-Result -Name "capabilities" -Ok $true -Detail $detail
        }
        else {
            throw
        }
    }
}

Invoke-Check -Name "sync" -Action {
    $uri = "$baseUrl/v1/sync?after=0&requester=$([uri]::EscapeDataString($Requester))"
    $r = Invoke-RestMethod -Method Get -Uri $uri -Headers $headers -TimeoutSec ($TimeoutSec + 2)
    $files = if ($null -ne $r.files) { @($r.files).Count } else { 0 }
    $tombs = if ($null -ne $r.tombstones) { @($r.tombstones).Count } else { 0 }
    Add-Result -Name "sync" -Ok $true -Detail "files=$files tombstones=$tombs"
}

if (-not $SkipAi) {
    Invoke-Check -Name "ai_embed" -Action {
        $body = @{ text = "mesh smoke embedding check" } | ConvertTo-Json
        $r = Invoke-RestMethod -Method Post -Uri "$baseUrl/v1/ai/embed" -Headers $headers -Body $body -ContentType "application/json" -TimeoutSec ($TimeoutSec + 4)
        $dims = @($r).Count
        Add-Result -Name "ai_embed" -Ok ($dims -gt 0) -Detail "vector_dims=$dims"
    }
}

if (-not $SkipTerminal) {
    Invoke-Check -Name "terminal_session" -Action {
        $sid = $null
        $terminalError = $null
        for ($i = 0; $i -lt 3; $i++) {
            try {
                $session = Invoke-RestMethod -Method Post -Uri "$baseUrl/v1/terminal/session" -Headers $headers -TimeoutSec $TimeoutSec
                $sid = $session.session_id
                if (-not [string]::IsNullOrWhiteSpace($sid)) {
                    break
                }
            }
            catch {
                $terminalError = $_.Exception.Message
            }
        }

        if ([string]::IsNullOrWhiteSpace($sid)) {
            if ([string]::IsNullOrWhiteSpace($terminalError)) {
                $terminalError = "missing session_id"
            }
            Add-Result -Name "terminal_session" -Ok $false -Detail $terminalError
            return
        }

        $cmd = [System.Text.Encoding]::UTF8.GetBytes("echo GRID_SMOKE`r`n")
        Invoke-WebRequest -UseBasicParsing -Method Post -Uri "$baseUrl/v1/terminal/input?id=$sid" -Headers $headers -Body $cmd -TimeoutSec $TimeoutSec | Out-Null
        $ok = $false
        $text = ""
        for ($i = 0; $i -lt 8; $i++) {
            $out = Invoke-WebRequest -UseBasicParsing -Method Get -Uri "$baseUrl/v1/terminal/output?id=$sid" -Headers $headers -TimeoutSec $TimeoutSec
            $text = [System.Text.Encoding]::UTF8.GetString($out.Content)
            if ($text -match "GRID_SMOKE") {
                $ok = $true
                break
            }
        }
        Add-Result -Name "terminal_session" -Ok $ok -Detail "session_id=$sid echo_detected=$ok"
    }
}

Write-Host ""
$results | Format-Table -AutoSize

$failed = @($results | Where-Object { $_.status -eq "FAIL" }).Count
$passed = @($results | Where-Object { $_.status -eq "PASS" }).Count

Write-Host ""
Write-Host "Summary: PASS=$passed FAIL=$failed" -ForegroundColor $(if ($failed -eq 0) { "Green" } else { "Yellow" })

if ($failed -gt 0) {
    exit 1
}

exit 0
