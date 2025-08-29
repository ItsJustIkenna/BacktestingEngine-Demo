// Types for BacktestingEngine data structures
export interface BacktestResult {
  id: string;
  strategy_name: string;
  symbol: string;
  timeframe: string;
  start_date: string;
  end_date: string;
  initial_capital: number;
  final_capital: number;
  
  // Performance metrics
  total_return_percent: number;
  annualized_return_percent: number;
  sharpe_ratio: number;
  sortino_ratio: number;
  calmar_ratio: number;
  max_drawdown_percent: number;
  max_drawdown_duration_days: number;
  
  // Trading metrics
  total_trades: number;
  winning_trades: number;
  losing_trades: number;
  win_rate: number;
  profit_factor: number;
  avg_trade_return: number;
  best_trade: number;
  worst_trade: number;
  
  // Risk metrics
  volatility: number;
  value_at_risk_95: number;
  expected_shortfall_95: number;
  beta: number;
  
  // Execution statistics
  total_orders: number;
  filled_orders: number;
  cancelled_orders: number;
  avg_slippage: number;
  total_commission_paid: number;
  
  // Time series data
  equity_curve: EquityPoint[];
  trades: TradeRecord[];
  drawdown_curve: DrawdownPoint[];
  
  // Additional metadata
  created_at: string;
  backtest_duration_seconds: number;
}

export interface EquityPoint {
  timestamp: string;
  portfolio_value: number;
  cash: number;
  positions_value: number;
  total_pnl: number;
}

export interface TradeRecord {
  id: string;
  timestamp: string;
  symbol: string;
  side: 'BUY' | 'SELL';
  quantity: number;
  price: number;
  commission: number;
  slippage: number;
  pnl: number;
  cumulative_pnl: number;
}

export interface DrawdownPoint {
  timestamp: string;
  drawdown_percent: number;
  underwater_days: number;
}

export interface StrategyMetrics {
  strategy_name: string;
  total_signals: number;
  signals_taken: number;
  signal_accuracy: number;
  avg_position_size: number;
  max_position_size: number;
  avg_holding_period_days: number;
}

export interface RiskMetrics {
  var_95_1d: number;
  var_99_1d: number;
  expected_shortfall_95: number;
  max_leverage: number;
  correlation_spy: number;
  beta_spy: number;
}

export interface BacktestConfig {
  strategy_name: string;
  symbol: string;
  start_date: string;
  end_date: string;
  initial_capital: number;
  commission_rate: number;
  slippage_rate: number;
  benchmark: string;
  risk_free_rate: number;
}

export interface BacktestStatus {
  status: 'idle' | 'running' | 'completed' | 'error';
  progress: number;
  message: string;
  start_time?: string;
  estimated_completion?: string;
}

export interface MarketDataPoint {
  timestamp: string;
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
}

// API Response types
export interface BacktestListResponse {
  backtests: BacktestResult[];
  total: number;
  page: number;
  per_page: number;
}

export interface BacktestDetailResponse {
  backtest: BacktestResult;
  market_data: MarketDataPoint[];
}

// Chart data types for visualization
export interface ChartData {
  labels: string[];
  datasets: Dataset[];
}

export interface Dataset {
  label: string;
  data: number[];
  borderColor: string;
  backgroundColor: string;
  fill?: boolean;
}

// Performance comparison types
export interface PerformanceComparison {
  strategies: string[];
  metrics: {
    [strategyName: string]: {
      total_return: number;
      sharpe_ratio: number;
      max_drawdown: number;
      win_rate: number;
    };
  };
}

// Filter and sort options
export interface BacktestFilters {
  strategy_names?: string[];
  symbols?: string[];
  date_range?: {
    start: string;
    end: string;
  };
  performance_range?: {
    min_return?: number;
    max_drawdown?: number;
    min_sharpe?: number;
  };
}

export interface SortOption {
  field: keyof BacktestResult;
  direction: 'asc' | 'desc';
}

// Real-time monitoring types (for live backtesting)
export interface LiveBacktestMetrics {
  current_portfolio_value: number;
  current_drawdown: number;
  trades_today: number;
  current_positions: Position[];
  recent_signals: Signal[];
}

export interface Position {
  symbol: string;
  quantity: number;
  avg_price: number;
  current_price: number;
  unrealized_pnl: number;
  market_value: number;
}

export interface Signal {
  timestamp: string;
  symbol: string;
  signal_type: 'BUY' | 'SELL' | 'HOLD';
  confidence: number;
  price: number;
  executed: boolean;
}
