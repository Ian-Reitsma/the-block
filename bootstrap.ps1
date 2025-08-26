# bootstrap.ps1 - Fully native Windows bootstrap for 'the-block' dev env
# Run in PowerShell as:  .\bootstrap.ps1

$ErrorActionPreference = "Stop"
$APP_NAME = "the-block"
$REQUIRED_PYTHON = "3.12.3"
$PYTHON_VENV = ".venv"
$CARGO_BIN = "$env:USERPROFILE\.cargo\bin"

Function Write-Color($Color, $Text) {
    Write-Host $Text -ForegroundColor $Color
}

Function Ensure-Choco {
    if (-not (Get-Command choco -ErrorAction SilentlyContinue)) {
        Write-Color Yellow "Chocolatey not found. Installing Chocolatey (admin required)..."
        Set-ExecutionPolicy Bypass -Scope Process -Force
        [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072
        Invoke-Expression ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))
    }
    choco upgrade chocolatey -y
}

Function Ensure-Scoop {
    if (-not (Get-Command scoop -ErrorAction SilentlyContinue)) {
        Write-Color Yellow "Scoop not found. Installing Scoop..."
        Set-ExecutionPolicy RemoteSigned -Scope CurrentUser -Force
        Invoke-Expression (New-Object System.Net.WebClient).DownloadString('https://get.scoop.sh')
    }
    scoop update
}

Function Ensure-Tool($Name, $ChocoPkg, $ScoopPkg) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        if (Get-Command choco -ErrorAction SilentlyContinue) {
            choco install $ChocoPkg -y
        } elseif (Get-Command scoop -ErrorAction SilentlyContinue) {
            scoop install $ScoopPkg
        } else {
            throw "No package manager found for $Name"
        }
    }
}

Function Ensure-Python {
    if (-not (Get-Command python -ErrorAction SilentlyContinue)) {
        Ensure-Tool "python" "python" "python"
    }
    $pyver = python --version 2>&1
    if ($pyver -notlike "*$REQUIRED_PYTHON*") {
        Write-Color Yellow "Python version mismatch. Installing $REQUIRED_PYTHON..."
        choco install python --version=$REQUIRED_PYTHON -y
    }
}

Function Ensure-Pip {
    if (-not (Get-Command pip -ErrorAction SilentlyContinue)) {
        Write-Color Yellow "Pip not found. Installing via get-pip.py..."
        Invoke-WebRequest https://bootstrap.pypa.io/get-pip.py -OutFile get-pip.py
        python get-pip.py
        Remove-Item get-pip.py
    }
}

Function Ensure-Venv {
    if (-not (Test-Path $PYTHON_VENV)) {
        Write-Color Cyan "Creating Python venv: $PYTHON_VENV"
        python -m venv $PYTHON_VENV
    }
    .\$PYTHON_VENV\Scripts\Activate.ps1
}


Function Ensure-Rust {
    if (-not (Test-Path "$CARGO_BIN\rustup.exe")) {
        Write-Color Cyan "Installing Rust toolchain..."
        Invoke-WebRequest https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe -OutFile rustup-init.exe
        Start-Process -Wait -NoNewWindow -FilePath rustup-init.exe -ArgumentList "-y"
        Remove-Item rustup-init.exe
    }
    & "$CARGO_BIN\rustup.exe" update
    $env:PATH += ";$CARGO_BIN"
}

Function Ensure-Maturin {
    $pipPath = ".\$PYTHON_VENV\Scripts\pip.exe"
    if (-not (Test-Path $pipPath)) {
        Write-Color Yellow "pip not found in venv. Installing pip."
        python -m ensurepip
    }
    if (-not (& $pipPath show maturin -q)) {
        Write-Color Cyan "Installing maturin (Rust-Python bridge)..."
        & $pipPath install maturin
    }
}

Function Ensure-Nextest {
    param([string]$Version = "0.9.97-b.2")
    $nextestPath = Join-Path $CARGO_BIN "cargo-nextest.exe"
    if ((Test-Path $nextestPath) -and ((& $nextestPath --version) -like "*$Version*")) {
        Write-Color Green "cargo-nextest $Version already installed"
        return
    }
    Write-Color Cyan "Installing cargo-nextest $Version..."
    $arch = "x86_64-pc-windows-msvc"
    $zip = "cargo-nextest-$Version-$arch.zip"
    $url = "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-$Version/$zip"
    $tmpDir = New-Item -ItemType Directory -Force -Path ([System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), [System.Guid]::NewGuid().ToString()))
    Invoke-WebRequest $url -OutFile (Join-Path $tmpDir $zip)
    Expand-Archive (Join-Path $tmpDir $zip) -DestinationPath $tmpDir -Force
    Move-Item -Force (Join-Path $tmpDir "cargo-nextest.exe") $nextestPath
    Remove-Item -Recurse -Force $tmpDir
    cargo nextest --version | Out-Null
}

Function Run-Maturin-Develop {
    $maturinPath = ".\$PYTHON_VENV\Scripts\maturin.exe"
    if (Test-Path $maturinPath -and (Test-Path "Cargo.toml")) {
        Write-Color Cyan "Running 'maturin develop --release' to build Python native module..."
        & $maturinPath develop --release
    } else {
        Write-Color Yellow "maturin or Cargo.toml missing. Skipping Rust-Python build."
    }
}

# Main steps

Write-Color Cyan "==> [$APP_NAME] Native Windows Bootstrap"

Ensure-Choco
Ensure-Scoop
Ensure-Python
Ensure-Pip
Ensure-Venv

$env:PYO3_PYTHON = (Resolve-Path ".\$PYTHON_VENV\Scripts\python.exe")

Write-Color Cyan "Upgrading pip, setuptools, wheel..."
.\$PYTHON_VENV\Scripts\python.exe -m pip install --upgrade pip setuptools wheel

Ensure-Rust
Ensure-Maturin
Ensure-Nextest
if (Test-Path "Cargo.toml") {
    Write-Color Cyan "Running database migrations..."
    cargo run --quiet --bin db_migrate
    if (Test-Path "db_compact.sh" -and (Get-Command bash -ErrorAction SilentlyContinue)) {
        Write-Color Cyan "Compacting database..."
        bash ./db_compact.sh
    }
}
Run-Maturin-Develop

# Python project deps
if (Test-Path "requirements.txt" -and (Get-Content requirements.txt | Where-Object {$_ -match '\S'})) {
    Write-Color Cyan "Installing Python requirements..."
    .\$PYTHON_VENV\Scripts\pip.exe install -r requirements.txt
}
if (Test-Path "pyproject.toml" -and (Get-Command poetry -ErrorAction SilentlyContinue)) {
    Write-Color Cyan "Installing poetry deps..."
    poetry install
}
  if (Test-Path "package.json" -and (Get-Command npm -ErrorAction SilentlyContinue)) {
      if (Test-Path "package-lock.json") { npm ci }
      else { npm install }
  }

# Optional: Pre-commit hooks (if desired)
if (Test-Path ".pre-commit-config.yaml") {
    Write-Color Cyan "Installing pre-commit..."
    .\$PYTHON_VENV\Scripts\pip.exe install pre-commit
    .\$PYTHON_VENV\Scripts\pre-commit.exe install
}

Write-Color Green "==> [$APP_NAME] bootstrap complete."
Write-Color Cyan "Activate venv:  .\$PYTHON_VENV\Scripts\Activate.ps1"
Write-Color Cyan "Python: $(.\$PYTHON_VENV\Scripts\python.exe --version)"
Write-Color Cyan "Rust: $(rustc --version)"
Write-Color Cyan "Cargo: $(cargo --version)"
Write-Color Cyan "Nextest: $(cargo nextest --version)"

# Show other tools if present
if (Get-Command docker -ErrorAction SilentlyContinue) {
    Write-Color Blue "docker: $(docker --version)"
}

Write-Color Yellow "If anything failed, see errors above or re-run as Administrator for missing tools."
