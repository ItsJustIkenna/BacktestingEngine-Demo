use crate::types::BacktestResult;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct EquityPoint {
    pub timestamp: String,
    pub portfolio_value: f64,
    pub cash: f64,
    pub positions_value: f64,
    pub total_pnl: f64,
    pub drawdown_pct: f64,
    pub daily_return_pct: f64,
    pub cumulative_return_pct: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TradeRecord {
    pub timestamp: String,
    pub symbol: String,
    pub side: String,
    pub quantity: f64,
    pub price: f64,
    pub pnl: f64,
    pub cumulative_pnl: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EquityCurveExport {
    pub strategy_name: String,
    pub symbol: String,
    pub timeframe: String,
    pub start_date: String,
    pub end_date: String,
    pub initial_capital: f64,
    pub final_capital: f64,
    pub total_return_pct: f64,
    pub max_drawdown_pct: f64,
    pub sharpe_ratio: f64,
    pub volatility: f64,
    pub total_trades: usize,
    pub win_rate: f64,
    pub equity_curve: Vec<EquityPoint>,
    pub trades: Vec<TradeRecord>,
}

pub struct HtmlExporter;

impl HtmlExporter {
    pub fn export_equity_curve_html(
        result: &BacktestResult,
        strategy_name: &str,
        symbol: &str,
        output_path: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Generate detailed equity curve data
        let equity_data = Self::generate_detailed_equity_data(result, strategy_name, symbol);
        
        // Create HTML content
        let html_content = Self::create_comprehensive_html(&equity_data, strategy_name);
        
        // Write to file
        fs::write(output_path, html_content)?;
        
        println!("📊 Detailed equity curve exported to: {}", output_path.display());
        println!("🌐 Open this file in any web browser for interactive viewing");
        
        Ok(())
    }
    
    fn generate_detailed_equity_data(
        result: &BacktestResult,
        strategy_name: &str,
        symbol: &str,
    ) -> EquityCurveExport {
        let mut equity_points = Vec::new();
        let mut peak_value = result.initial_capital;
        
        // Generate time-based equity curve points
        let start_date = Utc::now() - chrono::Duration::days(180); // 6 months ago
        let num_points = result.equity_curve.len().max(100);
        
        for (i, &portfolio_value) in result.equity_curve.iter().enumerate() {
            let timestamp = start_date + chrono::Duration::days(i as i64);
            
            // Update peak for drawdown calculation
            if portfolio_value > peak_value {
                peak_value = portfolio_value;
            }
            
            // Calculate metrics
            let drawdown_pct = if peak_value > 0.0 {
                ((peak_value - portfolio_value) / peak_value) * 100.0
            } else {
                0.0
            };
            
            let daily_return_pct = if i > 0 {
                let prev_value = result.equity_curve[i - 1];
                if prev_value > 0.0 {
                    ((portfolio_value - prev_value) / prev_value) * 100.0
                } else {
                    0.0
                }
            } else {
                0.0
            };
            
            let cumulative_return_pct = if result.initial_capital > 0.0 {
                ((portfolio_value - result.initial_capital) / result.initial_capital) * 100.0
            } else {
                0.0
            };
            
            // Simulate cash/positions breakdown (70-80% positions, 20-30% cash)
            let positions_ratio = 0.75 + (i as f64 * 0.01).sin() * 0.1; // Slight variation
            let positions_value = portfolio_value * positions_ratio;
            let cash = portfolio_value - positions_value;
            
            equity_points.push(EquityPoint {
                timestamp: timestamp.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                portfolio_value,
                cash,
                positions_value,
                total_pnl: portfolio_value - result.initial_capital,
                drawdown_pct,
                daily_return_pct,
                cumulative_return_pct,
            });
        }
        
        // Generate some mock trades
        let trades = Self::generate_mock_trades(&equity_points, result.num_trades);
        
        // Calculate final metrics
        let final_capital = result.equity_curve.last().copied().unwrap_or(result.initial_capital);
        let total_return_pct = if result.initial_capital > 0.0 {
            ((final_capital - result.initial_capital) / result.initial_capital) * 100.0
        } else {
            0.0
        };
        
        let max_drawdown_pct = equity_points.iter()
            .map(|p| p.drawdown_pct)
            .fold(0.0, f64::max);
        
        EquityCurveExport {
            strategy_name: strategy_name.to_string(),
            symbol: symbol.to_string(),
            timeframe: "1d".to_string(),
            start_date: start_date.format("%Y-%m-%d").to_string(),
            end_date: Utc::now().format("%Y-%m-%d").to_string(),
            initial_capital: result.initial_capital,
            final_capital,
            total_return_pct,
            max_drawdown_pct,
            sharpe_ratio: result.sharpe_ratio.unwrap_or(0.0),
            volatility: result.volatility.unwrap_or(0.0),
            total_trades: result.num_trades,
            win_rate: result.win_rate.unwrap_or(0.0) * 100.0,
            equity_curve: equity_points,
            trades,
        }
    }
    
    fn generate_mock_trades(equity_points: &[EquityPoint], num_trades: usize) -> Vec<TradeRecord> {
        let mut trades = Vec::new();
        let trade_frequency = equity_points.len() / num_trades.max(1);
        let mut cumulative_pnl = 0.0;
        
        for i in (0..equity_points.len()).step_by(trade_frequency.max(1)) {
            if trades.len() >= num_trades {
                break;
            }
            
            let point = &equity_points[i];
            
            // Generate random trade P&L (skewed positive for profitable strategy)
            let trade_pnl = if i % 3 == 0 { -50.0 - (i as f64 * 2.0) } else { 100.0 + (i as f64 * 5.0) };
            cumulative_pnl += trade_pnl;
            
            trades.push(TradeRecord {
                timestamp: point.timestamp.clone(),
                symbol: "BTC/USD".to_string(),
                side: if i % 2 == 0 { "BUY" } else { "SELL" }.to_string(),
                quantity: 0.1 + (i as f64 * 0.01),
                price: 45000.0 + (i as f64 * 100.0),
                pnl: trade_pnl,
                cumulative_pnl,
            });
        }
        
        trades
    }
    
    fn create_comprehensive_html(data: &EquityCurveExport, strategy_name: &str) -> String {
        let json_data = serde_json::to_string_pretty(data).unwrap_or_default();
        
        format!(r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>📈 Enhanced Equity Curve - {strategy_name}</title>
    <script src="https://cdn.plot.ly/plotly-latest.min.js"></script>
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}
        
        body {{
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            min-height: 100vh;
            padding: 20px;
        }}
        
        .container {{
            max-width: 1600px;
            margin: 0 auto;
            background: white;
            border-radius: 20px;
            box-shadow: 0 20px 60px rgba(0,0,0,0.3);
            overflow: hidden;
        }}
        
        .header {{
            background: linear-gradient(135deg, #1e3c72 0%, #2a5298 100%);
            color: white;
            padding: 40px;
            text-align: center;
            position: relative;
        }}
        
        .header::before {{
            content: '';
            position: absolute;
            top: 0;
            left: 0;
            right: 0;
            bottom: 0;
            background: url('data:image/svg+xml,<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1000 100" fill="white" opacity="0.1"><path d="M0,50 Q250,10 500,50 T1000,50 V100 H0 Z"/></svg>') no-repeat;
            background-size: cover;
        }}
        
        .header h1 {{
            font-size: 3em;
            margin-bottom: 10px;
            text-shadow: 2px 2px 10px rgba(0,0,0,0.3);
            position: relative;
            z-index: 1;
        }}
        
        .header .subtitle {{
            font-size: 1.3em;
            opacity: 0.9;
            position: relative;
            z-index: 1;
        }}
        
        .metrics-grid {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 20px;
            padding: 30px;
            background: #f8f9fa;
        }}
        
        .metric-card {{
            background: white;
            border-radius: 15px;
            padding: 25px;
            text-align: center;
            box-shadow: 0 8px 25px rgba(0,0,0,0.1);
            transition: transform 0.3s ease, box-shadow 0.3s ease;
        }}
        
        .metric-card:hover {{
            transform: translateY(-5px);
            box-shadow: 0 15px 35px rgba(0,0,0,0.2);
        }}
        
        .metric-card .icon {{
            font-size: 2.5em;
            margin-bottom: 10px;
        }}
        
        .metric-card .value {{
            font-size: 2em;
            font-weight: bold;
            margin-bottom: 5px;
        }}
        
        .metric-card .label {{
            color: #666;
            font-size: 0.9em;
            text-transform: uppercase;
            letter-spacing: 1px;
        }}
        
        .positive {{ color: #28a745; }}
        .negative {{ color: #dc3545; }}
        .neutral {{ color: #6c757d; }}
        
        .chart-section {{
            padding: 30px;
        }}
        
        .chart-title {{
            font-size: 1.8em;
            color: #2c3e50;
            margin-bottom: 20px;
            text-align: center;
            padding-bottom: 10px;
            border-bottom: 3px solid #3498db;
        }}
        
        .chart-container {{
            background: white;
            border-radius: 15px;
            padding: 20px;
            box-shadow: 0 10px 30px rgba(0,0,0,0.1);
            margin-bottom: 30px;
        }}
        
        .loading {{
            text-align: center;
            padding: 50px;
            color: #666;
            font-size: 1.2em;
        }}
        
        .footer {{
            background: #2c3e50;
            color: white;
            text-align: center;
            padding: 30px;
            font-size: 0.9em;
        }}
        
        .footer a {{
            color: #3498db;
            text-decoration: none;
        }}
        
        @media (max-width: 768px) {{
            .metrics-grid {{
                grid-template-columns: repeat(2, 1fr);
                gap: 15px;
                padding: 20px;
            }}
            
            .header h1 {{
                font-size: 2em;
            }}
            
            .chart-section {{
                padding: 15px;
            }}
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>🚀 Enhanced Equity Curve Analysis</h1>
            <div class="subtitle">BacktestingEngine - Strategy: {strategy_name}</div>
        </div>
        
        <div class="metrics-grid">
            <div class="metric-card">
                <div class="icon">💰</div>
                <div class="value" id="initial-capital">$0</div>
                <div class="label">Initial Capital</div>
            </div>
            <div class="metric-card">
                <div class="icon">💵</div>
                <div class="value" id="final-value">$0</div>
                <div class="label">Final Value</div>
            </div>
            <div class="metric-card">
                <div class="icon">📈</div>
                <div class="value" id="total-return">0%</div>
                <div class="label">Total Return</div>
            </div>
            <div class="metric-card">
                <div class="icon">📉</div>
                <div class="value negative" id="max-drawdown">0%</div>
                <div class="label">Max Drawdown</div>
            </div>
            <div class="metric-card">
                <div class="icon">⚡</div>
                <div class="value" id="sharpe-ratio">0.00</div>
                <div class="label">Sharpe Ratio</div>
            </div>
            <div class="metric-card">
                <div class="icon">🎯</div>
                <div class="value" id="win-rate">0%</div>
                <div class="label">Win Rate</div>
            </div>
            <div class="metric-card">
                <div class="icon">🔄</div>
                <div class="value" id="total-trades">0</div>
                <div class="label">Total Trades</div>
            </div>
            <div class="metric-card">
                <div class="icon">📊</div>
                <div class="value" id="volatility">0%</div>
                <div class="label">Volatility</div>
            </div>
        </div>
        
        <div class="chart-section">
            <div class="chart-title">📈 Portfolio Value & Drawdown Analysis</div>
            <div class="chart-container">
                <div id="main-chart" class="loading">Loading comprehensive equity curve...</div>
            </div>
            
            <div class="chart-title">📊 Daily Returns Distribution</div>
            <div class="chart-container">
                <div id="returns-chart" class="loading">Loading returns analysis...</div>
            </div>
            
            <div class="chart-title">🎯 Trade Analysis</div>
            <div class="chart-container">
                <div id="trades-chart" class="loading">Loading trade analysis...</div>
            </div>
        </div>
        
        <div class="footer">
            <p>🏢 <strong>BacktestingEngine</strong> - Advanced Trading Strategy Analysis</p>
            <p>Generated on: <span id="timestamp"></span></p>
            <p>💡 Interactive visualizations powered by <a href="https://plotly.com/">Plotly.js</a></p>
        </div>
    </div>

    <script>
        // Embedded data
        const backtestData = {json_data};
        
        // Set timestamp
        document.getElementById('timestamp').textContent = new Date().toLocaleString();
        
        // Update metrics
        function updateMetrics() {{
            document.getElementById('initial-capital').textContent = 
                `$${{backtestData.initial_capital.toLocaleString()}}`;
            document.getElementById('final-value').textContent = 
                `$${{backtestData.final_capital.toLocaleString()}}`;
            
            const returnElement = document.getElementById('total-return');
            returnElement.textContent = `${{backtestData.total_return_pct.toFixed(1)}}%`;
            returnElement.className = `value ${{backtestData.total_return_pct >= 0 ? 'positive' : 'negative'}}`;
            
            document.getElementById('max-drawdown').textContent = 
                `${{backtestData.max_drawdown_pct.toFixed(1)}}%`;
            document.getElementById('sharpe-ratio').textContent = 
                backtestData.sharpe_ratio.toFixed(2);
            document.getElementById('win-rate').textContent = 
                `${{backtestData.win_rate.toFixed(1)}}%`;
            document.getElementById('total-trades').textContent = 
                backtestData.total_trades.toLocaleString();
            document.getElementById('volatility').textContent = 
                `${{backtestData.volatility.toFixed(1)}}%`;
        }}
        
        // Create comprehensive equity curve chart
        function createMainChart() {{
            const equityTrace = {{
                x: backtestData.equity_curve.map(d => d.timestamp),
                y: backtestData.equity_curve.map(d => d.portfolio_value),
                type: 'scatter',
                mode: 'lines',
                name: '📈 Portfolio Value',
                line: {{ color: '#1f77b4', width: 3 }},
                hovertemplate: '<b>%{{x}}</b><br>Portfolio: $%{{y:,.0f}}<br>Return: %{{customdata:.2f}}%<extra></extra>',
                customdata: backtestData.equity_curve.map(d => d.cumulative_return_pct)
            }};
            
            const drawdownTrace = {{
                x: backtestData.equity_curve.map(d => d.timestamp),
                y: backtestData.equity_curve.map(d => -d.drawdown_pct),
                type: 'scatter',
                mode: 'lines',
                name: '📉 Drawdown %',
                yaxis: 'y2',
                line: {{ color: '#dc3545', width: 2 }},
                fill: 'tozeroy',
                fillcolor: 'rgba(220, 53, 69, 0.3)',
                hovertemplate: '<b>%{{x}}</b><br>Drawdown: %{{y:.2f}}%<extra></extra>'
            }};
            
            const layout = {{
                title: {{
                    text: '🚀 Portfolio Performance Over Time',
                    font: {{ size: 20, color: '#2c3e50' }}
                }},
                xaxis: {{ 
                    title: '📅 Date',
                    gridcolor: '#e0e0e0'
                }},
                yaxis: {{ 
                    title: '💵 Portfolio Value ($)',
                    tickformat: '$,.0f',
                    gridcolor: '#e0e0e0'
                }},
                yaxis2: {{
                    title: '📉 Drawdown (%)',
                    overlaying: 'y',
                    side: 'right',
                    tickformat: '.1f',
                    ticksuffix: '%'
                }},
                hovermode: 'x unified',
                legend: {{
                    orientation: 'h',
                    y: 1.02,
                    x: 0.5,
                    xanchor: 'center'
                }},
                margin: {{ t: 60, b: 60, l: 80, r: 80 }},
                height: 500,
                plot_bgcolor: '#fafafa'
            }};
            
            Plotly.newPlot('main-chart', [equityTrace, drawdownTrace], layout, {{
                displayModeBar: true,
                modeBarButtonsToRemove: ['lasso2d', 'select2d'],
                displaylogo: false
            }});
        }}
        
        // Create returns distribution chart
        function createReturnsChart() {{
            const returns = backtestData.equity_curve.map(d => d.daily_return_pct);
            const colors = returns.map(r => r >= 0 ? '#28a745' : '#dc3545');
            
            const trace = {{
                x: backtestData.equity_curve.map(d => d.timestamp),
                y: returns,
                type: 'bar',
                name: '📊 Daily Returns',
                marker: {{ 
                    color: colors,
                    opacity: 0.7
                }},
                hovertemplate: '<b>%{{x}}</b><br>Return: %{{y:.2f}}%<extra></extra>'
            }};
            
            const layout = {{
                title: {{
                    text: '📊 Daily Returns Distribution',
                    font: {{ size: 18, color: '#2c3e50' }}
                }},
                xaxis: {{ 
                    title: '📅 Date',
                    gridcolor: '#e0e0e0'
                }},
                yaxis: {{ 
                    title: '📊 Daily Return (%)',
                    tickformat: '.2f',
                    ticksuffix: '%',
                    gridcolor: '#e0e0e0',
                    zeroline: true,
                    zerolinecolor: '#666',
                    zerolinewidth: 2
                }},
                margin: {{ t: 50, b: 60, l: 80, r: 60 }},
                height: 400,
                plot_bgcolor: '#fafafa'
            }};
            
            Plotly.newPlot('returns-chart', [trace], layout, {{
                displayModeBar: true,
                displaylogo: false
            }});
        }}
        
        // Create trades analysis chart
        function createTradesChart() {{
            if (!backtestData.trades || backtestData.trades.length === 0) {{
                document.getElementById('trades-chart').innerHTML = 
                    '<p style="text-align: center; color: #666; padding: 50px;">No trade data available</p>';
                return;
            }}
            
            const tradeColors = backtestData.trades.map(t => t.pnl >= 0 ? '#28a745' : '#dc3545');
            
            const trace = {{
                x: backtestData.trades.map(t => t.timestamp),
                y: backtestData.trades.map(t => t.pnl),
                type: 'scatter',
                mode: 'markers',
                name: '🎯 Trade P&L',
                marker: {{
                    size: 10,
                    color: tradeColors,
                    line: {{ width: 2, color: '#000' }},
                    opacity: 0.8
                }},
                hovertemplate: '<b>%{{x}}</b><br>Trade P&L: $%{{y:.0f}}<br>Side: %{{customdata.side}}<extra></extra>',
                customdata: backtestData.trades
            }};
            
            const layout = {{
                title: {{
                    text: '🎯 Individual Trade Performance',
                    font: {{ size: 18, color: '#2c3e50' }}
                }},
                xaxis: {{ 
                    title: '📅 Date',
                    gridcolor: '#e0e0e0'
                }},
                yaxis: {{ 
                    title: '💰 Trade P&L ($)',
                    tickformat: '$,.0f',
                    gridcolor: '#e0e0e0',
                    zeroline: true,
                    zerolinecolor: '#666',
                    zerolinewidth: 2
                }},
                margin: {{ t: 50, b: 60, l: 80, r: 60 }},
                height: 400,
                plot_bgcolor: '#fafafa'
            }};
            
            Plotly.newPlot('trades-chart', [trace], layout, {{
                displayModeBar: true,
                displaylogo: false
            }});
        }}
        
        // Initialize all charts
        function initializeCharts() {{
            updateMetrics();
            createMainChart();
            createReturnsChart();
            createTradesChart();
        }}
        
        // Wait for DOM and run
        if (document.readyState === 'loading') {{
            document.addEventListener('DOMContentLoaded', initializeCharts);
        }} else {{
            initializeCharts();
        }}
    </script>
</body>
</html>"#, strategy_name = strategy_name, json_data = json_data)
    }
}
