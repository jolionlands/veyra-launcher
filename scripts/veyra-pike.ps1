[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$PromptFile,
    [string]$PikeCommand = 'pike',
    [string]$Provider = '',
    [string]$Model = '',
    [string]$Tools = 'read,grep,find',
    [int]$MaxTurns = 40,
    [switch]$NoProjectContext,
    [switch]$AllowMcp
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $PromptFile)) {
    throw "Missing Pike prompt file: $PromptFile"
}

$prompt = Get-Content -LiteralPath $PromptFile -Raw
$pikeArgs = @('--print', '--no-anim')

if ($Provider.Trim()) {
    $pikeArgs += @('--provider', $Provider.Trim())
}
if ($Model.Trim()) {
    $pikeArgs += @('--model', $Model.Trim())
}
if ($NoProjectContext) {
    $pikeArgs += '--no-project-context'
}
if (-not $AllowMcp) {
    $pikeArgs += '--no-mcp'
}
if ($Tools.Trim()) {
    $pikeArgs += @('--tools', $Tools.Trim())
}
if ($MaxTurns -gt 0) {
    $pikeArgs += @('--max-turns', $MaxTurns.ToString())
}

$pikeArgs += $prompt
& $PikeCommand @pikeArgs
$exitCode = $LASTEXITCODE
if ($null -eq $exitCode) {
    $exitCode = 0
}
exit $exitCode
