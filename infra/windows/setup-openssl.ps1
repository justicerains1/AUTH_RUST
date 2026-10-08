[CmdletBinding()]
param(
    [string]$ExpectedVersion = '3.6.4',
    [string]$InstallationRoot = (Join-Path $env:ProgramFiles 'OpenSSL')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if (-not $IsWindows -or -not [Environment]::Is64BitProcess) {
    throw 'OpenSSL setup requires a 64-bit Windows PowerShell process.'
}

# The official windows-2025 image installs the full package here, with DLLs in bin.
$IncludeDirectory = Join-Path $InstallationRoot 'include'
$BinaryDirectory = Join-Path $InstallationRoot 'bin'
$OpenSslExecutable = Join-Path $BinaryDirectory 'openssl.exe'
foreach ($RequiredPath in @(
    $OpenSslExecutable,
    (Join-Path $IncludeDirectory 'openssl\ssl.h'),
    (Join-Path $IncludeDirectory 'openssl\opensslv.h')
)) {
    if (-not (Test-Path -LiteralPath $RequiredPath -PathType Leaf)) {
        throw "The runner does not provide the full OpenSSL installation: $RequiredPath"
    }
}

$VersionOutput = & $OpenSslExecutable version
if ($LASTEXITCODE -ne 0 -or $VersionOutput -notmatch ('^OpenSSL ' + [regex]::Escape($ExpectedVersion) + '(?:\s|$)')) {
    throw "Expected OpenSSL $ExpectedVersion in the pinned CI image; update the reviewed version before retrying."
}

$CandidateDirectories = @(
    (Join-Path $InstallationRoot 'lib\VC\x64\MD'),
    (Join-Path $InstallationRoot 'lib\VC\x64'),
    (Join-Path $InstallationRoot 'lib')
)
$LibraryDirectory = $null
foreach ($CandidateDirectory in $CandidateDirectories) {
    if ((Test-Path -LiteralPath (Join-Path $CandidateDirectory 'libssl.lib') -PathType Leaf) -and
        (Test-Path -LiteralPath (Join-Path $CandidateDirectory 'libcrypto.lib') -PathType Leaf)) {
        $LibraryDirectory = $CandidateDirectory
        break
    }
}
if ($null -eq $LibraryDirectory) {
    # Report actual relative library layout so a runner change is reviewable, without guessing success.
    Get-ChildItem -LiteralPath (Join-Path $InstallationRoot 'lib') -Recurse -File -Filter '*.lib' |
        ForEach-Object { Write-Output $_.FullName }
    throw 'MSVC x64 OpenSSL import libraries libssl.lib and libcrypto.lib were not found.'
}

foreach ($DllPattern in @('libssl-3*.dll', 'libcrypto-3*.dll')) {
    if (@(Get-ChildItem -LiteralPath $BinaryDirectory -File -Filter $DllPattern).Count -eq 0) {
        throw "The matching runtime DLL is missing from OpenSSL bin: $DllPattern"
    }
}

$Values = [ordered]@{
    OPENSSL_DIR = $InstallationRoot
    OPENSSL_INCLUDE_DIR = $IncludeDirectory
    OPENSSL_LIB_DIR = $LibraryDirectory
    OPENSSL_STATIC = '0'
    OPENSSL_LIBS = 'libssl:libcrypto'
}
foreach ($Entry in $Values.GetEnumerator()) {
    Set-Item -Path "Env:$($Entry.Key)" -Value $Entry.Value
    if ($env:GITHUB_ENV) {
        "$($Entry.Key)=$($Entry.Value)" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
    }
}
$env:PATH = "$BinaryDirectory;$env:PATH"
if ($env:GITHUB_PATH) {
    $BinaryDirectory | Out-File -FilePath $env:GITHUB_PATH -Encoding utf8 -Append
}

Write-Output "Verified OpenSSL $ExpectedVersion headers, MSVC import libraries and runtime DLLs."
Write-Output "OpenSSL include directory: $IncludeDirectory"
Write-Output "OpenSSL library directory: $LibraryDirectory"
Write-Output 'Rust openssl-sys will link dynamically; subsequent steps receive only public installation paths.'
