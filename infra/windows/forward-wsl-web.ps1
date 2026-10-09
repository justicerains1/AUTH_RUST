[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Distribution,
    [Parameter(Mandatory = $true)]
    [string]$ListenAddress,
    [int[]]$Ports = @(5173),
    [string[]]$AllowedRemoteAddress = @('Any'),
    [switch]$Plan,
    [switch]$Remove
)

$ErrorActionPreference = 'Stop'
$parsed = $null
if (-not [System.Net.IPAddress]::TryParse($ListenAddress, [ref]$parsed) -or
    $parsed.AddressFamily -ne [System.Net.Sockets.AddressFamily]::InterNetwork -or
    $ListenAddress -in @('0.0.0.0', '127.0.0.1')) {
    throw 'Use the actual Windows IPv4 address, not a wildcard or loopback address.'
}
$permitted = @(5173, 5174, 5175, 8025)
if ($Ports.Count -eq 0 -or ($Ports | Select-Object -Unique).Count -ne $Ports.Count -or
    @($Ports | Where-Object { $_ -notin $permitted }).Count -gt 0) {
    throw 'Only web ports 5173, 5174, 5175 and 8025 are supported.'
}
$wslAddress = $null
if (-not $Remove) {
    $route = (& wsl.exe -d $Distribution -u root -- ip -4 route get 1.1.1.1 | Out-String)
    if ($LASTEXITCODE -ne 0 -or $route -notmatch '\bsrc\s+(\d+\.\d+\.\d+\.\d+)') {
        throw 'Unable to determine the selected WSL distribution IPv4 address.'
    }
    $wslAddress = $Matches[1]
}
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $Plan -and -not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this script from an Administrator PowerShell. Use -Plan for read-only preview.'
}
foreach ($port in $Ports) {
    $rule = "AUTH-RUST-WSL-$ListenAddress-$port"
    if ($Plan) {
        if ($Remove) { Write-Output "Remove $ListenAddress`:$port and firewall rule $rule" }
        else { Write-Output "$ListenAddress`:$port -> $wslAddress`:$port; remote: $($AllowedRemoteAddress -join ',')" }
        continue
    }
    if ($Remove) {
        & netsh.exe interface portproxy delete v4tov4 "listenaddress=$ListenAddress" "listenport=$port" | Out-Null
        Get-NetFirewallRule -Name $rule -ErrorAction SilentlyContinue | Remove-NetFirewallRule
        continue
    }
    $target = [Net.Sockets.TcpClient]::new()
    try {
        $connection = $target.ConnectAsync($wslAddress, $port)
        if (-not $connection.Wait(5000) -or -not $target.Connected) {
            throw "WSL target port $port is not reachable; forwarding was not applied."
        }
    } finally { $target.Dispose() }
    Set-Service iphlpsvc -StartupType Automatic
    Start-Service iphlpsvc
    & netsh.exe interface portproxy set v4tov4 "listenaddress=$ListenAddress" "listenport=$port" "connectaddress=$wslAddress" "connectport=$port" | Out-Null
    if ($LASTEXITCODE -ne 0) {
        & netsh.exe interface portproxy add v4tov4 "listenaddress=$ListenAddress" "listenport=$port" "connectaddress=$wslAddress" "connectport=$port" | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "Port proxy configuration failed for $port." }
    }
    Get-NetFirewallRule -Name $rule -ErrorAction SilentlyContinue | Remove-NetFirewallRule
    New-NetFirewallRule -Name $rule -DisplayName $rule -Direction Inbound -Action Allow -Protocol TCP -LocalAddress $ListenAddress -LocalPort $port -RemoteAddress $AllowedRemoteAddress -Profile Any | Out-Null
    $response = Invoke-WebRequest -Uri "http://$ListenAddress`:$port" -UseBasicParsing -TimeoutSec 10
    Write-Output "Forwarded $ListenAddress`:$port -> $wslAddress`:$port; HTTP $([int]$response.StatusCode)"
}
if (-not $Plan) { & netsh.exe interface portproxy show v4tov4 }
