# Port forward BacktestingEngine UI and API
# Usage: .\scripts\port-forward.ps1

$namespace = "production"

Write-Host "🔌 Starting port forwards for BacktestingEngine..." -ForegroundColor Cyan

# Kill any existing port-forwards
Get-Process -Name "kubectl" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

# Start port forwards in background
$jobs = @()

# UI on port 3000
$jobs += Start-Job -ScriptBlock {
    kubectl port-forward svc/backtesting-engine-ui 3000:80 -n production 2>&1
}
Write-Host "  ✅ UI: http://localhost:3000" -ForegroundColor Green

# API on port 8080
$jobs += Start-Job -ScriptBlock {
    kubectl port-forward svc/backtesting-engine-api 8080:8080 -n production 2>&1
}
Write-Host "  ✅ API: http://localhost:8080" -ForegroundColor Green

Write-Host ""
Write-Host "Port forwards running in background. Press Ctrl+C to stop." -ForegroundColor Yellow
Write-Host "Or run: Get-Job | Stop-Job; Get-Job | Remove-Job" -ForegroundColor Gray

# Keep script running
try {
    while ($true) {
        Start-Sleep -Seconds 60
        # Check if jobs are still running
        $running = $jobs | Get-Job | Where-Object { $_.State -eq "Running" }
        if ($running.Count -eq 0) {
            Write-Host "⚠️ Port forwards stopped. Restarting..." -ForegroundColor Yellow
            # Restart logic could go here
        }
    }
} finally {
    Write-Host "`nStopping port forwards..." -ForegroundColor Cyan
    $jobs | Stop-Job -ErrorAction SilentlyContinue
    $jobs | Remove-Job -ErrorAction SilentlyContinue
}
