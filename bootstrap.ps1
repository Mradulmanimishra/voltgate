# VoltGate — Windows Bootstrap and Onboarding Script

Clear-Host
Write-Host "=========================================================" -ForegroundColor Cyan
Write-Host "      ⚡ VoltGate — Intelligent Anthropic LLM Router     " -ForegroundColor Cyan
Write-Host "=========================================================" -ForegroundColor Cyan
Write-Host "This script will help you configure and run VoltGate in one click."
Write-Host ""

# 1. Check prerequisites
if (-not (Get-Command cargo -ErrorAction SilentlyContinue) -and -not (Get-Command docker -ErrorAction SilentlyContinue)) {
    Write-Host "⚠️ Warning: Neither Rust (cargo) nor Docker was found in your PATH." -ForegroundColor Yellow
    Write-Host "Please ensure you have at least one of them installed to run the project."
    Write-Host ""
}

# 2. Check and configure environment variables (.env)
$EnvFile = Join-Path $PWD ".env"
$ExampleFile = Join-Path $PWD ".env.example"

if (-not (Test-Path $EnvFile)) {
    if (Test-Path $ExampleFile) {
        Write-Host "Creating new .env file from .env.example..." -ForegroundColor Green
        Copy-Item $ExampleFile $EnvFile
    } else {
        Write-Host "Creating new blank .env file..." -ForegroundColor Green
        New-Item -Path $EnvFile -ItemType File | Out-Null
    }
}

# Load current key
$EnvContent = Get-Content $EnvFile
$ApiKeyLine = $EnvContent | Where-Object { $_ -like "ANTHROPIC_API_KEY=*" }
$CurrentKey = ""
if ($ApiKeyLine -and $ApiKeyLine -match "ANTHROPIC_API_KEY=(.+)") {
    $CurrentKey = $Matches[1].Trim()
}

if ($CurrentKey -eq "" -or $CurrentKey -like "your_anthropic_api_key*") {
    Write-Host "Please configure your credentials." -ForegroundColor Yellow
    $InputKey = Read-Host "Enter your ANTHROPIC_API_KEY"
    if ($InputKey) {
        $EnvContent = $EnvContent | Where-Object { $_ -notlike "ANTHROPIC_API_KEY=*" }
        $EnvContent += "ANTHROPIC_API_KEY=$InputKey"
        $EnvContent | Set-Content $EnvFile
        Write-Host "API Key successfully saved to .env file." -ForegroundColor Green
    } else {
        Write-Host "⚠️ No key entered. Using existing placeholder key." -ForegroundColor Red
    }
} else {
    Write-Host "✓ ANTHROPIC_API_KEY is already configured in .env." -ForegroundColor Green
}
Write-Host ""

# 3. Choose run mode
Write-Host "Select how you would like to run VoltGate:" -ForegroundColor Cyan
Write-Host "  [1] Local Cargo Run (Recommended for development)"
Write-Host "  [2] Docker Compose (Recommended for isolated environments)"
Write-Host "  [3] Exit"
$Choice = Read-Host "Enter choice [1-3]"

switch ($Choice) {
    "1" {
        Write-Host "Compiling and running VoltGate locally..." -ForegroundColor Green
        $env:RUSTFLAGS = "-L $PWD\gcc_compat"
        
        # Start dashboard in the background once server launches
        Start-ThreadJob {
            Start-Sleep -Seconds 4
            Start-Process "http://localhost:3001/dashboard"
        } | Out-Null

        cargo run
    }
    "2" {
        Write-Host "Starting VoltGate with Docker Compose..." -ForegroundColor Green
        
        # Start dashboard in the background once server launches
        Start-ThreadJob {
            Start-Sleep -Seconds 6
            Start-Process "http://localhost:3001/dashboard"
        } | Out-Null

        docker-compose up --build
    }
    "3" {
        Write-Host "Exiting setup." -ForegroundColor Yellow
    }
    default {
        Write-Host "Invalid option. Exiting setup." -ForegroundColor Red
    }
}
