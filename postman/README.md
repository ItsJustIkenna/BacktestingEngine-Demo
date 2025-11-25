# Postman API Testing Guide

Complete guide for testing the BacktestingEngine API using Postman.

## Quick Start

### 1. Import Collection & Environment

**Option A: Import Files**
1. Open Postman
2. Click **Import** button (top left)
3. Drag and drop these files:
   - `postman/BacktestingEngine-API.postman_collection.json`
   - `postman/Local.postman_environment.json`
   - `postman/Production.postman_environment.json`

**Option B: Import from URL**
If hosted on GitHub, you can import directly:
```
https://raw.githubusercontent.com/YOUR_ORG/BacktestingEngine/main/postman/BacktestingEngine-API.postman_collection.json
```

### 2. Select Environment
- Click the environment dropdown (top right)
- Select **BacktestingEngine - Local** for local testing
- Select **BacktestingEngine - Production** for production API

### 3. Start the API Server
```powershell
cd BacktestingEngine
cargo run --bin program -- serve --bind 127.0.0.1:8080
```

### 4. Run Your First Request
1. Open **Health & Status** → **Health Check**
2. Click **Send**
3. You should see: `{"status": "ok", "message": "BacktestingEngine API is running"}`

## Collection Structure

### 📁 Health & Status
- **Health Check** - Verify API is running
- **Queue Statistics** - Get job queue metrics

### 📁 Metadata
- **Get Available Symbols** - Fetch all trading pairs
- **Get Available Exchanges** - Fetch all exchanges

### 📁 Backtest Jobs
- **Submit Backtest (Genetic)** - Submit with genetic algorithm optimization
- **Submit Backtest (Manual)** - Submit with manual parameters
- **Submit Backtest (Grid Search)** - Submit with grid search optimization
- **Get Job Status** - Check job progress
- **Get Job Details** - View job configuration
- **Get Job Results** - Get performance metrics (completed jobs only)
- **Delete Backtest** - Remove a job

### 📁 Backtest History
- **Get All Backtests** - List all backtests
- **Get Backtests (Paginated)** - With page/page_size
- **Filter by Status** - completed, running, queued, failed
- **Filter by Symbol** - e.g., BTC/USDT
- **Filter by Exchange** - e.g., binance
- **Combined Filters** - Use multiple filters

## Typical Workflow

### 1. Check API Health
```
GET /health
```

### 2. Get Available Symbols and Exchanges
```
GET /api/symbols
GET /api/exchanges
```

### 3. Submit a Backtest
```
POST /api/backtest
Content-Type: application/json

{
  "symbol": "BTC/USDT",
  "exchange": "binance",
  "strategy": "arbitrage",
  "initial_capital": 100000.0,
  "optimization_method": "genetic",
  "population_size": 50,
  "generations": 20
}
```

**Response:**
```json
{
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "queued",
  "message": "Backtest submitted successfully"
}
```

The collection automatically saves `job_id` to environment variables!

### 4. Monitor Job Status
```
GET /api/backtest/{{job_id}}/status
```

**Response:**
```json
{
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "running",
  "progress": 45.5,
  "created_at": "2024-01-01T10:00:00Z",
  "started_at": "2024-01-01T10:01:00Z"
}
```

### 5. Get Results (when completed)
```
GET /api/backtest/{{job_id}}/results
```

**Response:**
```json
{
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "completed",
  "sharpe_ratio": 1.85,
  "total_pnl": 15234.50,
  "max_drawdown": -0.12,
  "win_rate": 0.67,
  "total_trades": 1523,
  "winning_trades": 1020,
  "losing_trades": 503
}
```

## Environment Variables

### Automatic Variables
- `job_id` - Automatically set when submitting a backtest
- `base_url` - API base URL (configured per environment)

### Manual Variables
You can add custom variables:
1. Go to environment settings
2. Add new variable (e.g., `default_capital`, `default_symbol`)
3. Use in requests: `{{default_symbol}}`

## Testing Different Scenarios

### Test Error Handling
**Submit with Missing Fields:**
```json
{
  "symbol": "BTC/USDT"
  // Missing required fields
}
```
Expected: `400 Bad Request`

**Submit with Invalid Capital:**
```json
{
  "initial_capital": -1000
}
```
Expected: `400 Bad Request` or `422 Unprocessable Entity`

### Test Pagination
```
GET /api/backtests?page=1&page_size=10
GET /api/backtests?page=2&page_size=10
```

### Test Filtering
```
GET /api/backtests?status=completed&symbol=BTC/USDT
```

## Using Collection Runner

Run all requests automatically:

1. Click **Collection** → **Run**
2. Select requests to run
3. Set iterations (e.g., 5)
4. Click **Run BacktestingEngine API**

This will execute all requests in sequence and show pass/fail results.

## Pre-request Scripts

The collection includes automatic test scripts that:
- ✅ Save `job_id` from submit responses
- ✅ Validate response status codes
- ✅ Check response structure

## Custom Tests

Add tests to verify responses:

```javascript
// Test status code
pm.test("Status code is 200", function() {
    pm.response.to.have.status(200);
});

// Test response body
pm.test("Response has job_id", function() {
    var jsonData = pm.response.json();
    pm.expect(jsonData.job_id).to.be.a('string');
});

// Test response time
pm.test("Response time < 500ms", function() {
    pm.expect(pm.response.responseTime).to.be.below(500);
});
```

## WebSocket Testing

For real-time progress updates, use a WebSocket client:
```
ws://localhost:8080/api/backtest/{job_id}/progress
```

Postman doesn't natively support WebSocket well, use:
- [Postman WebSocket Request](https://learning.postman.com/docs/sending-requests/websocket/websocket/)
- Or use browser tools like [WebSocket King](https://chrome.google.com/webstore/detail/websocket-king)

## Troubleshooting

### Connection Refused
```
Error: connect ECONNREFUSED 127.0.0.1:8080
```
**Fix:** Ensure API server is running
```powershell
cargo run --bin program -- serve
```

### 404 Not Found
**Fix:** Check endpoint URL and make sure you're using the correct environment

### Job ID Not Found
**Fix:** 
1. Submit a new backtest first
2. Check that `{{job_id}}` variable is set in environment
3. Verify job exists: `GET /api/backtests`

### CORS Issues (Browser)
The API includes CORS middleware, but if testing from browser:
- Use Postman desktop app (not web version)
- Or configure CORS in API if needed

## Advanced Usage

### Bulk Testing
Create a CSV file with test data:
```csv
symbol,exchange,capital
BTC/USDT,binance,100000
ETH/USDT,coinbase,50000
SOL/USDT,binance,25000
```

Use in Collection Runner → **Select File** → Run with data

### Monitor Collection
Set up Postman Monitors to run collection periodically:
1. Collection → **Monitors**
2. Configure schedule (e.g., every hour)
3. Get email alerts on failures

## Example cURL Commands

If you prefer command line:

```bash
# Health check
curl http://localhost:8080/health

# Submit backtest
curl -X POST http://localhost:8080/api/backtest \
  -H "Content-Type: application/json" \
  -d '{
    "symbol": "BTC/USDT",
    "exchange": "binance",
    "strategy": "arbitrage",
    "initial_capital": 100000,
    "optimization_method": "genetic"
  }'

# Get status
curl http://localhost:8080/api/backtest/{job_id}/status
```

## Next Steps

1. ✅ Import collection and environment
2. ✅ Start API server
3. ✅ Run Health Check
4. ✅ Submit a test backtest
5. ✅ Monitor job status
6. ✅ Get results

For issues or questions, check the main [README.md](../README.md) or [TESTING.md](../TESTING.md).
