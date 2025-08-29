// Mock API service for development
export interface ApiService {
  getBacktests(): Promise<any[]>;
  getRunningBacktests(): Promise<any[]>;
  getDashboardMetrics(): Promise<any>;
  createBacktest(config: any): Promise<any>;
}

// Simulated API responses for demo purposes
export const mockApiService: ApiService = {
  async getBacktests() {
    // Simulate network delay
    await new Promise(resolve => setTimeout(resolve, 500));
    
    return [
      {
        id: '1',
        strategy_name: 'Mean Reversion Pro',
        symbol: 'SPY',
        timeframe: '1d',
        start_date: '2024-01-01',
        end_date: '2024-08-28',
        initial_capital: 100000,
        final_capital: 132500,
        total_return_percent: 32.5,
        annualized_return_percent: 40.6,
        sharpe_ratio: 2.1,
        sortino_ratio: 2.8,
        calmar_ratio: 1.9,
        max_drawdown_percent: -5.8,
        max_drawdown_duration_days: 23,
        total_trades: 187,
        winning_trades: 123,
        losing_trades: 64,
        win_rate: 0.658,
        profit_factor: 2.15,
        avg_trade_return: 0.0052,
        best_trade: 3200,
        worst_trade: -980,
        volatility: 0.16,
        value_at_risk_95: -2100,
        expected_shortfall_95: -2800,
        beta: 0.78,
        total_orders: 374,
        filled_orders: 370,
        cancelled_orders: 4,
        avg_slippage: 0.0008,
        total_commission_paid: 935,
        created_at: '2024-08-28T10:30:00Z',
        backtest_duration_seconds: 42.3,
      },
      {
        id: '2',
        strategy_name: 'Momentum Breakout',
        symbol: 'QQQ',
        timeframe: '1h',
        start_date: '2024-06-01',
        end_date: '2024-08-28',
        initial_capital: 100000,
        final_capital: 118750,
        total_return_percent: 18.75,
        annualized_return_percent: 75.0,
        sharpe_ratio: 1.6,
        sortino_ratio: 2.2,
        calmar_ratio: 1.3,
        max_drawdown_percent: -12.3,
        max_drawdown_duration_days: 18,
        total_trades: 89,
        winning_trades: 52,
        losing_trades: 37,
        win_rate: 0.584,
        profit_factor: 1.68,
        avg_trade_return: 0.0078,
        best_trade: 4500,
        worst_trade: -2100,
        volatility: 0.24,
        value_at_risk_95: -3400,
        expected_shortfall_95: -4200,
        beta: 1.15,
        total_orders: 178,
        filled_orders: 175,
        cancelled_orders: 3,
        avg_slippage: 0.0012,
        total_commission_paid: 445,
        created_at: '2024-08-28T09:15:00Z',
        backtest_duration_seconds: 78.9,
      },
      {
        id: '3',
        strategy_name: 'RSI Divergence',
        symbol: 'AAPL',
        timeframe: '4h',
        start_date: '2024-03-01',
        end_date: '2024-08-28',
        initial_capital: 100000,
        final_capital: 108900,
        total_return_percent: 8.9,
        annualized_return_percent: 18.5,
        sharpe_ratio: 1.25,
        sortino_ratio: 1.65,
        calmar_ratio: 0.95,
        max_drawdown_percent: -15.2,
        max_drawdown_duration_days: 67,
        total_trades: 134,
        winning_trades: 78,
        losing_trades: 56,
        win_rate: 0.582,
        profit_factor: 1.42,
        avg_trade_return: 0.0034,
        best_trade: 2800,
        worst_trade: -3200,
        volatility: 0.21,
        value_at_risk_95: -2900,
        expected_shortfall_95: -3600,
        beta: 0.92,
        total_orders: 268,
        filled_orders: 264,
        cancelled_orders: 4,
        avg_slippage: 0.0015,
        total_commission_paid: 670,
        created_at: '2024-08-28T08:45:00Z',
        backtest_duration_seconds: 156.7,
      }
    ];
  },

  async getRunningBacktests() {
    await new Promise(resolve => setTimeout(resolve, 300));
    
    return [
      {
        status: 'running',
        progress: 78,
        message: 'Processing MACD Golden Cross strategy on TSLA...',
        start_time: '2024-08-28T11:00:00Z',
        estimated_completion: '2024-08-28T11:12:00Z',
      },
      {
        status: 'running',
        progress: 34,
        message: 'Backtesting Bollinger Bands strategy on NVDA...',
        start_time: '2024-08-28T11:05:00Z',
        estimated_completion: '2024-08-28T11:25:00Z',
      }
    ];
  },

  async getDashboardMetrics() {
    await new Promise(resolve => setTimeout(resolve, 200));
    
    return {
      totalBacktests: 1847,
      avgReturn: 22.3,
      avgSharpe: 1.68,
      bestStrategy: 'Mean Reversion Pro',
      totalStrategies: 31,
      avgExecutionTime: 52.4,
    };
  },

  async createBacktest(config: any) {
    await new Promise(resolve => setTimeout(resolve, 1000));
    
    return {
      id: `bt_${Date.now()}`,
      status: 'queued',
      message: 'Backtest queued for execution',
      config,
    };
  }
};

// Export default service
export const apiService = mockApiService;
