# Installs Athenaeum for your coding agents on Windows. Nothing needs to be installed first, and no
# administrator prompt appears: everything goes under your user profile.
#
#   irm https://github.com/Hanoryx-North/athenaeum/releases/latest/download/install.ps1 | iex
#
# It puts a private, checksum-verified Node next to Athenaeum (so it never depends on, or disturbs,
# whatever Node you have), then runs install.mjs, which installs the rest and connects Claude Code,
# Codex and Cursor.
#
# Options are environment variables, so the one-liner above keeps working:
#   $env:ATHENAEUM_PREFIX            where to install (default: %LOCALAPPDATA%\Athenaeum)
#   $env:ATHENAEUM_CLI               install this athenaeum CLI archive (path or URL) instead of the latest
#   $env:ATHENAEUM_INTEGRATIONS      install this athenaeum-integrations.mjs (path or URL) instead of the latest
#   $env:ATHENAEUM_INSTALL_ARGS      extra install.mjs options, e.g. '--auto-sync --agents claude'
#   $env:ATHENAEUM_INSTALL_BASE_URL  fetch install.mjs from here

# Run in a child scope: `irm ... | iex` evaluates in the caller's session, and the preference
# variables below must not outlive the install in a person's own PowerShell window.
& {
    $ErrorActionPreference = 'Stop'
    # The progress bar makes Invoke-WebRequest many times slower on Windows PowerShell 5.1.
    $ProgressPreference = 'SilentlyContinue'
    # Older Windows PowerShell defaults to protocols GitHub and nodejs.org no longer accept.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $NodeVersion = 'v22.22.0'
    $Base = if ($env:ATHENAEUM_INSTALL_BASE_URL) { $env:ATHENAEUM_INSTALL_BASE_URL } else { 'https://github.com/Hanoryx-North/athenaeum/releases/latest/download' }
    $Prefix = if ($env:ATHENAEUM_PREFIX) { $env:ATHENAEUM_PREFIX } else { Join-Path $env:LOCALAPPDATA 'Athenaeum' }

    # A 32-bit PowerShell on 64-bit Windows reports x86; the real architecture is in the other variable.
    $Machine = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    switch ($Machine) {
        'AMD64' { $Arch = 'x64';   $Sha = 'c97fa376d2becdc8863fcd3ca2dd9a83a9f3468ee7ccf7a6d076ec66a645c77a' }
        'ARM64' { $Arch = 'arm64'; $Sha = '5b44fd410df7b4cd0a1891a05a7b606f8fb7d8786a94997b996a372e82478d7a' }
        default { throw "Unsupported CPU: $Machine" }
    }

    $Work = Join-Path ([IO.Path]::GetTempPath()) ("athenaeum-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $Work | Out-Null
    try {
        $NodeDir = Join-Path $Prefix 'node'
        $NodeExe = Join-Path $NodeDir 'node.exe'

        if ((Test-Path $NodeExe) -and ((& $NodeExe -v) -eq $NodeVersion)) {
            Write-Host "Node $NodeVersion is already in place."
        } else {
            $Name = "node-$NodeVersion-win-$Arch"
            Write-Host "Fetching Node $NodeVersion (kept private to Athenaeum)..."
            $Zip = Join-Path $Work 'node.zip'
            Invoke-WebRequest -UseBasicParsing -Uri "https://nodejs.org/dist/$NodeVersion/$Name.zip" -OutFile $Zip
            # Pinned here so a tampered download is refused.
            if ((Get-FileHash -Algorithm SHA256 $Zip).Hash.ToLower() -ne $Sha) {
                throw 'Node download failed its checksum; refusing to use it.'
            }
            $Extract = Join-Path $Work 'node-extract'
            Expand-Archive -Path $Zip -DestinationPath $Extract
            New-Item -ItemType Directory -Force -Path $Prefix | Out-Null
            if (Test-Path $NodeDir) { Remove-Item -Recurse -Force $NodeDir }
            Move-Item -Path (Join-Path $Extract $Name) -Destination $NodeDir
        }

        # The Athenaeum app runs this script with its own copy of install.mjs (ATHENAEUM_INSTALL_MJS), so
        # the two cannot drift. Under `irm | iex` the variable is unset and the latest release's copy is fetched.
        if ($env:ATHENAEUM_INSTALL_MJS) {
            if (-not (Test-Path -PathType Leaf $env:ATHENAEUM_INSTALL_MJS)) { throw 'ATHENAEUM_INSTALL_MJS does not name a file.' }
            Copy-Item $env:ATHENAEUM_INSTALL_MJS (Join-Path $Work 'install.mjs')
        } else {
            Write-Host 'Fetching the installer...'
            Invoke-WebRequest -UseBasicParsing -Uri "$Base/install.mjs" -OutFile (Join-Path $Work 'install.mjs')
        }

        $InstallArgs = @((Join-Path $Work 'install.mjs'), '--prefix', $Prefix)
        if ($env:ATHENAEUM_CLI) { $InstallArgs += @('--cli', $env:ATHENAEUM_CLI) }
        if ($env:ATHENAEUM_INTEGRATIONS) { $InstallArgs += @('--integrations', $env:ATHENAEUM_INTEGRATIONS) }
        if ($env:ATHENAEUM_INSTALL_ARGS) { $InstallArgs += ($env:ATHENAEUM_INSTALL_ARGS -split '\s+' | Where-Object { $_ }) }

        & $NodeExe @InstallArgs
        if ($LASTEXITCODE -ne 0) { throw "The installer failed (exit code $LASTEXITCODE)." }
    } finally {
        Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
    }
}
