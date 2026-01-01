import React, { useState, useEffect, useCallback } from 'react';

interface MonteCarloSummary {
  num_runs: number;
  mean_sharpe: number;
  stddev_sharpe: number;
  var_95: number;
  cvar_95: number;
  confidence_lower: number;
  confidence_upper: number;
  // Percentile distributions
  p5_total_pnl?: number;
  p50_total_pnl?: number;
  p95_total_pnl?: number;
  p5_max_drawdown?: number;
  p50_max_drawdown?: number;
  p95_max_drawdown?: number;
  p50_sharpe?: number;
  p5_win_rate?: number;
  p50_win_rate?: number;
  p95_win_rate?: number;
}

interface WalkForwardSummary {
  periods_tested: number;
  average_performance: number;
  consistency_score: number;
  out_of_sample_degradation: number;
}

interface InventorySummary {
  total_volume: number;
  avg_inventory_value: number;
  turnover_per_day: number;
  buy_fills: number;
  sell_fills: number;
  buy_volume: number;
  sell_volume: number;
  imbalance_ratio: number;
  // Position tracking fields
  starting_inventory?: number;
  starting_inventory_value?: number;
  starting_mark_price?: number;
  ending_inventory?: number;
  ending_inventory_value?: number;
  ending_mark_price?: number;
  starting_capital?: number;
  ending_capital?: number;
  final_equity?: number;
}

interface ExecutionSummary {
  orders_placed: number;
  orders_filled: number;
  orders_cancelled: number;
  total_fills: number;
  fill_rate: number;
  cancellation_rate: number;
  avg_fills_per_order: number;
  avg_time_to_fill_seconds: number;
  median_time_to_fill_seconds: number;
  bid_orders: number;
  ask_orders: number;
  bid_fills: number;
  ask_fills: number;
  bid_fill_rate: number;
  ask_fill_rate: number;
  avg_quote_spread_bps: number;
  // Market-Making Specific Win Rate Metrics
  round_trip_win_rate?: number;
  total_round_trips?: number;
  profitable_round_trips?: number;
  spread_capture_rate?: number;
  adverse_selection_rate?: number;
  avg_pnl_per_round_trip?: number;
}

// Smart Order Routing Metrics
interface VenueRoutingSummary {
  venue: string;
  orders_routed: number;
  volume_routed: number;
  avg_fill_price: number;
  total_fees: number;
  avg_slippage_bps: number;
  fill_rate: number;
}

interface ArbitrageMetricsSummary {
  opportunities_detected: number;
  opportunities_executed: number;
  opportunities_missed: number;
  total_arb_pnl: number;
  avg_profit_bps: number;
  win_rate: number;
  avg_execution_latency_ms: number;
}

interface RoutingMetrics {
  total_orders_routed: number;
  total_volume_routed: number;
  total_fees_paid: number;
  avg_slippage_bps: number;
  max_slippage_bps: number;
  implementation_shortfall_bps: number;
  multi_venue_orders: number;
  single_venue_orders: number;
  avg_venues_per_order: number;
  quality_excellent_pct: number;
  quality_good_pct: number;
  quality_average_pct: number;
  quality_poor_pct: number;
  venue_breakdown: VenueRoutingSummary[];
  routing_savings: number;
  arbitrage_metrics?: ArbitrageMetricsSummary;
}

// Portfolio-specific interfaces
interface SymbolResult {
  symbol: string;
  exchange?: string;  // Per-symbol exchange (optional, falls back to primary)
  sharpe_ratio: number;
  max_drawdown: number;
  total_pnl: number;
  win_rate: number;
  profit_factor: number;
  total_trades: number;
  allocation_weight: number;
  // Per-symbol position tracking
  starting_capital?: number;
  ending_capital?: number;
  starting_inventory?: number;
  ending_inventory?: number;
  starting_inventory_value?: number;
  ending_inventory_value?: number;
  starting_mark_price?: number;
  ending_mark_price?: number;
}

interface CorrelationPair {
  symbol_a: string;
  symbol_b: string;
  correlation: number;
}

interface PortfolioMetrics {
  total_symbols: number;
  symbol_results: SymbolResult[];
  correlation_matrix: CorrelationPair[];
  portfolio_sharpe: number;
  portfolio_max_drawdown: number;
  diversification_ratio?: number;
}

interface BacktestResult {
  job_id?: string;
  strategy_type?: string;  // "market_making", "mean_reversion", "momentum", "arbitrage"
  symbol?: string;
  exchange?: string;
  sharpe_ratio?: number;
  max_drawdown?: number;
  total_pnl?: number;
  win_rate?: number;
  profit_factor?: number;
  total_trades?: number;
  equity_curve?: Array<number | { timestamp?: string; value?: number }>;
  strategy_metrics?: any;
  monte_carlo?: MonteCarloSummary;
  walk_forward?: WalkForwardSummary;
  inventory_metrics?: InventorySummary;
  execution_metrics?: ExecutionSummary;
  
  // Portfolio mode results
  portfolio_metrics?: PortfolioMetrics;
  is_portfolio?: boolean;
  
  // Smart Order Routing results
  routing_metrics?: RoutingMetrics;
  
  // Trade breakdown
  closed_trades?: number;
  open_positions?: number;
  realized_pnl?: number;
  unrealized_pnl?: number;
  
  // Date range for equity curve x-axis display
  start_date?: string;
  end_date?: string;
  
  // Additional performance metrics
  annualized_return?: number;
  volatility?: number;
  sortino_ratio?: number;
  calmar_ratio?: number;
  avg_trade_return?: number;
  best_trade?: number;
  worst_trade?: number;
  avg_time_in_trade?: number;
  
  // Risk metrics
  value_at_risk_95?: number;
  expected_shortfall_95?: number;
  max_drawdown_duration_days?: number;
  current_drawdown?: number;
  avg_drawdown?: number;
  
  // Execution metrics (legacy)
  total_orders?: number;
  filled_orders?: number;
  cancelled_orders?: number;
  avg_slippage?: number;
  total_commission_paid?: number;
  
  // Strategy parameters
  strategy_params?: any;
  
  // Risk halt reason
  risk_halt_reason?: string;
}

interface JobStatus {
  job_id: string;
  status: string;
  progress: number;
  created_at: string;
  started_at: string | null;
  completed_at: string | null;
  error_message: string | null;
  current_generation: number | null;
  total_generations: number | null;
  current_phase: string;
  phase_details: any;
}

interface FitnessWeights {
  sharpe: number;
  sortino: number;
  profit_factor: number;
  win_rate: number;
  net_profit: number;
  calmar: number;
}

interface JobDetails {
  job_id: string;
  symbol: string;
  exchange: string;
  strategy: string;
  initial_capital: number;
  optimization_method: string;
  population_size?: number;
  generations?: number;
  created_at: string;
  started_at: string | null;
  // Portfolio mode fields
  is_portfolio: boolean;
  portfolio_assets?: Array<{ symbol: string; exchange: string; weight?: number }>;
  // All configuration parameters
  initial_inventory_pct: number;
  initial_30d_volume: number;
  monte_carlo_enabled: boolean;
  monte_carlo_runs: number;
  walk_forward_enabled: boolean;
  train_ratio: number;
  test_ratio: number;
  validate_ratio: number;
  auto_save_profitable: boolean;
  min_sharpe_ratio: number;
  min_profit_factor: number;
  max_drawdown: number;
  min_win_rate: number;
  min_trades: number;
  min_sortino_ratio?: number;
  min_calmar_ratio?: number;
  min_total_pnl?: number;
  fitness_weights?: FitnessWeights;
  start_date?: string;
  end_date?: string;
  commission_rate: number;
  // Manual strategy parameters
  risk_aversion: number;
  inventory_target: number;
  order_size: number;
  order_size_pct: number;
  window_ms: number;
  min_quote_lifetime_ms: number;
  volatility_cap: number;
  time_horizon_minutes: number;
  market_depth_k: number;
  volatility_adjustment_factor: number;
  inventory_penalty_factor: number;
  trend_sensitivity: number;
  flow_sensitivity: number;
  take_profit_pct: number;
  stop_loss_pct: number;
  min_spread_bps: number;
  max_trades_per_minute: number;
}

interface ResultsVisualizationProps {
  jobId: string;
  onBack?: () => void;
}

// Helper function to safely format numbers that might be undefined
const safeToFixed = (value: number | undefined | null, decimals: number = 2): string => {
  if (value === undefined || value === null || isNaN(value)) {
    return 'N/A';
  }
  return value.toFixed(decimals);
};

// Helper function to format large numbers (K, M, B)
const formatLargeNumber = (value: number): string => {
  if (value === undefined || value === null || isNaN(value)) {
    return 'N/A';
  }
  if (Math.abs(value) >= 1_000_000_000) {
    return `${(value / 1_000_000_000).toFixed(2)}B`;
  }
  if (Math.abs(value) >= 1_000_000) {
    return `${(value / 1_000_000).toFixed(2)}M`;
  }
  if (Math.abs(value) >= 1_000) {
    return `${(value / 1_000).toFixed(2)}K`;
  }
  return value.toFixed(2);
};

// Helper function to format currency with full dollar amounts (e.g., $10,000 instead of $10.00K)
const formatCurrency = (value: number | undefined | null, decimals: number = 2): string => {
  if (value === undefined || value === null || isNaN(value)) {
    return 'N/A';
  }
  return value.toLocaleString('en-US', { minimumFractionDigits: decimals, maximumFractionDigits: decimals });
};

export const ResultsVisualization: React.FC<ResultsVisualizationProps> = ({ 
  jobId, 
  onBack 
}) => {
  const [result, setResult] = useState<BacktestResult | null>(null);
  const [jobStatus, setJobStatus] = useState<JobStatus | null>(null);
  const [jobDetails, setJobDetails] = useState<JobDetails | null>(null);
  const [loading, setLoading] = useState(true); // Start with loading true for initial load
  const [error, setError] = useState<string | null>(null);
  const [displayProgress, setDisplayProgress] = useState<number>(0);
  const [copied, setCopied] = useState(false);
  const [actionLoading, setActionLoading] = useState<'cancel' | 'delete' | null>(null);
  const [elapsedTime, setElapsedTime] = useState<string>('');
  const isFirstLoad = React.useRef(true);

  const copyToClipboard = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy:', err);
    }
  };

  const cancelJob = async () => {
    if (!window.confirm('Are you sure you want to cancel this job? This action cannot be undone.')) {
      return;
    }
    setActionLoading('cancel');
    try {
      const response = await fetch(`/api/backtest/${jobId}/cancel`, { method: 'POST' });
      if (response.ok) {
        alert('Job cancelled successfully');
        if (onBack) onBack();
      } else {
        const data = await response.json();
        alert(`Failed to cancel job: ${data.error || 'Unknown error'}`);
      }
    } catch (err) {
      alert(`Failed to cancel job: ${err}`);
    } finally {
      setActionLoading(null);
    }
  };

  const deleteJob = async () => {
    if (!window.confirm('Are you sure you want to delete this job? All results will be permanently removed.')) {
      return;
    }
    setActionLoading('delete');
    try {
      const response = await fetch(`/api/backtest/${jobId}`, { method: 'DELETE' });
      if (response.ok) {
        alert('Job deleted successfully');
        if (onBack) onBack();
      } else {
        const data = await response.json();
        alert(`Failed to delete job: ${data.error || 'Unknown error'}`);
      }
    } catch (err) {
      alert(`Failed to delete job: ${err}`);
    } finally {
      setActionLoading(null);
    }
  };

  // Ref to track mounted state for cleanup
  const isMountedRef = React.useRef(true);
  const pollingTimeoutRef = React.useRef<NodeJS.Timeout | null>(null);
  const elapsedTimerRef = React.useRef<NodeJS.Timeout | null>(null);

  // Format elapsed time helper
  const formatElapsedTime = (startedAt: string): string => {
    const startTime = new Date(startedAt).getTime();
    const elapsed = Date.now() - startTime;
    const seconds = Math.floor(elapsed / 1000);
    const minutes = Math.floor(seconds / 60);
    const hours = Math.floor(minutes / 60);
    if (hours > 0) return `${hours}h ${minutes % 60}m ${seconds % 60}s`;
    if (minutes > 0) return `${minutes}m ${seconds % 60}s`;
    return `${seconds}s`;
  };

  // Update elapsed time every second for running jobs
  React.useEffect(() => {
    if (jobStatus?.status === 'running' && jobStatus?.started_at) {
      // Update immediately
      setElapsedTime(formatElapsedTime(jobStatus.started_at));
      
      // Then update every second
      elapsedTimerRef.current = setInterval(() => {
        if (jobStatus?.started_at) {
          setElapsedTime(formatElapsedTime(jobStatus.started_at));
        }
      }, 1000);
    }
    
    return () => {
      if (elapsedTimerRef.current) {
        clearInterval(elapsedTimerRef.current);
      }
    };
  }, [jobStatus?.status, jobStatus?.started_at]);

  // Cleanup on unmount
  React.useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
      if (pollingTimeoutRef.current) {
        clearTimeout(pollingTimeoutRef.current);
      }
      if (elapsedTimerRef.current) {
        clearInterval(elapsedTimerRef.current);
      }
    };
  }, []);

  const fetchData = useCallback(async () => {
    // Don't fetch if component is unmounted
    if (!isMountedRef.current) return;
    
    // Only show loading spinner on first load to avoid page flicker
    if (isFirstLoad.current) {
      setLoading(true);
    }
    setError(null);

    try {
      // First fetch job status
      const statusResponse = await fetch(`/api/backtest/${jobId}/status`);
      const statusData = await statusResponse.json();
      
      // Check if still mounted before updating state
      if (!isMountedRef.current) return;
      
      console.log(`[FetchData] Job ${jobId} status: ${statusData.status}, progress: ${statusData.progress}%`);
      setJobStatus(statusData);

      // Mark first load as complete to stop showing loading spinner on subsequent polls
      isFirstLoad.current = false;

      // If completed, fetch results
      if (statusData.status === 'completed') {
        const resultsResponse = await fetch(`/api/backtest/${jobId}/results`);
        const resultsData = await resultsResponse.json();
        if (isMountedRef.current) {
          setResult(resultsData);
          setLoading(false);
        }
      } else if (statusData.status === 'running' || statusData.status === 'queued') {
        // Job still in progress, auto-retry after 3 seconds
        if (isMountedRef.current) {
          setLoading(false);
          pollingTimeoutRef.current = setTimeout(fetchData, 3000);
        }
      } else if (statusData.status === 'failed') {
        if (isMountedRef.current) {
          setError(statusData.error_message || 'Job failed');
          setLoading(false);
        }
      } else {
        if (isMountedRef.current) {
          setLoading(false);
        }
      }
    } catch (err) {
      if (isMountedRef.current) {
        setError(err instanceof Error ? err.message : 'An error occurred');
        setLoading(false);
      }
    }
  }, [jobId]);

  // Fetch job details once on mount
  useEffect(() => {
    const fetchJobDetails = async () => {
      try {
        const detailsResponse = await fetch(`/api/backtest/${jobId}`);
        const detailsData = await detailsResponse.json();
        setJobDetails(detailsData);
      } catch (err) {
        console.error('Failed to fetch job details:', err);
      }
    };
    fetchJobDetails();
  }, [jobId]);

  // Poll for status updates
  useEffect(() => {
    fetchData();
  }, [fetchData]);

  // Smooth progress interpolation between API updates
  useEffect(() => {
    if (jobStatus && jobStatus.status === 'running' && jobStatus.started_at) {
      console.log(`[Progress Update] API progress: ${jobStatus.progress}%, setting displayProgress`);
      setDisplayProgress(jobStatus.progress);
      
      // Gradually increment progress between updates
      const interval = setInterval(() => {
        setDisplayProgress((prev) => {
          // Small increment per second based on estimated completion rate
          const increment = jobStatus.progress < 99 ? 0.1 : 0;
          return Math.min(prev + increment, jobStatus.progress + 0.5);
        });
      }, 1000);

      return () => clearInterval(interval);
    } else if (jobStatus) {
      setDisplayProgress(jobStatus.progress);
    }
  }, [jobStatus]);

  const downloadReport = (format: 'json' | 'csv') => {
    if (!result) return;

    let content: string;
    let filename: string;
    let mimeType: string;

    if (format === 'json') {
      // Include job configuration alongside results
      const exportData = {
        ...result,
        configuration: jobDetails ? {
          symbol: jobDetails.symbol,
          exchange: jobDetails.exchange,
          strategy: jobDetails.strategy,
          initial_capital: jobDetails.initial_capital,
          optimization_method: jobDetails.optimization_method,
          population_size: jobDetails.population_size,
          generations: jobDetails.generations,
          initial_inventory_pct: jobDetails.initial_inventory_pct,
          initial_30d_volume: jobDetails.initial_30d_volume,
          monte_carlo_enabled: jobDetails.monte_carlo_enabled,
          monte_carlo_runs: jobDetails.monte_carlo_runs,
          walk_forward_enabled: jobDetails.walk_forward_enabled,
          train_ratio: jobDetails.train_ratio,
          test_ratio: jobDetails.test_ratio,
          validate_ratio: jobDetails.validate_ratio,
          auto_save_profitable: jobDetails.auto_save_profitable,
          min_sharpe_ratio: jobDetails.min_sharpe_ratio,
          min_profit_factor: jobDetails.min_profit_factor,
          max_drawdown: jobDetails.max_drawdown,
          min_win_rate: jobDetails.min_win_rate,
          min_trades: jobDetails.min_trades,
          min_sortino_ratio: jobDetails.min_sortino_ratio,
          min_calmar_ratio: jobDetails.min_calmar_ratio,
          min_total_pnl: jobDetails.min_total_pnl,
          fitness_weights: jobDetails.fitness_weights,
          start_date: jobDetails.start_date,
          end_date: jobDetails.end_date,
          commission_rate: jobDetails.commission_rate,
          risk_aversion: jobDetails.risk_aversion,
          inventory_target: jobDetails.inventory_target,
          order_size: jobDetails.order_size,
          order_size_pct: jobDetails.order_size_pct,
          window_ms: jobDetails.window_ms,
          min_quote_lifetime_ms: jobDetails.min_quote_lifetime_ms,
          volatility_cap: jobDetails.volatility_cap,
          time_horizon_minutes: jobDetails.time_horizon_minutes,
          market_depth_k: jobDetails.market_depth_k,
          volatility_adjustment_factor: jobDetails.volatility_adjustment_factor,
          inventory_penalty_factor: jobDetails.inventory_penalty_factor,
          trend_sensitivity: jobDetails.trend_sensitivity,
          flow_sensitivity: jobDetails.flow_sensitivity,
          take_profit_pct: jobDetails.take_profit_pct,
          stop_loss_pct: jobDetails.stop_loss_pct,
          min_spread_bps: jobDetails.min_spread_bps,
          max_trades_per_minute: jobDetails.max_trades_per_minute,
        } : null,
      };
      content = JSON.stringify(exportData, null, 2);
      filename = `backtest_${jobId.substring(0, 8)}.json`;
      mimeType = 'application/json';
    } else {
      // CSV format - include configuration columns
      const headers = [
        // Results
        'Job ID', 'Symbol', 'Exchange', 'Sharpe Ratio', 'Max Drawdown (%)',
        'Total P&L', 'Win Rate (%)', 'Profit Factor', 'Total Trades', 'Avg Time in Trade (h)',
        // Configuration
        'Strategy', 'Initial Capital', 'Optimization Method', 'Population Size', 'Generations',
        'Initial Inventory %', 'Initial 30d Volume ($)', 'Monte Carlo Enabled', 'Monte Carlo Runs',
        'Walk Forward Enabled', 'Train Ratio', 'Test Ratio', 'Validate Ratio',
        'Commission Rate', 'Start Date', 'End Date',
        // Strategy Parameters
        'Risk Aversion', 'Inventory Target', 'Order Size %', 'Window (ms)',
        'Min Quote Lifetime (ms)', 'Volatility Cap', 'Time Horizon (min)', 'Market Depth K',
        'Volatility Adjustment', 'Inventory Penalty', 'Trend Sensitivity', 'Flow Sensitivity',
        'Take Profit %', 'Stop Loss %', 'Min Spread (bps)', 'Max Trades/Min',
        // Profitability Criteria
        'Auto Save', 'Min Sharpe', 'Min Profit Factor', 'Max Drawdown Criteria', 'Min Win Rate', 'Min Trades',
      ];
      
      const row = [
        // Results
        result.job_id ?? 'N/A',
        result.symbol ?? 'N/A',
        result.exchange ?? 'N/A',
        safeToFixed(result.sharpe_ratio, 4),
        safeToFixed((result.max_drawdown ?? 0) * 100, 2),
        safeToFixed(result.total_pnl, 2),
        safeToFixed((result.win_rate ?? 0) * 100, 2),
        safeToFixed(result.profit_factor, 2),
        (result.total_trades ?? 0).toString(),
        safeToFixed(result.avg_time_in_trade, 2),
        // Configuration
        jobDetails?.strategy ?? 'N/A',
        jobDetails?.initial_capital?.toString() ?? 'N/A',
        jobDetails?.optimization_method ?? 'N/A',
        jobDetails?.population_size?.toString() ?? 'N/A',
        jobDetails?.generations?.toString() ?? 'N/A',
        safeToFixed((jobDetails?.initial_inventory_pct ?? 0) * 100, 1),
        safeToFixed(jobDetails?.initial_30d_volume ?? 0, 0),
        jobDetails?.monte_carlo_enabled ? 'Yes' : 'No',
        jobDetails?.monte_carlo_runs?.toString() ?? 'N/A',
        jobDetails?.walk_forward_enabled ? 'Yes' : 'No',
        safeToFixed((jobDetails?.train_ratio ?? 0) * 100, 0),
        safeToFixed((jobDetails?.test_ratio ?? 0) * 100, 0),
        safeToFixed((jobDetails?.validate_ratio ?? 0) * 100, 0),
        safeToFixed((jobDetails?.commission_rate ?? 0) * 100, 4),
        jobDetails?.start_date ?? 'N/A',
        jobDetails?.end_date ?? 'N/A',
        // Strategy Parameters
        safeToFixed(jobDetails?.risk_aversion, 4),
        safeToFixed(jobDetails?.inventory_target, 4),
        safeToFixed((jobDetails?.order_size_pct ?? 0) * 100, 2),
        jobDetails?.window_ms?.toString() ?? 'N/A',
        jobDetails?.min_quote_lifetime_ms?.toString() ?? 'N/A',
        safeToFixed((jobDetails?.volatility_cap ?? 0) * 100, 2),
        jobDetails?.time_horizon_minutes?.toString() ?? 'N/A',
        safeToFixed(jobDetails?.market_depth_k, 2),
        safeToFixed(jobDetails?.volatility_adjustment_factor, 2),
        safeToFixed(jobDetails?.inventory_penalty_factor, 2),
        safeToFixed(jobDetails?.trend_sensitivity, 2),
        safeToFixed(jobDetails?.flow_sensitivity, 2),
        safeToFixed((jobDetails?.take_profit_pct ?? 0) * 100, 2),
        safeToFixed((jobDetails?.stop_loss_pct ?? 0) * 100, 2),
        safeToFixed(jobDetails?.min_spread_bps, 1),
        safeToFixed(jobDetails?.max_trades_per_minute, 1),
        // Profitability Criteria
        jobDetails?.auto_save_profitable ? 'Yes' : 'No',
        safeToFixed(jobDetails?.min_sharpe_ratio, 2),
        safeToFixed(jobDetails?.min_profit_factor, 2),
        safeToFixed((jobDetails?.max_drawdown ?? 0) * 100, 1),
        safeToFixed((jobDetails?.min_win_rate ?? 0) * 100, 1),
        jobDetails?.min_trades?.toString() ?? 'N/A',
      ];
      
      content = headers.join(',') + '\n' + row.join(',');
      filename = `backtest_${jobId.substring(0, 8)}.csv`;
      mimeType = 'text/csv';
    }

    const blob = new Blob([content], { type: mimeType });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = filename;
    link.click();
    URL.revokeObjectURL(url);
  };

  if (loading) {
    return (
      <div style={{ padding: '20px', textAlign: 'center' }}>
        <div style={{ fontSize: '18px', color: '#666' }}>Loading results...</div>
      </div>
    );
  }

  // Show progress for running/queued jobs
  if (jobStatus && (jobStatus.status === 'running' || jobStatus.status === 'queued')) {
    const formatTime = (ms: number) => {
      const seconds = Math.floor(ms / 1000);
      const minutes = Math.floor(seconds / 60);
      const hours = Math.floor(minutes / 60);
      
      if (hours > 0) {
        return `${hours}h ${minutes % 60}m`;
      } else if (minutes > 0) {
        return `${minutes}m ${seconds % 60}s`;
      } else {
        return `${seconds}s`;
      }
    };

    return (
      <div style={{ padding: '20px', maxWidth: '1000px', margin: '0 auto' }}>
        <div style={{ marginBottom: '20px' }}>
          <h2 style={{ margin: '0 0 10px 0' }}>Backtest In Progress</h2>
          <div style={{ fontSize: '14px', color: '#666', display: 'flex', alignItems: 'center', gap: '8px' }}>
            Job ID: <code>{jobId}</code>
            <button
              onClick={() => copyToClipboard(jobId)}
              style={{
                padding: '2px 8px',
                fontSize: '12px',
                backgroundColor: copied ? '#28a745' : '#6c757d',
                color: 'white',
                border: 'none',
                borderRadius: '4px',
                cursor: 'pointer',
              }}
              title="Copy Job ID to clipboard"
            >
              {copied ? '✓ Copied' : '📋 Copy'}
            </button>
          </div>
        </div>

        {/* Job Details Card - Comprehensive Configuration */}
        {jobDetails && (
          <div style={{
            padding: '20px',
            backgroundColor: 'white',
            border: '1px solid #dee2e6',
            borderRadius: '8px',
            marginBottom: '20px',
            boxShadow: '0 2px 4px rgba(0,0,0,0.05)',
          }}>
            <h3 style={{ marginTop: 0, marginBottom: '15px' }}>📋 Backtest Configuration</h3>
            
            {/* Basic Settings */}
            <div style={{ marginBottom: '15px' }}>
              <h4 style={{ margin: '0 0 10px 0', fontSize: '14px', color: '#495057' }}>Basic Settings</h4>
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: '10px', fontSize: '13px' }}>
                {/* Show portfolio assets if in portfolio mode, otherwise show single symbol */}
                {jobDetails.is_portfolio && jobDetails.portfolio_assets && jobDetails.portfolio_assets.length > 0 ? (
                  <div style={{ gridColumn: 'span 2' }}>
                    <strong>Portfolio ({jobDetails.portfolio_assets.length} assets):</strong>{' '}
                    {jobDetails.portfolio_assets.map((a, i) => (
                      <span key={i} style={{ marginRight: '8px', padding: '2px 6px', backgroundColor: '#e7f3ff', borderRadius: '4px', fontSize: '12px' }}>
                        {a.symbol}@{a.exchange}{a.weight ? ` (${(a.weight * 100).toFixed(0)}%)` : ''}
                      </span>
                    ))}
                  </div>
                ) : (
                  <>
                    <div><strong>Symbol:</strong> {jobDetails.symbol}</div>
                    <div><strong>Exchange:</strong> {jobDetails.exchange}</div>
                  </>
                )}
                <div><strong>Strategy:</strong> {jobDetails.strategy}</div>
                <div><strong>Initial Capital:</strong> ${jobDetails.initial_capital.toLocaleString()}</div>
                <div><strong>Initial Inventory:</strong> {safeToFixed((jobDetails.initial_inventory_pct ?? 0) * 100, 0)}%</div>
                <div><strong>30d Volume:</strong> {(jobDetails.initial_30d_volume ?? 0) > 0 ? `$${(jobDetails.initial_30d_volume ?? 0).toLocaleString()}` : 'Default Tier'}</div>
                <div><strong>Commission Rate:</strong> {safeToFixed((jobDetails.commission_rate ?? 0) * 100, 4)}%</div>
                <div><strong>Start Date:</strong> {jobDetails.start_date ? new Date(jobDetails.start_date).toLocaleDateString() : 'Auto (earliest data)'}</div>
                <div><strong>End Date:</strong> {jobDetails.end_date ? new Date(jobDetails.end_date).toLocaleDateString() : 'Auto (latest data)'}</div>
              </div>
            </div>

            {/* Optimization Settings */}
            <div style={{ marginBottom: '15px', padding: '10px', backgroundColor: '#f8f9fa', borderRadius: '4px' }}>
              <h4 style={{ margin: '0 0 10px 0', fontSize: '14px', color: '#495057' }}>🧬 Optimization Settings</h4>
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: '10px', fontSize: '13px' }}>
                <div><strong>Method:</strong> {jobDetails.optimization_method}</div>
                {jobDetails.optimization_method === 'genetic' && (
                  <>
                    <div><strong>Population:</strong> {jobDetails.population_size}</div>
                    <div><strong>Generations:</strong> {jobDetails.generations}</div>
                    <div><strong>Total Sims:</strong> {(jobDetails.population_size ?? 0) * (jobDetails.generations ?? 0)}</div>
                  </>
                )}
                <div><strong>Train:</strong> {safeToFixed((jobDetails.train_ratio ?? 0) * 100, 0)}%</div>
                <div><strong>Test:</strong> {safeToFixed((jobDetails.test_ratio ?? 0) * 100, 0)}%</div>
                <div><strong>Validate:</strong> {safeToFixed((jobDetails.validate_ratio ?? 0) * 100, 0)}%</div>
              </div>
            </div>

            {/* Analysis Settings */}
            <div style={{ marginBottom: '15px', padding: '10px', backgroundColor: '#e7f3ff', borderRadius: '4px' }}>
              <h4 style={{ margin: '0 0 10px 0', fontSize: '14px', color: '#495057' }}>📊 Analysis Settings</h4>
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: '10px', fontSize: '13px' }}>
                <div><strong>Monte Carlo:</strong> {jobDetails.monte_carlo_enabled ? `✅ ${jobDetails.monte_carlo_runs} runs` : '❌ Disabled'}</div>
                <div><strong>Walk-Forward:</strong> {jobDetails.walk_forward_enabled ? '✅ Enabled' : '❌ Disabled'}</div>
                <div><strong>Auto-Save:</strong> {jobDetails.auto_save_profitable ? '✅ Enabled' : '❌ Disabled'}</div>
              </div>
            </div>

            {/* Fitness Weights (if custom) */}
            {jobDetails.fitness_weights && (
              <div style={{ marginBottom: '15px', padding: '10px', backgroundColor: '#fff3cd', borderRadius: '4px' }}>
                <h4 style={{ margin: '0 0 10px 0', fontSize: '14px', color: '#495057' }}>⚖️ Custom Fitness Weights</h4>
                <div style={{ display: 'grid', gridTemplateColumns: 'repeat(6, 1fr)', gap: '10px', fontSize: '13px' }}>
                  <div><strong>Sharpe:</strong> {safeToFixed(jobDetails.fitness_weights.sharpe * 100, 0)}%</div>
                  <div><strong>Sortino:</strong> {safeToFixed(jobDetails.fitness_weights.sortino * 100, 0)}%</div>
                  <div><strong>Profit Factor:</strong> {safeToFixed(jobDetails.fitness_weights.profit_factor * 100, 0)}%</div>
                  <div><strong>Win Rate:</strong> {safeToFixed(jobDetails.fitness_weights.win_rate * 100, 0)}%</div>
                  <div><strong>Net Profit:</strong> {safeToFixed(jobDetails.fitness_weights.net_profit * 100, 0)}%</div>
                  <div><strong>Calmar:</strong> {safeToFixed(jobDetails.fitness_weights.calmar * 100, 0)}%</div>
                </div>
              </div>
            )}

            {/* Profitability Criteria */}
            {jobDetails.auto_save_profitable && (
              <div style={{ marginBottom: '15px', padding: '10px', backgroundColor: '#d4edda', borderRadius: '4px' }}>
                <h4 style={{ margin: '0 0 10px 0', fontSize: '14px', color: '#495057' }}>✅ Profitability Criteria</h4>
                <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: '10px', fontSize: '13px' }}>
                  <div><strong>Min Sharpe:</strong> {safeToFixed(jobDetails.min_sharpe_ratio, 2)}</div>
                  <div><strong>Min Profit Factor:</strong> {safeToFixed(jobDetails.min_profit_factor, 2)}</div>
                  <div><strong>Max Drawdown:</strong> {safeToFixed((jobDetails.max_drawdown ?? 0) * 100, 1)}%</div>
                  <div><strong>Min Win Rate:</strong> {safeToFixed((jobDetails.min_win_rate ?? 0) * 100, 1)}%</div>
                  <div><strong>Min Trades:</strong> {jobDetails.min_trades}</div>
                  {jobDetails.min_sortino_ratio && <div><strong>Min Sortino:</strong> {safeToFixed(jobDetails.min_sortino_ratio, 2)}</div>}
                  {jobDetails.min_calmar_ratio && <div><strong>Min Calmar:</strong> {safeToFixed(jobDetails.min_calmar_ratio, 2)}</div>}
                  {jobDetails.min_total_pnl && <div><strong>Min P&L:</strong> ${jobDetails.min_total_pnl.toLocaleString()}</div>}
                </div>
              </div>
            )}

            {/* Strategy Parameters - Only show for manual mode (not GA), since GA optimizes these */}
            {jobDetails.optimization_method !== 'genetic' && (
              <details style={{ marginBottom: '10px' }}>
                <summary style={{ cursor: 'pointer', fontWeight: 'bold', fontSize: '14px', color: '#495057' }}>
                  ⚙️ Strategy Parameters (click to expand)
                </summary>
                <div style={{ marginTop: '10px', display: 'grid', gridTemplateColumns: '1fr 1fr 1fr 1fr', gap: '10px', fontSize: '13px', padding: '10px', backgroundColor: '#f8f9fa', borderRadius: '4px' }}>
                  <div><strong>Risk Aversion:</strong> {safeToFixed(jobDetails.risk_aversion, 4)}</div>
                  <div><strong>Inventory Target:</strong> {safeToFixed(jobDetails.inventory_target, 4)}</div>
                  <div><strong>Order Size:</strong> {safeToFixed((jobDetails.order_size_pct ?? 0) * 100, 2)}%</div>
                  <div><strong>Window:</strong> {jobDetails.window_ms}ms</div>
                  <div><strong>Min Quote Life:</strong> {jobDetails.min_quote_lifetime_ms}ms</div>
                  <div><strong>Volatility Cap:</strong> {safeToFixed((jobDetails.volatility_cap ?? 0) * 100, 2)}%</div>
                  <div><strong>Time Horizon:</strong> {jobDetails.time_horizon_minutes} min</div>
                  <div><strong>Market Depth K:</strong> {safeToFixed(jobDetails.market_depth_k, 2)}</div>
                  <div><strong>Vol Adjustment:</strong> {safeToFixed(jobDetails.volatility_adjustment_factor, 2)}</div>
                  <div><strong>Inv Penalty:</strong> {safeToFixed(jobDetails.inventory_penalty_factor, 2)}</div>
                  <div><strong>Trend Sens:</strong> {safeToFixed(jobDetails.trend_sensitivity, 2)}</div>
                  <div><strong>Flow Sens:</strong> {safeToFixed(jobDetails.flow_sensitivity, 2)}</div>
                  <div><strong>Take Profit:</strong> {safeToFixed((jobDetails.take_profit_pct ?? 0) * 100, 2)}%</div>
                  <div><strong>Stop Loss:</strong> {safeToFixed((jobDetails.stop_loss_pct ?? 0) * 100, 2)}%</div>
                  <div><strong>Min Spread:</strong> {safeToFixed(jobDetails.min_spread_bps, 1)} bps</div>
                  <div><strong>Max Trades/Min:</strong> {safeToFixed(jobDetails.max_trades_per_minute, 1)}</div>
                </div>
              </details>
            )}
          </div>
        )}

        {/* Progress Card */}
        <div style={{
          padding: '20px',
          backgroundColor: '#e7f3ff',
          border: '1px solid #b3d9ff',
          borderRadius: '8px',
          marginBottom: '20px',
        }}>
          <div style={{ marginBottom: '15px' }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '5px' }}>
              <strong>Status:</strong>
              <span style={{ textTransform: 'capitalize', color: jobStatus.status === 'running' ? '#28a745' : '#ffc107' }}>
                {jobStatus.status === 'running' ? '🏃 Running' : '⏳ Queued'}
              </span>
            </div>
            {jobStatus.current_phase && jobStatus.current_phase !== 'initializing' && (
              <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '5px' }}>
                <strong>Phase:</strong>
                <span style={{ color: '#007bff', fontWeight: 'bold' }}>
                  {(() => {
                    const phaseLabels: { [key: string]: string } = {
                      'data_loading': '📂 Loading Data',
                      'loading_data': '📂 Loading Data',
                      'data_ready': '✅ Data Ready',
                      'preparing': '⚙️ Preparing',
                      'running_backtest': '▶️ Running Backtest',
                      'processing_results': '📊 Processing Results',
                      'genetic_optimization': '🧬 Genetic Optimization',
                      'optimizing': '🧬 Optimizing',
                      'post_optimization': '🎯 Post-Optimization',
                      'monte_carlo': '🎲 Monte Carlo Simulation',
                      'walk_forward': '🔄 Walk-Forward Analysis',
                      'final_analysis': '📊 Final Analysis',
                      'final_evaluation': '🎯 Final Evaluation',
                      'calculating_metrics': '📊 Calculating Metrics',
                      'portfolio_loading': '📂 Portfolio Loading',
                      'portfolio_simulation': '📈 Portfolio Simulation',
                      'portfolio_ga_loading': '📂 Portfolio GA Loading',
                      'portfolio_ga_symbol_optimization': '🧬 Symbol Optimization',
                      'portfolio_ga_weight_optimization': '⚖️ Weight Optimization',
                      'portfolio_final_simulation': '📊 Final Portfolio Simulation',
                      'completed': '✅ Completed',
                      'failed': '❌ Failed',
                    };
                    return phaseLabels[jobStatus.current_phase] || 
                      `🔄 ${jobStatus.current_phase.replace(/_/g, ' ').replace(/\b\w/g, c => c.toUpperCase())}`;
                  })()}
                </span>
              </div>
            )}
            {jobStatus.current_phase === 'monte_carlo' && jobStatus.phase_details?.current && jobStatus.phase_details?.total && (
              <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '5px' }}>
                <strong>Simulation:</strong>
                <span style={{ color: '#007bff', fontWeight: 'bold' }}>
                  {jobStatus.phase_details.current} of {jobStatus.phase_details.total} runs
                </span>
              </div>
            )}
            {jobStatus.current_phase === 'walk_forward' && jobStatus.phase_details?.current && jobStatus.phase_details?.total && (
              <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '5px' }}>
                <strong>Window:</strong>
                <span style={{ color: '#007bff', fontWeight: 'bold' }}>
                  {jobStatus.phase_details.current} of {jobStatus.phase_details.total}
                </span>
              </div>
            )}
            <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '5px' }}>
              <strong>Progress:</strong>
              <span>{safeToFixed(displayProgress, 1)}%</span>
            </div>
            {jobStatus.current_generation !== null && jobStatus.total_generations !== null && jobStatus.total_generations > 0 && (
              <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '5px' }}>
                <strong>Generation:</strong>
                <span style={{ color: '#007bff', fontWeight: 'bold' }}>
                  {jobStatus.current_generation} of {jobStatus.total_generations}
                </span>
              </div>
            )}
          </div>

          <div style={{
            width: '100%',
            height: '30px',
            backgroundColor: '#f0f0f0',
            borderRadius: '15px',
            overflow: 'hidden',
            marginBottom: '15px',
          }}>
            <div style={{
              width: `${displayProgress}%`,
              height: '100%',
              backgroundColor: displayProgress < 100 ? '#007bff' : '#28a745',
              transition: 'width 0.3s ease',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              color: 'white',
              fontWeight: 'bold',
            }}>
              {safeToFixed(displayProgress, 1)}%
            </div>
          </div>

          <div style={{ fontSize: '14px', color: '#666' }}>
            <div><strong>Created:</strong> {new Date(jobStatus.created_at).toLocaleString()}</div>
            {jobStatus.started_at && (
              <>
                <div><strong>Started:</strong> {new Date(jobStatus.started_at).toLocaleString()}</div>
                <div style={{ fontSize: '16px', fontWeight: 'bold', color: '#007bff' }}>
                  <strong>⏱️ Elapsed:</strong> {elapsedTime || '0s'}
                </div>
              </>
            )}
            {/* Stall detection warning */}
            {jobStatus.started_at && jobStatus.progress < 5 && (() => {
              const elapsed = Date.now() - new Date(jobStatus.started_at).getTime();
              const elapsedMinutes = elapsed / (1000 * 60);
              return elapsedMinutes > 5 && (
                <div style={{ marginTop: '10px', padding: '10px', backgroundColor: '#fff3cd', borderRadius: '4px', color: '#856404' }}>
                  ⚠️ <strong>Possible stall detected:</strong> Job has been running for {Math.floor(elapsedMinutes)}+ minutes with minimal progress ({safeToFixed(jobStatus.progress, 1)}%). Check worker logs if this persists.
                </div>
              );
            })()}
            <div style={{ marginTop: '10px', fontStyle: 'italic' }}>
              🔄 Progress updates every 3 seconds...
            </div>
          </div>
        </div>

        <div style={{ display: 'flex', gap: '10px', flexWrap: 'wrap' }}>
          {onBack && (
            <button
              onClick={onBack}
              style={{
                padding: '10px 20px',
                backgroundColor: '#6c757d',
                color: 'white',
                border: 'none',
                borderRadius: '4px',
                cursor: 'pointer',
              }}
            >
              ← Back to Job List
            </button>
          )}
          {jobStatus && (jobStatus.status === 'running' || jobStatus.status === 'queued') && (
            <button
              onClick={cancelJob}
              disabled={actionLoading !== null}
              style={{
                padding: '10px 20px',
                backgroundColor: '#ffc107',
                color: '#000',
                border: 'none',
                borderRadius: '4px',
                cursor: actionLoading ? 'not-allowed' : 'pointer',
                opacity: actionLoading ? 0.6 : 1,
              }}
            >
              {actionLoading === 'cancel' ? '⏳ Cancelling...' : '⚠️ Cancel Job'}
            </button>
          )}
          <button
            onClick={deleteJob}
            disabled={actionLoading !== null}
            style={{
              padding: '10px 20px',
              backgroundColor: '#dc3545',
              color: 'white',
              border: 'none',
              borderRadius: '4px',
              cursor: actionLoading ? 'not-allowed' : 'pointer',
              opacity: actionLoading ? 0.6 : 1,
            }}
          >
            {actionLoading === 'delete' ? '⏳ Deleting...' : '🗑️ Delete Job'}
          </button>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div style={{ padding: '20px' }}>
        <div style={{
          padding: '15px',
          backgroundColor: '#f8d7da',
          border: '1px solid #f5c6cb',
          borderRadius: '4px',
          color: '#721c24',
          marginBottom: '15px',
        }}>
          <strong>Error:</strong> {error}
        </div>
        {jobStatus && (
          <div style={{ marginBottom: '15px', padding: '10px', backgroundColor: '#f8f9fa', borderRadius: '4px' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
              <strong>Job ID:</strong> {jobStatus.job_id}
              <button
                onClick={() => copyToClipboard(jobStatus.job_id)}
                style={{
                  padding: '2px 8px',
                  fontSize: '12px',
                  backgroundColor: copied ? '#28a745' : '#6c757d',
                  color: 'white',
                  border: 'none',
                  borderRadius: '4px',
                  cursor: 'pointer',
                }}
                title="Copy Job ID to clipboard"
              >
                {copied ? '✓ Copied' : '📋 Copy'}
              </button>
            </div>
            <div><strong>Status:</strong> {jobStatus.status}</div>
            {jobStatus.error_message && (
              <div><strong>Error Details:</strong> {jobStatus.error_message}</div>
            )}
          </div>
        )}
        <div style={{ display: 'flex', gap: '10px' }}>
          {onBack && (
            <button
              onClick={onBack}
              style={{
                padding: '8px 16px',
                backgroundColor: '#007bff',
                color: 'white',
                border: 'none',
                borderRadius: '4px',
                cursor: 'pointer',
              }}
            >
              ← Back to Job List
            </button>
          )}
          <button
            onClick={deleteJob}
            disabled={actionLoading !== null}
            style={{
              padding: '8px 16px',
              backgroundColor: '#dc3545',
              color: 'white',
              border: 'none',
              borderRadius: '4px',
              cursor: actionLoading ? 'not-allowed' : 'pointer',
              opacity: actionLoading ? 0.6 : 1,
            }}
          >
            {actionLoading === 'delete' ? '⏳ Deleting...' : '🗑️ Delete Job'}
          </button>
        </div>
      </div>
    );
  }

  if (!result) {
    return <div style={{ padding: '20px' }}>No results available</div>;
  }

  return (
    <div className="results-container" style={{ padding: '20px', maxWidth: '1400px', margin: '0 auto' }}>
      {/* Header */}
      <div style={{ 
        display: 'flex', 
        justifyContent: 'space-between', 
        alignItems: 'center',
        marginBottom: '20px',
      }}>
        <div>
          <h2>Backtest Results</h2>
          <div style={{ color: '#666', fontSize: '14px', display: 'flex', alignItems: 'center', gap: '8px' }}>
            Job ID: <code>{result.job_id ?? 'N/A'}</code>
            <button
              onClick={() => copyToClipboard(result.job_id ?? '')}
              style={{
                padding: '2px 8px',
                fontSize: '12px',
                backgroundColor: copied ? '#28a745' : '#6c757d',
                color: 'white',
                border: 'none',
                borderRadius: '4px',
                cursor: 'pointer',
              }}
              title="Copy Job ID to clipboard"
            >
              {copied ? '✓ Copied' : '📋 Copy'}
            </button>
            <span>| {result.symbol ?? 'Unknown'} on {result.exchange ?? 'Unknown'}</span>
          </div>
        </div>
        <div style={{ display: 'flex', gap: '10px' }}>
          <button
            onClick={() => downloadReport('json')}
            style={{
              padding: '8px 16px',
              backgroundColor: '#28a745',
              color: 'white',
              border: 'none',
              borderRadius: '4px',
              cursor: 'pointer',
            }}
          >
            Download JSON
          </button>
          <button
            onClick={() => downloadReport('csv')}
            style={{
              padding: '8px 16px',
              backgroundColor: '#17a2b8',
              color: 'white',
              border: 'none',
              borderRadius: '4px',
              cursor: 'pointer',
            }}
          >
            Download CSV
          </button>
          {onBack && (
            <button
              onClick={onBack}
              style={{
                padding: '8px 16px',
                backgroundColor: '#6c757d',
                color: 'white',
                border: 'none',
                borderRadius: '4px',
                cursor: 'pointer',
              }}
            >
              Back
            </button>
          )}
          <button
            onClick={deleteJob}
            disabled={actionLoading !== null}
            style={{
              padding: '8px 16px',
              backgroundColor: '#dc3545',
              color: 'white',
              border: 'none',
              borderRadius: '4px',
              cursor: actionLoading ? 'not-allowed' : 'pointer',
              opacity: actionLoading ? 0.6 : 1,
            }}
          >
            {actionLoading === 'delete' ? '⏳ Deleting...' : '🗑️ Delete'}
          </button>
        </div>
      </div>

      {/* Configuration Section - Collapsible */}
      {jobDetails && (
        <details style={{ marginBottom: '20px' }}>
          <summary style={{ 
            cursor: 'pointer', 
            padding: '15px', 
            backgroundColor: '#f8f9fa', 
            border: '1px solid #dee2e6',
            borderRadius: '8px',
            fontWeight: 'bold',
            fontSize: '16px',
          }}>
            📋 Backtest Configuration (click to expand)
          </summary>
          <div style={{
            padding: '20px',
            backgroundColor: 'white',
            border: '1px solid #dee2e6',
            borderTop: 'none',
            borderRadius: '0 0 8px 8px',
          }}>
            {/* Basic Settings */}
            <div style={{ marginBottom: '15px' }}>
              <h4 style={{ margin: '0 0 10px 0', fontSize: '14px', color: '#495057' }}>Basic Settings</h4>
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr 1fr', gap: '10px', fontSize: '13px' }}>
                <div><strong>Symbol:</strong> {jobDetails.symbol}</div>
                <div><strong>Exchange:</strong> {jobDetails.exchange}</div>
                <div><strong>Strategy:</strong> {jobDetails.strategy}</div>
                <div><strong>Initial Capital:</strong> ${jobDetails.initial_capital.toLocaleString()}</div>
                <div><strong>Initial Inventory:</strong> {safeToFixed((jobDetails.initial_inventory_pct ?? 0) * 100, 0)}%</div>
                <div><strong>Commission Rate:</strong> {safeToFixed((jobDetails.commission_rate ?? 0) * 100, 4)}%</div>
                <div><strong>Start Date:</strong> {jobDetails.start_date ? new Date(jobDetails.start_date).toLocaleDateString() : 'N/A'}</div>
                <div><strong>End Date:</strong> {jobDetails.end_date ? new Date(jobDetails.end_date).toLocaleDateString() : 'N/A'}</div>
              </div>
            </div>

            {/* Optimization Settings */}
            <div style={{ marginBottom: '15px', padding: '10px', backgroundColor: '#f8f9fa', borderRadius: '4px' }}>
              <h4 style={{ margin: '0 0 10px 0', fontSize: '14px', color: '#495057' }}>🧬 Optimization Settings</h4>
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr 1fr', gap: '10px', fontSize: '13px' }}>
                <div><strong>Method:</strong> {jobDetails.optimization_method}</div>
                {jobDetails.optimization_method === 'genetic' && (
                  <>
                    <div><strong>Population:</strong> {jobDetails.population_size}</div>
                    <div><strong>Generations:</strong> {jobDetails.generations}</div>
                    <div><strong>Total Sims:</strong> {(jobDetails.population_size ?? 0) * (jobDetails.generations ?? 0)}</div>
                  </>
                )}
                <div><strong>Train:</strong> {safeToFixed((jobDetails.train_ratio ?? 0) * 100, 0)}%</div>
                <div><strong>Test:</strong> {safeToFixed((jobDetails.test_ratio ?? 0) * 100, 0)}%</div>
                <div><strong>Validate:</strong> {safeToFixed((jobDetails.validate_ratio ?? 0) * 100, 0)}%</div>
              </div>
            </div>

            {/* Analysis Settings */}
            <div style={{ marginBottom: '15px', padding: '10px', backgroundColor: '#e7f3ff', borderRadius: '4px' }}>
              <h4 style={{ margin: '0 0 10px 0', fontSize: '14px', color: '#495057' }}>📊 Analysis Settings</h4>
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: '10px', fontSize: '13px' }}>
                <div><strong>Monte Carlo:</strong> {jobDetails.monte_carlo_enabled ? `✅ ${jobDetails.monte_carlo_runs} runs` : '❌ Disabled'}</div>
                <div><strong>Walk-Forward:</strong> {jobDetails.walk_forward_enabled ? '✅ Enabled' : '❌ Disabled'}</div>
                <div><strong>Auto-Save:</strong> {jobDetails.auto_save_profitable ? '✅ Enabled' : '❌ Disabled'}</div>
              </div>
            </div>

            {/* Fitness Weights (if custom) */}
            {jobDetails.fitness_weights && (
              <div style={{ marginBottom: '15px', padding: '10px', backgroundColor: '#fff3cd', borderRadius: '4px' }}>
                <h4 style={{ margin: '0 0 10px 0', fontSize: '14px', color: '#495057' }}>⚖️ Custom Fitness Weights</h4>
                <div style={{ display: 'grid', gridTemplateColumns: 'repeat(6, 1fr)', gap: '10px', fontSize: '13px' }}>
                  <div><strong>Sharpe:</strong> {safeToFixed(jobDetails.fitness_weights.sharpe * 100, 0)}%</div>
                  <div><strong>Sortino:</strong> {safeToFixed(jobDetails.fitness_weights.sortino * 100, 0)}%</div>
                  <div><strong>Profit Factor:</strong> {safeToFixed(jobDetails.fitness_weights.profit_factor * 100, 0)}%</div>
                  <div><strong>Win Rate:</strong> {safeToFixed(jobDetails.fitness_weights.win_rate * 100, 0)}%</div>
                  <div><strong>Net Profit:</strong> {safeToFixed(jobDetails.fitness_weights.net_profit * 100, 0)}%</div>
                  <div><strong>Calmar:</strong> {safeToFixed(jobDetails.fitness_weights.calmar * 100, 0)}%</div>
                </div>
              </div>
            )}

            {/* Profitability Criteria */}
            {jobDetails.auto_save_profitable && (
              <div style={{ marginBottom: '15px', padding: '10px', backgroundColor: '#d4edda', borderRadius: '4px' }}>
                <h4 style={{ margin: '0 0 10px 0', fontSize: '14px', color: '#495057' }}>✅ Profitability Criteria</h4>
                <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: '10px', fontSize: '13px' }}>
                  <div><strong>Min Sharpe:</strong> {safeToFixed(jobDetails.min_sharpe_ratio, 2)}</div>
                  <div><strong>Min Profit Factor:</strong> {safeToFixed(jobDetails.min_profit_factor, 2)}</div>
                  <div><strong>Max Drawdown:</strong> {safeToFixed((jobDetails.max_drawdown ?? 0) * 100, 1)}%</div>
                  <div><strong>Min Win Rate:</strong> {safeToFixed((jobDetails.min_win_rate ?? 0) * 100, 1)}%</div>
                  <div><strong>Min Trades:</strong> {jobDetails.min_trades}</div>
                  {jobDetails.min_sortino_ratio && <div><strong>Min Sortino:</strong> {safeToFixed(jobDetails.min_sortino_ratio, 2)}</div>}
                  {jobDetails.min_calmar_ratio && <div><strong>Min Calmar:</strong> {safeToFixed(jobDetails.min_calmar_ratio, 2)}</div>}
                  {jobDetails.min_total_pnl && <div><strong>Min P&L:</strong> ${jobDetails.min_total_pnl.toLocaleString()}</div>}
                </div>
              </div>
            )}

            {/* Strategy Parameters */}
            <div style={{ padding: '10px', backgroundColor: '#f8f9fa', borderRadius: '4px' }}>
              <h4 style={{ margin: '0 0 10px 0', fontSize: '14px', color: '#495057' }}>⚙️ Strategy Parameters</h4>
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr 1fr', gap: '10px', fontSize: '13px' }}>
                <div><strong>Risk Aversion:</strong> {safeToFixed(jobDetails.risk_aversion, 4)}</div>
                <div><strong>Inventory Target:</strong> {safeToFixed(jobDetails.inventory_target, 4)}</div>
                <div><strong>Order Size:</strong> {safeToFixed((jobDetails.order_size_pct ?? 0) * 100, 2)}%</div>
                <div><strong>Window:</strong> {jobDetails.window_ms}ms</div>
                <div><strong>Min Quote Life:</strong> {jobDetails.min_quote_lifetime_ms}ms</div>
                <div><strong>Volatility Cap:</strong> {safeToFixed((jobDetails.volatility_cap ?? 0) * 100, 2)}%</div>
                <div><strong>Time Horizon:</strong> {jobDetails.time_horizon_minutes} min</div>
                <div><strong>Market Depth K:</strong> {safeToFixed(jobDetails.market_depth_k, 2)}</div>
                <div><strong>Vol Adjustment:</strong> {safeToFixed(jobDetails.volatility_adjustment_factor, 2)}</div>
                <div><strong>Inv Penalty:</strong> {safeToFixed(jobDetails.inventory_penalty_factor, 2)}</div>
                <div><strong>Trend Sens:</strong> {safeToFixed(jobDetails.trend_sensitivity, 2)}</div>
                <div><strong>Flow Sens:</strong> {safeToFixed(jobDetails.flow_sensitivity, 2)}</div>
                <div><strong>Take Profit:</strong> {safeToFixed((jobDetails.take_profit_pct ?? 0) * 100, 2)}%</div>
                <div><strong>Stop Loss:</strong> {safeToFixed((jobDetails.stop_loss_pct ?? 0) * 100, 2)}%</div>
                <div><strong>Min Spread:</strong> {safeToFixed(jobDetails.min_spread_bps, 1)} bps</div>
                <div><strong>Max Trades/Min:</strong> {safeToFixed(jobDetails.max_trades_per_minute, 1)}</div>
              </div>
            </div>
          </div>
        </details>
      )}

      {/* Metrics Cards */}
      <div style={{
        display: 'grid',
        gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))',
        gap: '15px',
        marginBottom: '30px',
      }}>
        <MetricCard
          title="Sharpe Ratio"
          value={safeToFixed(result.sharpe_ratio, 4)}
          color={(result.sharpe_ratio ?? 0) > 1 ? '#28a745' : (result.sharpe_ratio ?? 0) > 0 ? '#ffc107' : '#dc3545'}
          tooltip="Risk-adjusted return measure. Above 1 is good, above 2 is excellent. Higher values indicate better return per unit of risk taken."
        />
        <MetricCard
          title="Max Drawdown"
          value={`${safeToFixed((result.max_drawdown ?? 0) * 100, 2)}%`}
          color={(result.max_drawdown ?? 0) < 0.10 ? '#28a745' : (result.max_drawdown ?? 0) < 0.20 ? '#ffc107' : '#dc3545'}
          tooltip="Largest peak-to-trough decline in portfolio value. Lower is better. Represents the worst-case loss you would have experienced."
        />
        <MetricCard
          title="Total P&L"
          value={safeToFixed(result.total_pnl, 2)}
          color={(result.total_pnl ?? 0) >= 0 ? '#28a745' : '#dc3545'}
          tooltip="Net profit or loss after all trades. The absolute dollar return of the strategy over the backtest period."
        />
        <MetricCard
          title="Win Rate"
          value={`${safeToFixed((result.win_rate ?? 0) * 100, 2)}%`}
          color={(result.win_rate ?? 0) > 0.50 ? '#28a745' : '#ffc107'}
          tooltip="Percentage of trades that were profitable. For market-making strategies, 50%+ is typical. Combine with avg trade size for full picture."
        />
        <MetricCard
          title="Profit Factor"
          value={safeToFixed(result.profit_factor, 2)}
          color={(result.profit_factor ?? 0) > 1.5 ? '#28a745' : (result.profit_factor ?? 0) > 1 ? '#ffc107' : '#dc3545'}
          tooltip="Gross profit divided by gross loss. Above 1 means profitable. Above 1.5 is good, above 2.0 is excellent."
        />
        <MetricCard
          title="Total Trades"
          value={(result.total_trades ?? 0).toString()}
          color="#007bff"
          tooltip="Number of completed round-trip trades. More trades = more statistical significance. Low trade count may indicate overfitting risk."
        />
      </div>

      {/* Portfolio Results Section - Check both portfolio_metrics AND strategy_params fallback */}
      {(() => {
        // Detect portfolio mode from multiple sources
        const isPortfolioMode = result.is_portfolio || 
          result.strategy_params?.portfolio_mode === true ||
          (result.strategy_params?.symbol_results && result.strategy_params.symbol_results.length > 0);
        
        // Get symbol results from portfolio_metrics or fallback to strategy_params
        const symbolResults = result.portfolio_metrics?.symbol_results || 
          result.strategy_params?.symbol_results?.map((sr: any) => ({
            symbol: sr.symbol,
            exchange: sr.symbol?.split('@')[1] || result.exchange,
            sharpe_ratio: sr.sharpe_ratio ?? sr.sharpe,
            max_drawdown: sr.max_drawdown,
            total_pnl: sr.total_pnl,
            win_rate: sr.win_rate,
            profit_factor: sr.profit_factor ?? 0,
            total_trades: sr.total_trades ?? sr.num_trades,
            allocation_weight: sr.allocation_weight
          })) || [];
        
        // Get correlation matrix from portfolio_metrics or fallback to strategy_params
        const correlationMatrix = result.portfolio_metrics?.correlation_matrix ||
          result.strategy_params?.correlation_matrix?.map((cp: any) => ({
            symbol_a: cp.symbol_a ?? cp.symbol1,
            symbol_b: cp.symbol_b ?? cp.symbol2,
            correlation: cp.correlation
          })) || [];
        
        // Calculate portfolio-level metrics if not provided
        const totalSymbols = result.portfolio_metrics?.total_symbols || symbolResults.length;
        const portfolioSharpe = result.portfolio_metrics?.portfolio_sharpe ?? 
          (symbolResults.length > 0 ? symbolResults.reduce((sum: number, sr: any) => sum + (sr.sharpe_ratio || 0), 0) / symbolResults.length : 0);
        const portfolioMaxDD = result.portfolio_metrics?.portfolio_max_drawdown ?? result.max_drawdown ?? 0;
        
        // Calculate totals for portfolio
        const totalPnL = symbolResults.reduce((sum: number, sr: any) => sum + (sr.total_pnl || 0), 0);
        const totalTrades = symbolResults.reduce((sum: number, sr: any) => sum + (sr.total_trades || 0), 0);
        const avgWinRate = symbolResults.length > 0 
          ? symbolResults.reduce((sum: number, sr: any) => sum + (sr.win_rate || 0), 0) / symbolResults.length 
          : 0;
        
        if (!isPortfolioMode || symbolResults.length === 0) return null;
        
        return (
        <>
          {/* ==================== PORTFOLIO OVERVIEW ==================== */}
          <div style={{
            backgroundColor: '#f0f7ff',
            border: '2px solid #0066cc',
            borderRadius: '12px',
            padding: '24px',
            marginBottom: '24px',
          }}>
            <div style={{ 
              display: 'flex', 
              justifyContent: 'space-between', 
              alignItems: 'center',
              marginBottom: '20px'
            }}>
              <h2 style={{ margin: 0, color: '#0066cc', fontSize: '20px' }}>
                📊 Portfolio Overview
              </h2>
              <span style={{ 
                backgroundColor: '#0066cc', 
                color: 'white', 
                padding: '4px 12px', 
                borderRadius: '16px',
                fontSize: '14px',
                fontWeight: 500
              }}>
                {totalSymbols} Assets
              </span>
            </div>
            
            {/* Portfolio Summary Cards */}
            <div style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(auto-fit, minmax(160px, 1fr))',
              gap: '16px',
              marginBottom: '20px'
            }}>
              <div style={{ 
                backgroundColor: 'white', 
                padding: '16px', 
                borderRadius: '8px',
                textAlign: 'center',
                boxShadow: '0 2px 4px rgba(0,0,0,0.05)'
              }}>
                <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Portfolio Sharpe</div>
                <div style={{ fontSize: '28px', fontWeight: 700, color: portfolioSharpe > 1.5 ? '#28a745' : portfolioSharpe > 0 ? '#ffc107' : '#dc3545' }}>
                  {safeToFixed(portfolioSharpe, 2)}
                </div>
              </div>
              <div style={{ 
                backgroundColor: 'white', 
                padding: '16px', 
                borderRadius: '8px',
                textAlign: 'center',
                boxShadow: '0 2px 4px rgba(0,0,0,0.05)'
              }}>
                <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Max Drawdown</div>
                <div style={{ fontSize: '28px', fontWeight: 700, color: portfolioMaxDD > 0.2 ? '#dc3545' : portfolioMaxDD > 0.1 ? '#ffc107' : '#28a745' }}>
                  {safeToFixed(portfolioMaxDD * 100, 1)}%
                </div>
              </div>
              <div style={{ 
                backgroundColor: 'white', 
                padding: '16px', 
                borderRadius: '8px',
                textAlign: 'center',
                boxShadow: '0 2px 4px rgba(0,0,0,0.05)'
              }}>
                <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Total P&L</div>
                <div style={{ fontSize: '28px', fontWeight: 700, color: totalPnL >= 0 ? '#28a745' : '#dc3545' }}>
                  ${formatLargeNumber(totalPnL)}
                </div>
              </div>
              <div style={{ 
                backgroundColor: 'white', 
                padding: '16px', 
                borderRadius: '8px',
                textAlign: 'center',
                boxShadow: '0 2px 4px rgba(0,0,0,0.05)'
              }}>
                <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Avg Win Rate</div>
                <div style={{ fontSize: '28px', fontWeight: 700, color: avgWinRate > 0.5 ? '#28a745' : '#ffc107' }}>
                  {safeToFixed(avgWinRate * 100, 1)}%
                </div>
              </div>
              <div style={{ 
                backgroundColor: 'white', 
                padding: '16px', 
                borderRadius: '8px',
                textAlign: 'center',
                boxShadow: '0 2px 4px rgba(0,0,0,0.05)'
              }}>
                <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Total Trades</div>
                <div style={{ fontSize: '28px', fontWeight: 700, color: '#007bff' }}>
                  {totalTrades.toLocaleString()}
                </div>
              </div>
              {(result.portfolio_metrics?.diversification_ratio !== undefined) && (
                <div style={{ 
                  backgroundColor: 'white', 
                  padding: '16px', 
                  borderRadius: '8px',
                  textAlign: 'center',
                  boxShadow: '0 2px 4px rgba(0,0,0,0.05)'
                }}>
                  <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Diversification</div>
                  <div style={{ fontSize: '28px', fontWeight: 700, color: '#6f42c1' }}>
                    {safeToFixed(result.portfolio_metrics.diversification_ratio, 2)}x
                  </div>
                </div>
              )}
            </div>

            {/* Allocation Pie Chart (simplified bar representation) */}
            <div style={{ marginBottom: '10px' }}>
              <div style={{ fontSize: '12px', color: '#666', marginBottom: '8px' }}>Portfolio Allocation</div>
              <div style={{ display: 'flex', borderRadius: '8px', overflow: 'hidden', height: '32px' }}>
                {symbolResults.map((sr: any, idx: number) => {
                  const colors = ['#007bff', '#28a745', '#ffc107', '#dc3545', '#6f42c1', '#17a2b8', '#fd7e14', '#6c757d'];
                  const color = colors[idx % colors.length];
                  const width = (sr.allocation_weight || 0) * 100;
                  return (
                    <div 
                      key={sr.symbol}
                      style={{ 
                        width: `${width}%`, 
                        backgroundColor: color,
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        color: 'white',
                        fontSize: '11px',
                        fontWeight: 500,
                        minWidth: width > 10 ? 'auto' : '0',
                        overflow: 'hidden'
                      }}
                      title={`${sr.symbol}: ${safeToFixed(width, 1)}%`}
                    >
                      {width > 15 ? sr.symbol.replace(/-.*|\/.*/, '') : ''}
                    </div>
                  );
                })}
              </div>
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: '12px', marginTop: '8px' }}>
                {symbolResults.map((sr: any, idx: number) => {
                  const colors = ['#007bff', '#28a745', '#ffc107', '#dc3545', '#6f42c1', '#17a2b8', '#fd7e14', '#6c757d'];
                  const color = colors[idx % colors.length];
                  return (
                    <div key={sr.symbol} style={{ display: 'flex', alignItems: 'center', gap: '6px', fontSize: '12px' }}>
                      <div style={{ width: '12px', height: '12px', borderRadius: '3px', backgroundColor: color }}></div>
                      <span>{sr.symbol.replace(/-.*|\/.*/, '')}: {safeToFixed((sr.allocation_weight || 0) * 100, 1)}%</span>
                    </div>
                  );
                })}
              </div>
            </div>
          </div>

          {/* ==================== INDIVIDUAL ASSET RESULTS ==================== */}
          <div style={{
            backgroundColor: 'white',
            borderRadius: '12px',
            padding: '24px',
            marginBottom: '24px',
            boxShadow: '0 2px 8px rgba(0,0,0,0.08)'
          }}>
            <h3 style={{ margin: '0 0 20px 0', color: '#333', fontSize: '18px' }}>
              📈 Individual Asset Performance
            </h3>
            
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))', gap: '16px' }}>
              {symbolResults.map((sr: any, idx: number) => {
                const colors = ['#007bff', '#28a745', '#ffc107', '#dc3545', '#6f42c1', '#17a2b8', '#fd7e14', '#6c757d'];
                const accentColor = colors[idx % colors.length];
                const isProfitable = sr.total_pnl > 0;
                
                return (
                  <div 
                    key={sr.symbol}
                    style={{ 
                      border: `2px solid ${accentColor}20`,
                      borderRadius: '10px',
                      overflow: 'hidden',
                      backgroundColor: '#fafbfc'
                    }}
                  >
                    {/* Asset Header */}
                    <div style={{ 
                      backgroundColor: accentColor,
                      color: 'white',
                      padding: '12px 16px',
                      display: 'flex',
                      justifyContent: 'space-between',
                      alignItems: 'center'
                    }}>
                      <div>
                        <div style={{ fontSize: '16px', fontWeight: 600 }}>{sr.symbol}</div>
                        <div style={{ fontSize: '12px', opacity: 0.9 }}>{sr.exchange || result.exchange || '-'}</div>
                      </div>
                      <div style={{ 
                        backgroundColor: 'rgba(255,255,255,0.2)',
                        padding: '4px 10px',
                        borderRadius: '12px',
                        fontSize: '12px',
                        fontWeight: 500
                      }}>
                        {safeToFixed((sr.allocation_weight || 0) * 100, 1)}% weight
                      </div>
                    </div>
                    
                    {/* Asset Metrics */}
                    <div style={{ padding: '16px' }}>
                      {/* P&L Highlight */}
                      <div style={{ 
                        textAlign: 'center', 
                        marginBottom: '16px',
                        padding: '12px',
                        backgroundColor: isProfitable ? '#d4edda' : '#f8d7da',
                        borderRadius: '8px'
                      }}>
                        <div style={{ fontSize: '12px', color: '#666' }}>P&L</div>
                        <div style={{ 
                          fontSize: '24px', 
                          fontWeight: 700, 
                          color: isProfitable ? '#28a745' : '#dc3545' 
                        }}>
                          {isProfitable ? '+' : ''}${formatLargeNumber(sr.total_pnl)}
                        </div>
                      </div>
                      
                      {/* Metrics Grid */}
                      <div style={{ 
                        display: 'grid', 
                        gridTemplateColumns: '1fr 1fr', 
                        gap: '12px',
                        fontSize: '13px'
                      }}>
                        <div style={{ 
                          padding: '10px', 
                          backgroundColor: 'white', 
                          borderRadius: '6px',
                          textAlign: 'center'
                        }}>
                          <div style={{ color: '#666', marginBottom: '4px' }}>Sharpe</div>
                          <div style={{ 
                            fontWeight: 600, 
                            color: sr.sharpe_ratio > 1.5 ? '#28a745' : sr.sharpe_ratio > 0 ? '#ffc107' : '#dc3545'
                          }}>
                            {safeToFixed(sr.sharpe_ratio, 2)}
                          </div>
                        </div>
                        <div style={{ 
                          padding: '10px', 
                          backgroundColor: 'white', 
                          borderRadius: '6px',
                          textAlign: 'center'
                        }}>
                          <div style={{ color: '#666', marginBottom: '4px' }}>Max DD</div>
                          <div style={{ 
                            fontWeight: 600, 
                            color: sr.max_drawdown > 0.2 ? '#dc3545' : '#28a745'
                          }}>
                            {safeToFixed((sr.max_drawdown || 0) * 100, 1)}%
                          </div>
                        </div>
                        <div style={{ 
                          padding: '10px', 
                          backgroundColor: 'white', 
                          borderRadius: '6px',
                          textAlign: 'center'
                        }}>
                          <div style={{ color: '#666', marginBottom: '4px' }}>Win Rate</div>
                          <div style={{ 
                            fontWeight: 600, 
                            color: sr.win_rate > 0.5 ? '#28a745' : '#ffc107'
                          }}>
                            {safeToFixed((sr.win_rate || 0) * 100, 1)}%
                          </div>
                        </div>
                        <div style={{ 
                          padding: '10px', 
                          backgroundColor: 'white', 
                          borderRadius: '6px',
                          textAlign: 'center'
                        }}>
                          <div style={{ color: '#666', marginBottom: '4px' }}>Profit Factor</div>
                          <div style={{ 
                            fontWeight: 600, 
                            color: sr.profit_factor > 1.5 ? '#28a745' : sr.profit_factor > 1 ? '#ffc107' : '#dc3545'
                          }}>
                            {safeToFixed(sr.profit_factor || 0, 2)}
                          </div>
                        </div>
                      </div>
                      
                      {/* Trades */}
                      <div style={{ 
                        marginTop: '12px', 
                        textAlign: 'center', 
                        color: '#666',
                        fontSize: '13px'
                      }}>
                        <strong>{sr.total_trades?.toLocaleString() || 0}</strong> trades executed
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          </div>

          {/* ==================== PER-SYMBOL POSITIONS ==================== */}
          {symbolResults.some((sr: SymbolResult) => sr.starting_capital !== undefined || sr.ending_capital !== undefined) && (
            <div style={{
              backgroundColor: 'white',
              borderRadius: '12px',
              padding: '24px',
              marginBottom: '24px',
              boxShadow: '0 2px 8px rgba(0,0,0,0.08)'
            }}>
              <h3 style={{ margin: '0 0 20px 0', color: '#333', fontSize: '18px' }}>
                💰 Per-Symbol Position Tracking
              </h3>
              
              <div style={{ overflowX: 'auto' }}>
                <table style={{ 
                  width: '100%', 
                  borderCollapse: 'collapse',
                  fontSize: '13px'
                }}>
                  <thead>
                    <tr style={{ backgroundColor: '#f8f9fa' }}>
                      <th style={{ padding: '12px 8px', textAlign: 'left', borderBottom: '2px solid #dee2e6', fontWeight: 600 }}>Symbol</th>
                      <th style={{ padding: '12px 8px', textAlign: 'right', borderBottom: '2px solid #dee2e6', fontWeight: 600 }}>Start Cash</th>
                      <th style={{ padding: '12px 8px', textAlign: 'right', borderBottom: '2px solid #dee2e6', fontWeight: 600 }}>Start Inv</th>
                      <th style={{ padding: '12px 8px', textAlign: 'right', borderBottom: '2px solid #dee2e6', fontWeight: 600 }}>Start Inv Value</th>
                      <th style={{ padding: '12px 8px', textAlign: 'center', borderBottom: '2px solid #dee2e6', fontWeight: 600 }}>→</th>
                      <th style={{ padding: '12px 8px', textAlign: 'right', borderBottom: '2px solid #dee2e6', fontWeight: 600 }}>End Cash</th>
                      <th style={{ padding: '12px 8px', textAlign: 'right', borderBottom: '2px solid #dee2e6', fontWeight: 600 }}>End Inv</th>
                      <th style={{ padding: '12px 8px', textAlign: 'right', borderBottom: '2px solid #dee2e6', fontWeight: 600 }}>End Inv Value</th>
                      <th style={{ padding: '12px 8px', textAlign: 'right', borderBottom: '2px solid #dee2e6', fontWeight: 600 }}>P&L</th>
                    </tr>
                  </thead>
                  <tbody>
                    {symbolResults.map((sr: SymbolResult, idx: number) => {
                      const baseAsset = sr.symbol?.split('/')[0] ?? 'Asset';
                      const isProfitable = (sr.total_pnl ?? 0) >= 0;
                      return (
                        <tr key={sr.symbol} style={{ backgroundColor: idx % 2 === 0 ? 'white' : '#f8f9fa' }}>
                          <td style={{ padding: '10px 8px', borderBottom: '1px solid #dee2e6', fontWeight: 600 }}>
                            {sr.symbol}
                            {sr.exchange && <span style={{ color: '#6c757d', fontSize: '11px', marginLeft: '4px' }}>@{sr.exchange}</span>}
                          </td>
                          <td style={{ padding: '10px 8px', textAlign: 'right', borderBottom: '1px solid #dee2e6' }}>
                            ${formatCurrency(sr.starting_capital ?? 0)}
                          </td>
                          <td style={{ padding: '10px 8px', textAlign: 'right', borderBottom: '1px solid #dee2e6' }}>
                            {safeToFixed(sr.starting_inventory ?? 0, 6)} {baseAsset}
                          </td>
                          <td style={{ padding: '10px 8px', textAlign: 'right', borderBottom: '1px solid #dee2e6' }}>
                            ${formatCurrency(sr.starting_inventory_value ?? 0)}
                          </td>
                          <td style={{ padding: '10px 8px', textAlign: 'center', borderBottom: '1px solid #dee2e6', color: '#6c757d' }}>
                            →
                          </td>
                          <td style={{ padding: '10px 8px', textAlign: 'right', borderBottom: '1px solid #dee2e6' }}>
                            ${formatCurrency(sr.ending_capital ?? 0)}
                          </td>
                          <td style={{ padding: '10px 8px', textAlign: 'right', borderBottom: '1px solid #dee2e6' }}>
                            {safeToFixed(sr.ending_inventory ?? 0, 6)} {baseAsset}
                          </td>
                          <td style={{ padding: '10px 8px', textAlign: 'right', borderBottom: '1px solid #dee2e6' }}>
                            ${formatCurrency(sr.ending_inventory_value ?? 0)}
                          </td>
                          <td style={{ 
                            padding: '10px 8px', 
                            textAlign: 'right', 
                            borderBottom: '1px solid #dee2e6',
                            fontWeight: 600,
                            color: isProfitable ? '#28a745' : '#dc3545'
                          }}>
                            {isProfitable ? '+' : ''}${formatCurrency(sr.total_pnl ?? 0)}
                          </td>
                        </tr>
                      );
                    })}
                    {/* Totals Row */}
                    <tr style={{ backgroundColor: '#e9ecef', fontWeight: 600 }}>
                      <td style={{ padding: '12px 8px', borderTop: '2px solid #dee2e6' }}>TOTAL</td>
                      <td style={{ padding: '12px 8px', textAlign: 'right', borderTop: '2px solid #dee2e6' }}>
                        ${formatCurrency(symbolResults.reduce((sum: number, sr: SymbolResult) => sum + (sr.starting_capital ?? 0), 0))}
                      </td>
                      <td style={{ padding: '12px 8px', textAlign: 'right', borderTop: '2px solid #dee2e6', color: '#6c757d' }}>—</td>
                      <td style={{ padding: '12px 8px', textAlign: 'right', borderTop: '2px solid #dee2e6' }}>
                        ${formatCurrency(symbolResults.reduce((sum: number, sr: SymbolResult) => sum + (sr.starting_inventory_value ?? 0), 0))}
                      </td>
                      <td style={{ padding: '12px 8px', textAlign: 'center', borderTop: '2px solid #dee2e6' }}>→</td>
                      <td style={{ padding: '12px 8px', textAlign: 'right', borderTop: '2px solid #dee2e6' }}>
                        ${formatCurrency(symbolResults.reduce((sum: number, sr: SymbolResult) => sum + (sr.ending_capital ?? 0), 0))}
                      </td>
                      <td style={{ padding: '12px 8px', textAlign: 'right', borderTop: '2px solid #dee2e6', color: '#6c757d' }}>—</td>
                      <td style={{ padding: '12px 8px', textAlign: 'right', borderTop: '2px solid #dee2e6' }}>
                        ${formatCurrency(symbolResults.reduce((sum: number, sr: SymbolResult) => sum + (sr.ending_inventory_value ?? 0), 0))}
                      </td>
                      <td style={{ 
                        padding: '12px 8px', 
                        textAlign: 'right', 
                        borderTop: '2px solid #dee2e6',
                        color: totalPnL >= 0 ? '#28a745' : '#dc3545'
                      }}>
                        {totalPnL >= 0 ? '+' : ''}${formatCurrency(totalPnL)}
                      </td>
                    </tr>
                  </tbody>
                </table>
              </div>
            </div>
          )}

          {/* ==================== CORRELATION MATRIX ==================== */}
          {correlationMatrix.length > 0 && (
            <div style={{
              backgroundColor: 'white',
              borderRadius: '12px',
              padding: '24px',
              marginBottom: '24px',
              boxShadow: '0 2px 8px rgba(0,0,0,0.08)'
            }}>
              <h3 style={{ margin: '0 0 20px 0', color: '#333', fontSize: '18px' }}>
                🔗 Asset Correlations
              </h3>
              
              <div style={{ 
                display: 'grid',
                gridTemplateColumns: `repeat(${totalSymbols + 1}, 1fr)`,
                gap: '4px',
                maxWidth: '600px',
                marginBottom: '20px'
              }}>
                {/* Build unique symbol list from correlation pairs */}
                {(() => {
                  const symbolsSet = new Set<string>();
                  correlationMatrix.forEach((pair: any) => {
                    symbolsSet.add(pair.symbol_a);
                    symbolsSet.add(pair.symbol_b);
                  });
                  const uniqueSymbols = Array.from(symbolsSet) as string[];
                  
                  // Create matrix map for O(1) lookup
                  const corrMap = new Map<string, number>();
                  correlationMatrix.forEach((pair: any) => {
                    corrMap.set(`${pair.symbol_a}-${pair.symbol_b}`, pair.correlation);
                    corrMap.set(`${pair.symbol_b}-${pair.symbol_a}`, pair.correlation);
                  });
                  
                  // Render header row
                  const headerRow = (
                    <React.Fragment key="header">
                      <div style={{ padding: '8px', fontSize: '12px', textAlign: 'center', fontWeight: 600 }}></div>
                      {uniqueSymbols.map(s => (
                        <div key={s} style={{ 
                          padding: '8px', 
                          fontSize: '11px', 
                          textAlign: 'center', 
                          fontWeight: 600,
                          backgroundColor: '#f8f9fa',
                          borderRadius: '4px'
                        }}>
                          {s.replace(/-.*|\/.*/, '')}
                        </div>
                      ))}
                    </React.Fragment>
                  );
                  
                  // Render data rows
                  const rows = uniqueSymbols.map(rowSym => (
                    <React.Fragment key={rowSym}>
                      <div style={{ 
                        padding: '8px', 
                        fontSize: '11px', 
                        fontWeight: 600,
                        backgroundColor: '#f8f9fa',
                        borderRadius: '4px',
                        display: 'flex',
                        alignItems: 'center'
                      }}>
                        {rowSym.replace(/-.*|\/.*/, '')}
                      </div>
                      {uniqueSymbols.map(colSym => {
                        const corr = rowSym === colSym ? 1.0 : (corrMap.get(`${rowSym}-${colSym}`) ?? 0);
                        const bgColor = rowSym === colSym 
                          ? '#e9ecef' 
                          : corr > 0.7 
                            ? `rgba(220, 53, 69, ${0.2 + Math.abs(corr) * 0.6})` 
                            : corr < -0.3 
                              ? `rgba(40, 167, 69, ${0.2 + Math.abs(corr) * 0.6})` 
                              : `rgba(0, 123, 255, ${0.1 + Math.abs(corr) * 0.3})`;
                        const textColor = rowSym === colSym ? '#666' : 
                          (corr > 0.7 || corr < -0.3) ? 'white' : '#333';
                        return (
                          <div 
                            key={colSym} 
                            style={{ 
                              padding: '8px', 
                              fontSize: '12px', 
                              textAlign: 'center',
                              backgroundColor: bgColor,
                              borderRadius: '4px',
                              fontWeight: 500,
                              color: textColor
                            }}
                            title={`${rowSym.replace(/-.*|\/.*/, '')} ↔ ${colSym.replace(/-.*|\/.*/, '')}: ${safeToFixed(corr, 3)}`}
                          >
                            {safeToFixed(corr, 2)}
                          </div>
                        );
                      })}
                    </React.Fragment>
                  ));
                  
                  return [headerRow, ...rows];
                })()}
              </div>
              
              {/* Legend */}
              <div style={{ 
                display: 'flex', 
                gap: '20px', 
                fontSize: '12px', 
                color: '#666',
                flexWrap: 'wrap'
              }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                  <div style={{ width: '16px', height: '16px', borderRadius: '4px', backgroundColor: 'rgba(220, 53, 69, 0.7)' }}></div>
                  <span>High correlation (&gt;0.7) - less diversification</span>
                </div>
                <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                  <div style={{ width: '16px', height: '16px', borderRadius: '4px', backgroundColor: 'rgba(40, 167, 69, 0.7)' }}></div>
                  <span>Negative correlation (&lt;-0.3) - hedging benefit</span>
                </div>
                <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                  <div style={{ width: '16px', height: '16px', borderRadius: '4px', backgroundColor: 'rgba(0, 123, 255, 0.3)' }}></div>
                  <span>Low correlation - diversification benefit</span>
                </div>
              </div>
            </div>
          )}

          {/* ==================== PERFORMANCE COMPARISON TABLE ==================== */}
          <div style={{
            backgroundColor: 'white',
            borderRadius: '12px',
            padding: '24px',
            marginBottom: '24px',
            boxShadow: '0 2px 8px rgba(0,0,0,0.08)'
          }}>
            <h3 style={{ margin: '0 0 20px 0', color: '#333', fontSize: '18px' }}>
              📋 Performance Comparison
            </h3>
            
            <div style={{ overflowX: 'auto' }}>
              <table style={{ 
                width: '100%', 
                borderCollapse: 'collapse', 
                fontSize: '14px'
              }}>
                <thead>
                  <tr style={{ backgroundColor: '#f8f9fa' }}>
                    <th style={{ padding: '12px', textAlign: 'left', borderBottom: '2px solid #dee2e6', fontWeight: 600 }}>Asset</th>
                    <th style={{ padding: '12px', textAlign: 'left', borderBottom: '2px solid #dee2e6', fontWeight: 600 }}>Exchange</th>
                    <th style={{ padding: '12px', textAlign: 'right', borderBottom: '2px solid #dee2e6', fontWeight: 600 }}>Weight</th>
                    <th style={{ padding: '12px', textAlign: 'right', borderBottom: '2px solid #dee2e6', fontWeight: 600 }}>Sharpe</th>
                    <th style={{ padding: '12px', textAlign: 'right', borderBottom: '2px solid #dee2e6', fontWeight: 600 }}>Max DD</th>
                    <th style={{ padding: '12px', textAlign: 'right', borderBottom: '2px solid #dee2e6', fontWeight: 600 }}>P&L</th>
                    <th style={{ padding: '12px', textAlign: 'right', borderBottom: '2px solid #dee2e6', fontWeight: 600 }}>Win Rate</th>
                    <th style={{ padding: '12px', textAlign: 'right', borderBottom: '2px solid #dee2e6', fontWeight: 600 }}>PF</th>
                    <th style={{ padding: '12px', textAlign: 'right', borderBottom: '2px solid #dee2e6', fontWeight: 600 }}>Trades</th>
                  </tr>
                </thead>
                <tbody>
                  {symbolResults.map((sr: any, idx: number) => (
                    <tr key={sr.symbol} style={{ 
                      backgroundColor: idx % 2 === 0 ? '#fff' : '#f8f9fa',
                      borderBottom: '1px solid #e9ecef'
                    }}>
                      <td style={{ padding: '12px', fontWeight: 600 }}>{sr.symbol}</td>
                      <td style={{ padding: '12px', color: '#666' }}>{sr.exchange || result.exchange || '-'}</td>
                      <td style={{ padding: '12px', textAlign: 'right' }}>
                        <span style={{ 
                          backgroundColor: '#e7f1ff', 
                          padding: '2px 8px', 
                          borderRadius: '12px',
                          fontSize: '12px'
                        }}>
                          {safeToFixed((sr.allocation_weight || 0) * 100, 1)}%
                        </span>
                      </td>
                      <td style={{ 
                        padding: '12px', 
                        textAlign: 'right',
                        fontWeight: 600,
                        color: sr.sharpe_ratio > 1.5 ? '#28a745' : sr.sharpe_ratio > 0 ? '#856404' : '#dc3545'
                      }}>
                        {safeToFixed(sr.sharpe_ratio, 2)}
                      </td>
                      <td style={{ 
                        padding: '12px', 
                        textAlign: 'right',
                        color: sr.max_drawdown > 0.2 ? '#dc3545' : '#28a745'
                      }}>
                        {safeToFixed((sr.max_drawdown || 0) * 100, 1)}%
                      </td>
                      <td style={{ 
                        padding: '12px', 
                        textAlign: 'right',
                        fontWeight: 600,
                        color: sr.total_pnl > 0 ? '#28a745' : '#dc3545'
                      }}>
                        {sr.total_pnl > 0 ? '+' : ''}${formatLargeNumber(sr.total_pnl)}
                      </td>
                      <td style={{ padding: '12px', textAlign: 'right' }}>
                        {safeToFixed((sr.win_rate || 0) * 100, 1)}%
                      </td>
                      <td style={{ 
                        padding: '12px', 
                        textAlign: 'right',
                        color: sr.profit_factor > 1.5 ? '#28a745' : sr.profit_factor > 1 ? '#856404' : '#dc3545'
                      }}>
                        {safeToFixed(sr.profit_factor || 0, 2)}
                      </td>
                      <td style={{ padding: '12px', textAlign: 'right' }}>{(sr.total_trades || 0).toLocaleString()}</td>
                    </tr>
                  ))}
                  {/* Totals Row */}
                  <tr style={{ 
                    backgroundColor: '#e7f1ff',
                    fontWeight: 600,
                    borderTop: '2px solid #0066cc'
                  }}>
                    <td style={{ padding: '12px' }}>PORTFOLIO</td>
                    <td style={{ padding: '12px', color: '#666' }}>Combined</td>
                    <td style={{ padding: '12px', textAlign: 'right' }}>100%</td>
                    <td style={{ 
                      padding: '12px', 
                      textAlign: 'right',
                      color: portfolioSharpe > 1.5 ? '#28a745' : '#0066cc'
                    }}>
                      {safeToFixed(portfolioSharpe, 2)}
                    </td>
                    <td style={{ 
                      padding: '12px', 
                      textAlign: 'right',
                      color: portfolioMaxDD > 0.2 ? '#dc3545' : '#28a745'
                    }}>
                      {safeToFixed(portfolioMaxDD * 100, 1)}%
                    </td>
                    <td style={{ 
                      padding: '12px', 
                      textAlign: 'right',
                      color: totalPnL > 0 ? '#28a745' : '#dc3545'
                    }}>
                      {totalPnL > 0 ? '+' : ''}${formatLargeNumber(totalPnL)}
                    </td>
                    <td style={{ padding: '12px', textAlign: 'right' }}>
                      {safeToFixed(avgWinRate * 100, 1)}%
                    </td>
                    <td style={{ padding: '12px', textAlign: 'right' }}>-</td>
                    <td style={{ padding: '12px', textAlign: 'right' }}>{totalTrades.toLocaleString()}</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>
        </>
        );
      })()}

      {/* Smart Order Routing Analysis */}
      {result.routing_metrics && (
        <div style={{
          backgroundColor: 'white',
          padding: '20px',
          borderRadius: '8px',
          boxShadow: '0 2px 4px rgba(0,0,0,0.1)',
          marginBottom: '20px',
        }}>
          <h3 style={{ marginTop: 0, marginBottom: '15px' }}>
            🔀 Smart Order Routing Analysis
          </h3>
          
          {/* Routing Summary */}
          <div style={{
            backgroundColor: '#f5f0ff',
            border: '1px solid #d0c4e8',
            borderRadius: '8px',
            padding: '15px',
            marginBottom: '20px',
          }}>
            <div style={{ display: 'flex', justifyContent: 'space-around', flexWrap: 'wrap', gap: '15px' }}>
              <div style={{ textAlign: 'center' }}>
                <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Orders Routed</div>
                <div style={{ fontSize: '20px', fontWeight: 600, color: '#0066cc' }}>
                  {result.routing_metrics.total_orders_routed.toLocaleString()}
                </div>
              </div>
              <div style={{ textAlign: 'center' }}>
                <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Volume Routed</div>
                <div style={{ fontSize: '20px', fontWeight: 600, color: '#0066cc' }}>
                  ${result.routing_metrics.total_volume_routed.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                </div>
              </div>
              <div style={{ textAlign: 'center' }}>
                <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Avg Venues/Order</div>
                <div style={{ fontSize: '20px', fontWeight: 600, color: '#17a2b8' }}>
                  {safeToFixed(result.routing_metrics.avg_venues_per_order, 1)}
                </div>
              </div>
              <div style={{ textAlign: 'center' }}>
                <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Avg Slippage</div>
                <div style={{ 
                  fontSize: '20px', 
                  fontWeight: 600, 
                  color: result.routing_metrics.avg_slippage_bps <= 2 ? '#28a745' : result.routing_metrics.avg_slippage_bps <= 5 ? '#ffc107' : '#dc3545' 
                }}>
                  {safeToFixed(result.routing_metrics.avg_slippage_bps, 2)} bps
                </div>
              </div>
              <div style={{ textAlign: 'center' }}>
                <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Routing Savings</div>
                <div style={{ 
                  fontSize: '20px', 
                  fontWeight: 600, 
                  color: result.routing_metrics.routing_savings >= 0 ? '#28a745' : '#dc3545' 
                }}>
                  ${safeToFixed(result.routing_metrics.routing_savings, 2)}
                </div>
              </div>
              <div style={{ textAlign: 'center' }}>
                <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Total Fees</div>
                <div style={{ fontSize: '20px', fontWeight: 600, color: '#dc3545' }}>
                  ${safeToFixed(result.routing_metrics.total_fees_paid, 2)}
                </div>
              </div>
            </div>
          </div>

          {/* Execution Quality Breakdown */}
          <div style={{ marginBottom: '20px' }}>
            <h4 style={{ marginBottom: '10px', color: '#333' }}>📊 Execution Quality</h4>
            <div style={{ display: 'flex', gap: '10px', flexWrap: 'wrap' }}>
              <div style={{ flex: 1, minWidth: '120px', padding: '10px', backgroundColor: '#d4edda', borderRadius: '6px', textAlign: 'center' }}>
                <div style={{ fontSize: '24px', fontWeight: 600, color: '#155724' }}>{safeToFixed(result.routing_metrics.quality_excellent_pct, 1)}%</div>
                <div style={{ fontSize: '12px', color: '#155724' }}>Excellent</div>
              </div>
              <div style={{ flex: 1, minWidth: '120px', padding: '10px', backgroundColor: '#cce5ff', borderRadius: '6px', textAlign: 'center' }}>
                <div style={{ fontSize: '24px', fontWeight: 600, color: '#004085' }}>{safeToFixed(result.routing_metrics.quality_good_pct, 1)}%</div>
                <div style={{ fontSize: '12px', color: '#004085' }}>Good</div>
              </div>
              <div style={{ flex: 1, minWidth: '120px', padding: '10px', backgroundColor: '#fff3cd', borderRadius: '6px', textAlign: 'center' }}>
                <div style={{ fontSize: '24px', fontWeight: 600, color: '#856404' }}>{safeToFixed(result.routing_metrics.quality_average_pct, 1)}%</div>
                <div style={{ fontSize: '12px', color: '#856404' }}>Average</div>
              </div>
              <div style={{ flex: 1, minWidth: '120px', padding: '10px', backgroundColor: '#f8d7da', borderRadius: '6px', textAlign: 'center' }}>
                <div style={{ fontSize: '24px', fontWeight: 600, color: '#721c24' }}>{safeToFixed(result.routing_metrics.quality_poor_pct, 1)}%</div>
                <div style={{ fontSize: '12px', color: '#721c24' }}>Poor</div>
              </div>
            </div>
          </div>

          {/* Venue Breakdown Table */}
          {result.routing_metrics.venue_breakdown && result.routing_metrics.venue_breakdown.length > 0 && (
            <>
              <h4 style={{ marginBottom: '10px', color: '#333' }}>📍 Venue Breakdown</h4>
              <div style={{ overflowX: 'auto', marginBottom: '20px' }}>
                <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: '14px' }}>
                  <thead>
                    <tr style={{ backgroundColor: '#f8f9fa' }}>
                      <th style={{ padding: '10px', textAlign: 'left', borderBottom: '2px solid #dee2e6' }}>Venue</th>
                      <th style={{ padding: '10px', textAlign: 'right', borderBottom: '2px solid #dee2e6' }}>Orders</th>
                      <th style={{ padding: '10px', textAlign: 'right', borderBottom: '2px solid #dee2e6' }}>Volume</th>
                      <th style={{ padding: '10px', textAlign: 'right', borderBottom: '2px solid #dee2e6' }}>Avg Fill Price</th>
                      <th style={{ padding: '10px', textAlign: 'right', borderBottom: '2px solid #dee2e6' }}>Fill Rate</th>
                      <th style={{ padding: '10px', textAlign: 'right', borderBottom: '2px solid #dee2e6' }}>Slippage</th>
                      <th style={{ padding: '10px', textAlign: 'right', borderBottom: '2px solid #dee2e6' }}>Fees</th>
                    </tr>
                  </thead>
                  <tbody>
                    {result.routing_metrics.venue_breakdown.map((venue, idx) => (
                      <tr key={idx} style={{ borderBottom: '1px solid #dee2e6' }}>
                        <td style={{ padding: '10px', fontWeight: 500, textTransform: 'capitalize' }}>
                          {venue.venue}
                        </td>
                        <td style={{ padding: '10px', textAlign: 'right' }}>
                          {venue.orders_routed.toLocaleString()}
                        </td>
                        <td style={{ padding: '10px', textAlign: 'right' }}>
                          ${venue.volume_routed.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                        </td>
                        <td style={{ padding: '10px', textAlign: 'right' }}>
                          ${safeToFixed(venue.avg_fill_price, 4)}
                        </td>
                        <td style={{ 
                          padding: '10px', 
                          textAlign: 'right',
                          color: venue.fill_rate >= 0.9 ? '#28a745' : venue.fill_rate >= 0.7 ? '#ffc107' : '#dc3545'
                        }}>
                          {safeToFixed(venue.fill_rate * 100, 1)}%
                        </td>
                        <td style={{ 
                          padding: '10px', 
                          textAlign: 'right',
                          color: venue.avg_slippage_bps <= 2 ? '#28a745' : venue.avg_slippage_bps <= 5 ? '#ffc107' : '#dc3545'
                        }}>
                          {safeToFixed(venue.avg_slippage_bps, 2)} bps
                        </td>
                        <td style={{ padding: '10px', textAlign: 'right' }}>
                          ${venue.total_fees.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </>
          )}

          {/* Arbitrage Performance (if arbitrage metrics exist) */}
          {result.routing_metrics.arbitrage_metrics && (
            <>
              <h4 style={{ marginBottom: '10px', color: '#333' }}>💹 Arbitrage Performance</h4>
              <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))',
                gap: '15px',
                backgroundColor: '#f0fff4',
                border: '1px solid #c3e6cb',
                borderRadius: '8px',
                padding: '15px',
                marginBottom: '20px',
              }}>
                <div style={{ textAlign: 'center' }}>
                  <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Opportunities Detected</div>
                  <div style={{ fontSize: '20px', fontWeight: 600, color: '#17a2b8' }}>
                    {result.routing_metrics.arbitrage_metrics.opportunities_detected.toLocaleString()}
                  </div>
                </div>
                <div style={{ textAlign: 'center' }}>
                  <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Opportunities Executed</div>
                  <div style={{ fontSize: '20px', fontWeight: 600, color: '#28a745' }}>
                    {result.routing_metrics.arbitrage_metrics.opportunities_executed.toLocaleString()}
                  </div>
                </div>
                <div style={{ textAlign: 'center' }}>
                  <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Win Rate</div>
                  <div style={{ fontSize: '20px', fontWeight: 600, color: '#6f42c1' }}>
                    {safeToFixed(result.routing_metrics.arbitrage_metrics.win_rate * 100, 1)}%
                  </div>
                </div>
                <div style={{ textAlign: 'center' }}>
                  <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Total Arb PnL</div>
                  <div style={{ 
                    fontSize: '20px', 
                    fontWeight: 600, 
                    color: result.routing_metrics.arbitrage_metrics.total_arb_pnl >= 0 ? '#28a745' : '#dc3545' 
                  }}>
                    ${result.routing_metrics.arbitrage_metrics.total_arb_pnl.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                  </div>
                </div>
                <div style={{ textAlign: 'center' }}>
                  <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Avg Profit</div>
                  <div style={{ fontSize: '20px', fontWeight: 600, color: '#28a745' }}>
                    {safeToFixed(result.routing_metrics.arbitrage_metrics.avg_profit_bps, 1)} bps
                  </div>
                </div>
                <div style={{ textAlign: 'center' }}>
                  <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Avg Latency</div>
                  <div style={{ fontSize: '20px', fontWeight: 600, color: '#6c757d' }}>
                    {safeToFixed(result.routing_metrics.arbitrage_metrics.avg_execution_latency_ms, 1)} ms
                  </div>
                </div>
              </div>
            </>
          )}
        </div>
      )}

      {/* Monte Carlo Analysis Results */}
      {result.monte_carlo && (
        <div style={{
          backgroundColor: 'white',
          padding: '20px',
          borderRadius: '8px',
          boxShadow: '0 2px 4px rgba(0,0,0,0.1)',
          marginBottom: '20px',
        }}>
          <h3 style={{ marginTop: 0, marginBottom: '15px' }}>🎲 Monte Carlo Analysis ({result.monte_carlo.num_runs} runs)</h3>
          
          {/* Key Risk Summary Box */}
          <div style={{
            backgroundColor: '#f8f9fa',
            border: '1px solid #dee2e6',
            borderRadius: '8px',
            padding: '15px',
            marginBottom: '20px',
          }}>
            <div style={{ display: 'flex', justifyContent: 'space-around', flexWrap: 'wrap', gap: '15px' }}>
              <div style={{ textAlign: 'center' }}>
                <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Value at Risk (95%)</div>
                <div style={{ fontSize: '20px', fontWeight: 600, color: '#dc3545' }}>
                  -${safeToFixed(Math.abs(result.monte_carlo.var_95), 2)}
                </div>
                <div style={{ fontSize: '11px', color: '#888' }}>
                  {safeToFixed((Math.abs(result.monte_carlo.var_95) / (jobDetails?.initial_capital || 10000)) * 100, 2)}% of capital
                </div>
              </div>
              <div style={{ textAlign: 'center' }}>
                <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Expected Shortfall (CVaR)</div>
                <div style={{ fontSize: '20px', fontWeight: 600, color: '#dc3545' }}>
                  -${safeToFixed(Math.abs(result.monte_carlo.cvar_95), 2)}
                </div>
                <div style={{ fontSize: '11px', color: '#888' }}>
                  {safeToFixed((Math.abs(result.monte_carlo.cvar_95) / (jobDetails?.initial_capital || 10000)) * 100, 2)}% of capital
                </div>
              </div>
              <div style={{ textAlign: 'center' }}>
                <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>Sharpe 95% Confidence</div>
                <div style={{ fontSize: '20px', fontWeight: 600, color: result.monte_carlo.confidence_lower > 0 ? '#28a745' : '#ffc107' }}>
                  [{safeToFixed(result.monte_carlo.confidence_lower, 2)}, {safeToFixed(result.monte_carlo.confidence_upper, 2)}]
                </div>
                <div style={{ fontSize: '11px', color: '#888' }}>
                  Mean: {safeToFixed(result.monte_carlo.mean_sharpe, 3)} ± {safeToFixed(result.monte_carlo.stddev_sharpe, 3)}
                </div>
              </div>
            </div>
          </div>
          
          {/* Risk Metrics */}
          <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>Risk Metrics</h4>
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
            gap: '15px',
            marginBottom: '20px',
          }}>
            <MetricCard
              title="Mean Sharpe"
              value={safeToFixed(result.monte_carlo.mean_sharpe, 3)}
              color={result.monte_carlo.mean_sharpe > 1 ? '#28a745' : result.monte_carlo.mean_sharpe > 0 ? '#ffc107' : '#dc3545'}
              tooltip="Average Sharpe Ratio across all Monte Carlo simulations. Represents expected risk-adjusted return accounting for randomness."
            />
            <MetricCard
              title="Sharpe Std Dev"
              value={safeToFixed(result.monte_carlo.stddev_sharpe, 3)}
              color={result.monte_carlo.stddev_sharpe < 0.5 ? '#28a745' : '#ffc107'}
              tooltip="Standard deviation of Sharpe across simulations. Lower values indicate more consistent performance. High values suggest luck-dependent results."
            />
            <MetricCard
              title="VaR (95%)"
              value={`$${safeToFixed(result.monte_carlo.var_95, 2)} (${safeToFixed((result.monte_carlo.var_95 / (jobDetails?.initial_capital || 10000)) * 100, 2)}%)`}
              color={(result.monte_carlo.var_95 / (jobDetails?.initial_capital || 10000)) < 0.05 ? '#28a745' : (result.monte_carlo.var_95 / (jobDetails?.initial_capital || 10000)) < 0.10 ? '#ffc107' : '#dc3545'}
              tooltip="Value at Risk at 95% confidence. The maximum expected loss under normal market conditions. 5% of outcomes will be worse than this."
            />
            <MetricCard
              title="CVaR (95%)"
              value={`$${safeToFixed(result.monte_carlo.cvar_95, 2)} (${safeToFixed((result.monte_carlo.cvar_95 / (jobDetails?.initial_capital || 10000)) * 100, 2)}%)`}
              color={(result.monte_carlo.cvar_95 / (jobDetails?.initial_capital || 10000)) < 0.08 ? '#28a745' : (result.monte_carlo.cvar_95 / (jobDetails?.initial_capital || 10000)) < 0.15 ? '#ffc107' : '#dc3545'}
              tooltip="Expected Shortfall (Conditional VaR). Average loss when VaR is exceeded. Better captures tail risk than VaR alone."
            />
            <MetricCard
              title="Sharpe 95% CI"
              value={`[${safeToFixed(result.monte_carlo.confidence_lower, 2)}, ${safeToFixed(result.monte_carlo.confidence_upper, 2)}]`}
              color="#007bff"
              tooltip="95% confidence interval for Sharpe Ratio. If lower bound > 0, strategy is likely profitable. Narrower range = more reliable estimate."
            />
          </div>
          
          {/* Percentile Distributions - PnL */}
          {(result.monte_carlo.p5_total_pnl !== undefined || result.monte_carlo.p50_total_pnl !== undefined) && (
            <>
              <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>📊 PnL Distribution (P5 / P50 / P95)</h4>
              <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
                gap: '15px',
                marginBottom: '20px',
              }}>
                <MetricCard
                  title="Worst Case (P5)"
                  value={`$${safeToFixed(result.monte_carlo.p5_total_pnl ?? 0, 0)}`}
                  color={(result.monte_carlo.p5_total_pnl ?? 0) > 0 ? '#28a745' : '#dc3545'}
                  tooltip="5th percentile P&L. Only 5% of simulations performed worse than this. Your realistic downside scenario."
                />
                <MetricCard
                  title="Typical (P50)"
                  value={`$${safeToFixed(result.monte_carlo.p50_total_pnl ?? 0, 0)}`}
                  color={(result.monte_carlo.p50_total_pnl ?? 0) > 0 ? '#28a745' : '#dc3545'}
                  tooltip="Median P&L across all simulations. The most likely outcome - half of simulations did better, half did worse."
                />
                <MetricCard
                  title="Best Case (P95)"
                  value={`$${safeToFixed(result.monte_carlo.p95_total_pnl ?? 0, 0)}`}
                  color={(result.monte_carlo.p95_total_pnl ?? 0) > 0 ? '#28a745' : '#ffc107'}
                  tooltip="95th percentile P&L. Only 5% of simulations did better. Your optimistic upside scenario."
                />
              </div>
            </>
          )}
          
          {/* Percentile Distributions - Drawdown */}
          {(result.monte_carlo.p5_max_drawdown !== undefined || result.monte_carlo.p50_max_drawdown !== undefined) && (
            <>
              <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>📉 Max Drawdown Distribution (P5 / P50 / P95)</h4>
              <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
                gap: '15px',
                marginBottom: '20px',
              }}>
                <MetricCard
                  title="Best Case (P5)"
                  value={`${safeToFixed((result.monte_carlo.p5_max_drawdown ?? 0) * 100, 1)}%`}
                  color={(result.monte_carlo.p5_max_drawdown ?? 0) > -0.1 ? '#28a745' : '#ffc107'}
                  tooltip="5th percentile drawdown. The smallest drawdown experienced in 95% of simulations. Best-case risk scenario."
                />
                <MetricCard
                  title="Typical (P50)"
                  value={`${safeToFixed((result.monte_carlo.p50_max_drawdown ?? 0) * 100, 1)}%`}
                  color={(result.monte_carlo.p50_max_drawdown ?? 0) > -0.2 ? '#28a745' : '#ffc107'}
                  tooltip="Median drawdown across simulations. The drawdown you should typically expect to experience."
                />
                <MetricCard
                  title="Worst Case (P95)"
                  value={`${safeToFixed((result.monte_carlo.p95_max_drawdown ?? 0) * 100, 1)}%`}
                  color={(result.monte_carlo.p95_max_drawdown ?? 0) > -0.3 ? '#ffc107' : '#dc3545'}
                  tooltip="95th percentile drawdown. Only 5% of simulations had worse drawdowns. Your stress-test scenario."
                />
              </div>
            </>
          )}
          
          {/* Percentile Distributions - Win Rate */}
          {(result.monte_carlo.p5_win_rate !== undefined || result.monte_carlo.p50_win_rate !== undefined) && (
            <>
              <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>🎯 Win Rate Distribution (P5 / P50 / P95)</h4>
              <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
                gap: '15px',
              }}>
                <MetricCard
                  title="Worst Case (P5)"
                  value={`${safeToFixed((result.monte_carlo.p5_win_rate ?? 0) * 100, 1)}%`}
                  color={(result.monte_carlo.p5_win_rate ?? 0) > 0.45 ? '#28a745' : (result.monte_carlo.p5_win_rate ?? 0) > 0.35 ? '#ffc107' : '#dc3545'}
                  tooltip="5th percentile win rate. Only 5% of simulations had a lower win rate. Your pessimistic scenario."
                />
                <MetricCard
                  title="Typical (P50)"
                  value={`${safeToFixed((result.monte_carlo.p50_win_rate ?? 0) * 100, 1)}%`}
                  color={(result.monte_carlo.p50_win_rate ?? 0) > 0.5 ? '#28a745' : '#ffc107'}
                  tooltip="Median win rate. The win rate you can most reliably expect across different market conditions."
                />
                <MetricCard
                  title="Best Case (P95)"
                  value={`${safeToFixed((result.monte_carlo.p95_win_rate ?? 0) * 100, 1)}%`}
                  color="#28a745"
                  tooltip="95th percentile win rate. Only 5% of simulations achieved a higher win rate. Your optimistic scenario."
                />
              </div>
            </>
          )}
        </div>
      )}

      {/* Walk-Forward Analysis Results */}
      {result.walk_forward && (
        <div style={{
          backgroundColor: 'white',
          padding: '20px',
          borderRadius: '8px',
          boxShadow: '0 2px 4px rgba(0,0,0,0.1)',
          marginBottom: '20px',
        }}>
          <h3 style={{ marginTop: 0, marginBottom: '15px' }}>🔄 Walk-Forward Analysis ({result.walk_forward.periods_tested} periods)</h3>
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
            gap: '15px',
          }}>
            <MetricCard
              title="Avg Performance"
              value={`${safeToFixed(result.walk_forward.average_performance * 100, 2)}%`}
              color={result.walk_forward.average_performance > 0 ? '#28a745' : '#dc3545'}
              tooltip="Average return across all out-of-sample test periods. Measures how well the strategy performs on unseen data."
            />
            <MetricCard
              title="Consistency"
              value={`${safeToFixed(result.walk_forward.consistency_score * 100, 1)}%`}
              color={result.walk_forward.consistency_score > 0.7 ? '#28a745' : result.walk_forward.consistency_score > 0.5 ? '#ffc107' : '#dc3545'}
              tooltip="Percentage of out-of-sample periods that were profitable. Higher = more reliable strategy across different time periods."
            />
            <MetricCard
              title="OOS Degradation"
              value={`${safeToFixed(result.walk_forward.out_of_sample_degradation * 100, 1)}%`}
              color={result.walk_forward.out_of_sample_degradation < 0.2 ? '#28a745' : result.walk_forward.out_of_sample_degradation < 0.4 ? '#ffc107' : '#dc3545'}
              tooltip="Performance drop from in-sample to out-of-sample. Low = less overfitting. High = strategy may not generalize well."
            />
          </div>
        </div>
      )}

      {/* Trade Analysis */}
      {(result.avg_trade_return !== undefined || result.best_trade !== undefined || result.avg_time_in_trade !== undefined || result.volatility !== undefined || result.sortino_ratio !== undefined || result.calmar_ratio !== undefined || result.annualized_return !== undefined) && (
        <div style={{
          backgroundColor: 'white',
          padding: '20px',
          borderRadius: '8px',
          boxShadow: '0 2px 4px rgba(0,0,0,0.1)',
          marginBottom: '20px',
        }}>
          <h3 style={{ marginTop: 0, marginBottom: '15px' }}>📊 Trade Analysis</h3>
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))',
            gap: '15px',
          }}>
            {result.annualized_return !== undefined && (
              <MetricCard
                title="Annualized Return"
                value={`${safeToFixed((result.annualized_return ?? 0) * 100, 2)}%`}
                color={(result.annualized_return ?? 0) > 0 ? '#28a745' : '#dc3545'}
                tooltip="Expected yearly return if performance continues. Extrapolated from backtest period to 1 year."
              />
            )}
            {result.avg_trade_return !== undefined && (
              <MetricCard
                title="Avg Trade Return"
                value={`$${safeToFixed(result.avg_trade_return, 2)}`}
                color={(result.avg_trade_return ?? 0) > 0 ? '#28a745' : '#dc3545'}
                tooltip="Average profit/loss per completed trade. Positive values needed for long-term profitability."
              />
            )}
            {result.best_trade !== undefined && (
              <MetricCard
                title="Best Trade"
                value={safeToFixed(result.best_trade, 2)}
                color="#28a745"
                tooltip="Largest single trade profit. Very high values may indicate outlier-dependent performance."
              />
            )}
            {result.worst_trade !== undefined && (
              <MetricCard
                title="Worst Trade"
                value={safeToFixed(result.worst_trade, 2)}
                color="#dc3545"
                tooltip="Largest single trade loss. Compare with position sizing to assess risk management."
              />
            )}
            {result.avg_time_in_trade !== undefined && (
              <MetricCard
                title="Avg Time in Trade"
                value={`${safeToFixed(result.avg_time_in_trade, 1)}h`}
                color="#007bff"
                tooltip="Average duration of each position. Shorter = higher turnover. Longer = more capital commitment."
              />
            )}
            {result.volatility !== undefined && (
              <MetricCard
                title="Volatility"
                value={`${safeToFixed((result.volatility ?? 0) * 100, 2)}%`}
                color={(result.volatility ?? 0) < 0.20 ? '#28a745' : (result.volatility ?? 0) < 0.40 ? '#ffc107' : '#dc3545'}
                tooltip="Annualized standard deviation of returns. Lower = smoother equity curve. Higher = more volatile performance."
              />
            )}
            {result.sortino_ratio !== undefined && (
              <MetricCard
                title="Sortino Ratio"
                value={safeToFixed(result.sortino_ratio, 2)}
                color={result.sortino_ratio > 1.5 ? '#28a745' : result.sortino_ratio > 1 ? '#ffc107' : '#dc3545'}
                tooltip="Like Sharpe, but only penalizes downside volatility. Better for asymmetric strategies. Above 1.5 is good."
              />
            )}
            {result.calmar_ratio !== undefined && (
              <MetricCard
                title="Calmar Ratio"
                value={safeToFixed(result.calmar_ratio, 2)}
                color={result.calmar_ratio > 1 ? '#28a745' : result.calmar_ratio > 0.5 ? '#ffc107' : '#dc3545'}
                tooltip="Annualized return divided by max drawdown. Higher = better return per unit of drawdown risk. Above 1 is good."
              />
            )}
          </div>
        </div>
      )}

      {/* Risk Metrics */}
      {(result.value_at_risk_95 !== undefined || result.expected_shortfall_95 !== undefined || result.current_drawdown !== undefined || result.avg_drawdown !== undefined || result.max_drawdown_duration_days !== undefined) && (
        <div style={{
          backgroundColor: 'white',
          padding: '20px',
          borderRadius: '8px',
          boxShadow: '0 2px 4px rgba(0,0,0,0.1)',
          marginBottom: '20px',
        }}>
          <h3 style={{ marginTop: 0, marginBottom: '15px' }}>⚠️ Risk Metrics</h3>
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))',
            gap: '15px',
          }}>
            {result.value_at_risk_95 !== undefined && (
              <MetricCard
                title="VaR (95%)"
                value={`$${safeToFixed(result.value_at_risk_95, 2)} (${safeToFixed((result.value_at_risk_95 / (jobDetails?.initial_capital ?? 10000)) * 100, 2)}%)`}
                color={(result.value_at_risk_95 / (jobDetails?.initial_capital ?? 10000)) < 0.05 ? '#28a745' : (result.value_at_risk_95 / (jobDetails?.initial_capital ?? 10000)) < 0.10 ? '#ffc107' : '#dc3545'}
                tooltip="Value at Risk - maximum expected daily loss at 95% confidence. 5% of days may have larger losses."
              />
            )}
            {result.expected_shortfall_95 !== undefined && (
              <MetricCard
                title="Expected Shortfall"
                value={`$${safeToFixed(result.expected_shortfall_95, 2)} (${safeToFixed((result.expected_shortfall_95 / (jobDetails?.initial_capital ?? 10000)) * 100, 2)}%)`}
                color={(result.expected_shortfall_95 / (jobDetails?.initial_capital ?? 10000)) < 0.08 ? '#28a745' : (result.expected_shortfall_95 / (jobDetails?.initial_capital ?? 10000)) < 0.15 ? '#ffc107' : '#dc3545'}
                tooltip="Average loss on the worst 5% of days. Also called CVaR - captures extreme tail risk better than VaR."
              />
            )}
            {result.current_drawdown !== undefined && (
              <MetricCard
                title="Current Drawdown"
                value={`${safeToFixed((result.current_drawdown ?? 0) * 100, 2)}%`}
                color={(result.current_drawdown ?? 0) < 0.05 ? '#28a745' : (result.current_drawdown ?? 0) < 0.15 ? '#ffc107' : '#dc3545'}
                tooltip="How far below the peak equity the portfolio currently sits. Shows if in a recovery phase."
              />
            )}
            {result.avg_drawdown !== undefined && (
              <MetricCard
                title="Avg Drawdown"
                value={`${safeToFixed((result.avg_drawdown ?? 0) * 100, 2)}%`}
                color={(result.avg_drawdown ?? 0) < 0.05 ? '#28a745' : (result.avg_drawdown ?? 0) < 0.10 ? '#ffc107' : '#dc3545'}
                tooltip="Mean drawdown across all drawdown periods. Lower = smoother equity curve with smaller dips."
              />
            )}
            {result.max_drawdown_duration_days !== undefined && (
              <MetricCard
                title="Max DD Duration"
                value={`${result.max_drawdown_duration_days} days`}
                color={result.max_drawdown_duration_days < 30 ? '#28a745' : result.max_drawdown_duration_days < 90 ? '#ffc107' : '#dc3545'}
                tooltip="Longest time spent in drawdown before recovering. Tests patience during losing periods."
              />
            )}
          </div>
        </div>
      )}

      {/* Execution Metrics */}
      {(result.total_orders !== undefined || result.filled_orders !== undefined || result.cancelled_orders !== undefined || result.avg_slippage !== undefined || result.total_commission_paid !== undefined) && (
        <div style={{
          backgroundColor: 'white',
          padding: '20px',
          borderRadius: '8px',
          boxShadow: '0 2px 4px rgba(0,0,0,0.1)',
          marginBottom: '20px',
        }}>
          <h3 style={{ marginTop: 0, marginBottom: '15px' }}>⚙️ Execution Metrics</h3>
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))',
            gap: '15px',
          }}>
            {result.total_orders !== undefined && (
              <MetricCard
                title="Total Orders"
                value={result.total_orders.toString()}
                color="#007bff"
                tooltip="Total number of orders submitted. Includes filled, cancelled, and rejected orders."
              />
            )}
            {result.filled_orders !== undefined && (
              <MetricCard
                title="Filled Orders"
                value={result.filled_orders.toString()}
                color="#28a745"
                tooltip="Number of orders that were successfully executed. Higher = better order placement."
              />
            )}
            {result.cancelled_orders !== undefined && (
              <MetricCard
                title="Cancelled"
                value={result.cancelled_orders.toString()}
                color={result.cancelled_orders === 0 ? '#28a745' : '#ffc107'}
                tooltip="Orders cancelled before execution. Some cancellations are normal for market makers adjusting quotes."
              />
            )}
            {result.avg_slippage !== undefined && (
              <MetricCard
                title="Avg Slippage"
                value={`${safeToFixed(result.avg_slippage * 100, 3)}%`}
                color={result.avg_slippage < 0.001 ? '#28a745' : result.avg_slippage < 0.005 ? '#ffc107' : '#dc3545'}
                tooltip="Average price difference between expected and actual fill. Lower = better execution quality."
              />
            )}
            {result.total_commission_paid !== undefined && (
              <MetricCard
                title="Total Commission"
                value={`$${safeToFixed(result.total_commission_paid, 2)}`}
                color="#6c757d"
                tooltip="Total fees paid for all trades. Compare with P&L to understand fee impact on returns."
              />
            )}
            {result.filled_orders !== undefined && result.total_orders !== undefined && result.total_orders > 0 && (
              <MetricCard
                title="Fill Rate"
                value={`${safeToFixed((result.filled_orders / result.total_orders) * 100, 1)}%`}
                color={result.filled_orders / result.total_orders > 0.95 ? '#28a745' : result.filled_orders / result.total_orders > 0.8 ? '#ffc107' : '#dc3545'}
                tooltip="Percentage of orders that got filled. Higher = more efficient order placement and better market access."
              />
            )}
          </div>
        </div>
      )}

      {/* Strategy Parameters */}
      {result.strategy_params && (
        <div style={{
          backgroundColor: 'white',
          padding: '20px',
          borderRadius: '8px',
          boxShadow: '0 2px 4px rgba(0,0,0,0.1)',
          marginBottom: '20px',
        }}>
          <h3 style={{ marginTop: 0, marginBottom: '15px' }}>🎯 Optimized Strategy Parameters (Avellaneda-Stoikov Market Making)</h3>
          
          {/* Core Parameters */}
          <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>Core Parameters</h4>
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
            gap: '15px',
            marginBottom: '20px',
          }}>
            {result.strategy_params.risk_aversion !== undefined && (
              <MetricCard
                title="Risk Aversion (γ)"
                value={safeToFixed(parseFloat(result.strategy_params.risk_aversion), 4)}
                color="#6f42c1"
                tooltip="Controls how aggressively the strategy manages inventory risk. Higher = wider spreads when holding inventory."
              />
            )}
            {result.strategy_params.inventory_target !== undefined && (
              <MetricCard
                title="Inventory Target"
                value={safeToFixed(parseFloat(result.strategy_params.inventory_target), 4)}
                color="#6f42c1"
                tooltip="Desired inventory level to maintain. Strategy skews quotes to move inventory toward this target."
              />
            )}
            {result.strategy_params.order_size !== undefined && (
              <MetricCard
                title="Order Size (units)"
                value={safeToFixed(parseFloat(result.strategy_params.order_size), 6)}
                color="#6f42c1"
                tooltip="Fixed size of each order in base units. Determines position sizing per trade."
              />
            )}
            {result.strategy_params.order_size_pct !== undefined && (
              <MetricCard
                title="Order Size (%)"
                value={`${safeToFixed(parseFloat(result.strategy_params.order_size_pct) * 100, 2)}%`}
                color="#6f42c1"
                tooltip="Order size as percentage of capital. Dynamic sizing based on portfolio value."
              />
            )}
            {result.strategy_params.market_depth_k !== undefined && (
              <MetricCard
                title="Market Depth (k)"
                value={safeToFixed(parseFloat(result.strategy_params.market_depth_k), 2)}
                color="#6f42c1"
                tooltip="Market impact parameter. Higher k = more liquid market, tighter optimal spreads."
              />
            )}
            {result.strategy_params.time_horizon_minutes !== undefined && (
              <MetricCard
                title="Time Horizon"
                value={`${safeToFixed(parseFloat(result.strategy_params.time_horizon_minutes), 1)} min`}
                color="#6f42c1"
                tooltip="Trading session length for quote calculations. Affects urgency of inventory management."
              />
            )}
            {result.strategy_params.initial_capital !== undefined && (
              <MetricCard
                title="Initial Capital"
                value={`$${formatLargeNumber(parseFloat(result.strategy_params.initial_capital))}`}
                color="#6f42c1"
                tooltip="Starting capital for the backtest. All returns are calculated relative to this amount."
              />
            )}
            {result.strategy_params.max_inventory_pct !== undefined && (
              <MetricCard
                title="Max Inventory"
                value={`${safeToFixed(parseFloat(result.strategy_params.max_inventory_pct) * 100, 1)}%`}
                color="#6f42c1"
                tooltip="Maximum position size as % of capital. Risk limit that prevents overexposure."
              />
            )}
            {result.strategy_params.inventory_skew_multiplier !== undefined && (
              <MetricCard
                title="Inv Skew Mult"
                value={`${safeToFixed(parseFloat(result.strategy_params.inventory_skew_multiplier), 2)}x`}
                color="#6f42c1"
                tooltip="How aggressively quotes skew based on inventory. Higher = more aggressive inventory control."
              />
            )}
          </div>
          
          {/* Spread & Volatility Parameters */}
          <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>Spread & Volatility</h4>
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
            gap: '15px',
            marginBottom: '20px',
          }}>
            {result.strategy_params.min_spread_bps !== undefined && (
              <MetricCard
                title="Min Spread"
                value={`${safeToFixed(parseFloat(result.strategy_params.min_spread_bps), 1)} bps`}
                color="#17a2b8"
                tooltip="Minimum bid-ask spread in basis points. Floor prevents losses from fees."
              />
            )}
            {result.strategy_params.volatility_cap !== undefined && (
              <MetricCard
                title="Volatility Cap"
                value={`${safeToFixed(parseFloat(result.strategy_params.volatility_cap) * 100, 1)}%`}
                color="#17a2b8"
                tooltip="Maximum volatility used in spread calculation. Prevents extreme spreads during high volatility."
              />
            )}
            {result.strategy_params.volatility_adjustment_factor !== undefined && (
              <MetricCard
                title="Vol Adjustment"
                value={safeToFixed(parseFloat(result.strategy_params.volatility_adjustment_factor), 2)}
                color="#17a2b8"
                tooltip="Multiplier for volatility in spread calculation. Higher = wider spreads for given volatility."
              />
            )}
            {result.strategy_params.fee_rate !== undefined && (
              <MetricCard
                title="Fee Rate"
                value={`${safeToFixed(parseFloat(result.strategy_params.fee_rate) * 100, 3)}%`}
                color="#17a2b8"
                tooltip="Trading fee per order. Used to calculate minimum profitable spread."
              />
            )}
          </div>
          
          {/* Dynamic Adjustment Parameters */}
          {(result.strategy_params.inventory_penalty_factor !== undefined || 
            result.strategy_params.trend_sensitivity !== undefined ||
            result.strategy_params.flow_sensitivity !== undefined) && (
            <>
              <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>Dynamic Adjustments (Phase 2)</h4>
              <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
                gap: '15px',
                marginBottom: '20px',
              }}>
                {result.strategy_params.inventory_penalty_factor !== undefined && (
                  <MetricCard
                    title="Inventory Penalty"
                    value={safeToFixed(parseFloat(result.strategy_params.inventory_penalty_factor), 2)}
                    color="#fd7e14"
                    tooltip="Extra penalty for holding large inventory. Encourages faster inventory turnover."
                  />
                )}
                {result.strategy_params.trend_sensitivity !== undefined && (
                  <MetricCard
                    title="Trend Sensitivity"
                    value={safeToFixed(parseFloat(result.strategy_params.trend_sensitivity), 2)}
                    color="#fd7e14"
                    tooltip="How much quotes adjust based on price trend. Higher = more trend-following."
                  />
                )}
                {result.strategy_params.flow_sensitivity !== undefined && (
                  <MetricCard
                    title="Flow Sensitivity"
                    value={safeToFixed(parseFloat(result.strategy_params.flow_sensitivity), 2)}
                    color="#fd7e14"
                    tooltip="How much quotes adjust to order flow imbalance. Higher = more reactive to buying/selling pressure."
                  />
                )}
              </div>
            </>
          )}
          
          {/* Phase 3: Dynamic K Parameters */}
          {(result.strategy_params.k_volatility_sensitivity !== undefined || 
            result.strategy_params.k_min !== undefined) && (
            <>
              <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>Dynamic K (Phase 3)</h4>
              <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
                gap: '15px',
                marginBottom: '20px',
              }}>
                {result.strategy_params.k_volatility_sensitivity !== undefined && (
                  <MetricCard
                    title="K Vol Sensitivity"
                    value={safeToFixed(parseFloat(result.strategy_params.k_volatility_sensitivity), 2)}
                    color="#ff6b6b"
                    tooltip="How much market depth parameter k adjusts with volatility. Higher = more conservative in volatile markets."
                  />
                )}
                {result.strategy_params.k_flow_sensitivity !== undefined && (
                  <MetricCard
                    title="K Flow Sensitivity"
                    value={safeToFixed(parseFloat(result.strategy_params.k_flow_sensitivity), 2)}
                    color="#ff6b6b"
                    tooltip="How much k adjusts based on order flow. Higher = more adaptive to market activity."
                  />
                )}
                {result.strategy_params.k_min !== undefined && (
                  <MetricCard
                    title="K Min"
                    value={safeToFixed(parseFloat(result.strategy_params.k_min), 2)}
                    color="#ff6b6b"
                    tooltip="Minimum allowed market depth parameter. Floor prevents too-tight spreads."
                  />
                )}
                {result.strategy_params.k_max !== undefined && (
                  <MetricCard
                    title="K Max"
                    value={safeToFixed(parseFloat(result.strategy_params.k_max), 2)}
                    color="#ff6b6b"
                    tooltip="Maximum allowed market depth parameter. Cap prevents too-wide spreads."
                  />
                )}
              </div>
            </>
          )}
          
          {/* Phase 4: Dynamic Time Horizon */}
          {(result.strategy_params.time_horizon_vol_sensitivity !== undefined || 
            result.strategy_params.time_horizon_min !== undefined) && (
            <>
              <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>Dynamic Time Horizon (Phase 4)</h4>
              <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
                gap: '15px',
                marginBottom: '20px',
              }}>
                {result.strategy_params.time_horizon_vol_sensitivity !== undefined && (
                  <MetricCard
                    title="T Vol Sensitivity"
                    value={safeToFixed(parseFloat(result.strategy_params.time_horizon_vol_sensitivity), 2)}
                    color="#4ecdc4"
                    tooltip="How time horizon adjusts with volatility. Higher = shorter horizons in volatile markets."
                  />
                )}
                {result.strategy_params.time_horizon_min !== undefined && (
                  <MetricCard
                    title="T Min (min)"
                    value={`${safeToFixed(parseFloat(result.strategy_params.time_horizon_min), 2)}`}
                    color="#4ecdc4"
                    tooltip="Minimum time horizon in minutes. Prevents too-aggressive inventory management."
                  />
                )}
                {result.strategy_params.time_horizon_max !== undefined && (
                  <MetricCard
                    title="T Max (min)"
                    value={`${safeToFixed(parseFloat(result.strategy_params.time_horizon_max), 2)}`}
                    color="#4ecdc4"
                    tooltip="Maximum time horizon in minutes. Prevents too-passive inventory management."
                  />
                )}
              </div>
            </>
          )}
          
          {/* Phase 4: Dynamic Min Spread */}
          {(result.strategy_params.min_spread_vol_sensitivity !== undefined || 
            result.strategy_params.min_spread_min_bps !== undefined) && (
            <>
              <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>Dynamic Min Spread (Phase 4)</h4>
              <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
                gap: '15px',
                marginBottom: '20px',
              }}>
                {result.strategy_params.min_spread_vol_sensitivity !== undefined && (
                  <MetricCard
                    title="Spread Vol Sensitivity"
                    value={safeToFixed(parseFloat(result.strategy_params.min_spread_vol_sensitivity), 2)}
                    color="#95e1d3"
                    tooltip="How min spread adjusts with volatility. Higher = wider minimum spreads in volatile markets."
                  />
                )}
                {result.strategy_params.min_spread_flow_sensitivity !== undefined && (
                  <MetricCard
                    title="Spread Flow Sensitivity"
                    value={safeToFixed(parseFloat(result.strategy_params.min_spread_flow_sensitivity), 2)}
                    color="#95e1d3"
                    tooltip="How min spread adjusts with order flow. Higher = wider spreads during heavy trading."
                  />
                )}
                {result.strategy_params.min_spread_min_bps !== undefined && (
                  <MetricCard
                    title="Spread Floor"
                    value={`${safeToFixed(parseFloat(result.strategy_params.min_spread_min_bps), 1)} bps`}
                    color="#95e1d3"
                    tooltip="Absolute minimum spread in basis points. Never go below this to ensure profitability."
                  />
                )}
                {result.strategy_params.min_spread_max_bps !== undefined && (
                  <MetricCard
                    title="Spread Cap"
                    value={`${safeToFixed(parseFloat(result.strategy_params.min_spread_max_bps), 1)} bps`}
                    color="#95e1d3"
                    tooltip="Maximum minimum spread in basis points. Prevents spreads from getting too wide."
                  />
                )}
              </div>
            </>
          )}
          
          {/* Phase 4: Dynamic Order Size */}
          {(result.strategy_params.size_vol_sensitivity !== undefined || 
            result.strategy_params.size_min_pct !== undefined) && (
            <>
              <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>Dynamic Order Size (Phase 4)</h4>
              <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
                gap: '15px',
                marginBottom: '20px',
              }}>
                {result.strategy_params.size_vol_sensitivity !== undefined && (
                  <MetricCard
                    title="Size Vol Sensitivity"
                    value={safeToFixed(parseFloat(result.strategy_params.size_vol_sensitivity), 2)}
                    color="#f9ca24"
                    tooltip="How order size adjusts with volatility. Higher = smaller orders in volatile markets."
                  />
                )}
                {result.strategy_params.size_drawdown_sensitivity !== undefined && (
                  <MetricCard
                    title="Size DD Sensitivity"
                    value={safeToFixed(parseFloat(result.strategy_params.size_drawdown_sensitivity), 2)}
                    color="#f9ca24"
                    tooltip="How order size reduces during drawdowns. Higher = more conservative when losing."
                  />
                )}
                {result.strategy_params.size_win_boost !== undefined && (
                  <MetricCard
                    title="Win Boost"
                    value={`${safeToFixed(parseFloat(result.strategy_params.size_win_boost) * 100, 1)}%`}
                    color="#f9ca24"
                    tooltip="Size increase after winning streaks. Allows scaling up during good performance."
                  />
                )}
                {result.strategy_params.size_min_pct !== undefined && (
                  <MetricCard
                    title="Size Min"
                    value={`${safeToFixed(parseFloat(result.strategy_params.size_min_pct) * 100, 2)}%`}
                    color="#f9ca24"
                    tooltip="Minimum order size as % of capital. Ensures orders remain meaningful."
                  />
                )}
                {result.strategy_params.size_max_pct !== undefined && (
                  <MetricCard
                    title="Size Max"
                    value={`${safeToFixed(parseFloat(result.strategy_params.size_max_pct) * 100, 2)}%`}
                    color="#f9ca24"
                    tooltip="Maximum order size as % of capital. Risk limit per single order."
                  />
                )}
              </div>
            </>
          )}
          
          {/* Phase 4: Dynamic Rate Limiting */}
          {(result.strategy_params.rate_vol_sensitivity !== undefined || 
            result.strategy_params.rate_min !== undefined) && (
            <>
              <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>Dynamic Rate Limiting (Phase 4)</h4>
              <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
                gap: '15px',
                marginBottom: '20px',
              }}>
                {result.strategy_params.rate_vol_sensitivity !== undefined && (
                  <MetricCard
                    title="Rate Vol Sensitivity"
                    value={safeToFixed(parseFloat(result.strategy_params.rate_vol_sensitivity), 2)}
                    color="#a29bfe"
                    tooltip="How quote refresh rate adjusts with volatility. Higher = faster updates in volatile markets."
                  />
                )}
                {result.strategy_params.rate_min !== undefined && (
                  <MetricCard
                    title="Rate Min"
                    value={`${safeToFixed(parseFloat(result.strategy_params.rate_min), 0)}/min`}
                    color="#a29bfe"
                    tooltip="Minimum quote updates per minute. Ensures quotes stay fresh."
                  />
                )}
                {result.strategy_params.rate_max !== undefined && (
                  <MetricCard
                    title="Rate Max"
                    value={`${safeToFixed(parseFloat(result.strategy_params.rate_max), 0)}/min`}
                    color="#a29bfe"
                    tooltip="Maximum quote updates per minute. Prevents excessive API calls and fees."
                  />
                )}
              </div>
            </>
          )}
          
          {/* Timing Parameters */}
          {(result.strategy_params.window_ms !== undefined || 
            result.strategy_params.min_quote_lifetime_ms !== undefined ||
            result.strategy_params.max_trades_per_minute !== undefined) && (
            <>
              <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>Timing & Rate Limits</h4>
              <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
                gap: '15px',
                marginBottom: '20px',
              }}>
                {result.strategy_params.window_ms !== undefined && (
                  <MetricCard
                    title="EWMA Window"
                    value={`${parseFloat(result.strategy_params.window_ms) / 1000}s`}
                    color="#20c997"
                    tooltip="Exponentially weighted moving average window for volatility calculation."
                  />
                )}
                {result.strategy_params.min_quote_lifetime_ms !== undefined && (
                  <MetricCard
                    title="Min Quote Life"
                    value={`${parseFloat(result.strategy_params.min_quote_lifetime_ms)}ms`}
                    color="#20c997"
                    tooltip="Minimum time quotes remain active before cancellation. Prevents quote flickering."
                  />
                )}
                {result.strategy_params.max_trades_per_minute !== undefined && (
                  <MetricCard
                    title="Max Trades/Min"
                    value={safeToFixed(parseFloat(result.strategy_params.max_trades_per_minute), 0)}
                    color="#20c997"
                    tooltip="Maximum trade execution rate. Prevents over-trading during volatile periods."
                  />
                )}
              </div>
            </>
          )}
          
          {/* Risk Limits */}
          {(result.strategy_params.take_profit_pct !== undefined || 
            result.strategy_params.stop_loss_pct !== undefined) && (
            <>
              <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>Risk Limits</h4>
              <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
                gap: '15px',
              }}>
                {result.strategy_params.take_profit_pct !== undefined && (
                  <MetricCard
                    title="Take Profit"
                    value={`${safeToFixed(parseFloat(result.strategy_params.take_profit_pct) * 100, 2)}%`}
                    color="#28a745"
                    tooltip="Target profit percentage to close position. Locks in gains at this level."
                  />
                )}
                {result.strategy_params.stop_loss_pct !== undefined && (
                  <MetricCard
                    title="Stop Loss"
                    value={`${safeToFixed(parseFloat(result.strategy_params.stop_loss_pct) * 100, 2)}%`}
                    color="#dc3545"
                    tooltip="Maximum loss percentage before closing position. Protects from large losses."
                  />
                )}
              </div>
            </>
          )}
          
          {/* Phase 5: Regime Detection */}
          {(result.strategy_params.regime_vol_threshold_high !== undefined || 
            result.strategy_params.regime_high_vol_spread_mult !== undefined) && (
            <>
              <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>🌡️ Regime Detection (Phase 5)</h4>
              <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
                gap: '15px',
                marginBottom: '20px',
              }}>
                {result.strategy_params.regime_vol_threshold_high !== undefined && (
                  <MetricCard
                    title="High Vol Threshold"
                    value={`${safeToFixed(parseFloat(result.strategy_params.regime_vol_threshold_high), 2)}x`}
                    color="#e83e8c"
                    tooltip="Volatility multiple that triggers high-volatility regime adjustments."
                  />
                )}
                {result.strategy_params.regime_vol_threshold_low !== undefined && (
                  <MetricCard
                    title="Low Vol Threshold"
                    value={`${safeToFixed(parseFloat(result.strategy_params.regime_vol_threshold_low), 2)}x`}
                    color="#e83e8c"
                    tooltip="Volatility multiple that triggers low-volatility regime adjustments."
                  />
                )}
                {result.strategy_params.regime_trend_threshold !== undefined && (
                  <MetricCard
                    title="Trend Threshold"
                    value={`${safeToFixed(parseFloat(result.strategy_params.regime_trend_threshold) * 100, 1)}%`}
                    color="#e83e8c"
                    tooltip="Price change threshold that triggers trending regime detection."
                  />
                )}
                {result.strategy_params.regime_high_vol_spread_mult !== undefined && (
                  <MetricCard
                    title="High Vol Spread Mult"
                    value={`${safeToFixed(parseFloat(result.strategy_params.regime_high_vol_spread_mult), 2)}x`}
                    color="#e83e8c"
                    tooltip="Spread multiplier during high-volatility regimes. Wider spreads = safer."
                  />
                )}
                {result.strategy_params.regime_low_vol_spread_mult !== undefined && (
                  <MetricCard
                    title="Low Vol Spread Mult"
                    value={`${safeToFixed(parseFloat(result.strategy_params.regime_low_vol_spread_mult), 2)}x`}
                    color="#e83e8c"
                    tooltip="Spread multiplier during low-volatility regimes. Tighter spreads = more competitive."
                  />
                )}
                {result.strategy_params.regime_trending_inv_mult !== undefined && (
                  <MetricCard
                    title="Trending Inv Mult"
                    value={`${safeToFixed(parseFloat(result.strategy_params.regime_trending_inv_mult), 2)}x`}
                    color="#e83e8c"
                    tooltip="Inventory skew multiplier during trending regimes. More aggressive inventory control."
                  />
                )}
              </div>
            </>
          )}
          
          {/* Phase 5: Order Book Microstructure */}
          {(result.strategy_params.orderbook_imbalance_sensitivity !== undefined || 
            result.strategy_params.orderbook_depth_levels !== undefined) && (
            <>
              <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>📚 Orderbook Microstructure (Phase 5)</h4>
              <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
                gap: '15px',
                marginBottom: '20px',
              }}>
                {result.strategy_params.orderbook_imbalance_sensitivity !== undefined && (
                  <MetricCard
                    title="Imbalance Sensitivity"
                    value={`${safeToFixed(parseFloat(result.strategy_params.orderbook_imbalance_sensitivity) * 100, 1)}%`}
                    color="#6610f2"
                    tooltip="How much quotes adjust based on order book imbalance. Higher = more reactive to buy/sell pressure."
                  />
                )}
                {result.strategy_params.orderbook_depth_levels !== undefined && (
                  <MetricCard
                    title="Depth Levels"
                    value={safeToFixed(parseFloat(result.strategy_params.orderbook_depth_levels), 0)}
                    color="#6610f2"
                    tooltip="Number of orderbook levels analyzed for imbalance calculation."
                  />
                )}
                {result.strategy_params.imbalance_skew_factor !== undefined && (
                  <MetricCard
                    title="Imbalance Skew"
                    value={`${safeToFixed(parseFloat(result.strategy_params.imbalance_skew_factor) * 100, 1)}%`}
                    color="#6610f2"
                    tooltip="How much to skew quotes based on orderbook imbalance."
                  />
                )}
              </div>
            </>
          )}
          
          {/* Phase 5: Adverse Selection */}
          {(result.strategy_params.adverse_selection_multiplier !== undefined || 
            result.strategy_params.toxic_flow_threshold !== undefined) && (
            <>
              <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>🛡️ Adverse Selection (Phase 5)</h4>
              <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
                gap: '15px',
              }}>
                {result.strategy_params.informed_trader_prob_window_ms !== undefined && (
                  <MetricCard
                    title="Detection Window"
                    value={`${parseFloat(result.strategy_params.informed_trader_prob_window_ms) / 1000}s`}
                    color="#d63384"
                    tooltip="Time window for detecting informed/toxic order flow patterns."
                  />
                )}
                {result.strategy_params.adverse_selection_multiplier !== undefined && (
                  <MetricCard
                    title="Adverse Sel. Mult"
                    value={`${safeToFixed(parseFloat(result.strategy_params.adverse_selection_multiplier), 2)}x`}
                    color="#d63384"
                    tooltip="Spread multiplier when adverse selection is detected. Protects from informed traders."
                  />
                )}
                {result.strategy_params.toxic_flow_threshold !== undefined && (
                  <MetricCard
                    title="Toxic Flow Threshold"
                    value={`${safeToFixed(parseFloat(result.strategy_params.toxic_flow_threshold) * 100, 1)}%`}
                    color="#d63384"
                    tooltip="Order flow toxicity level that triggers defensive spread widening."
                  />
                )}
              </div>
            </>
          )}
        </div>
      )}

      {/* Market Making Inventory Metrics */}
      {result.inventory_metrics && (
        <div style={{
          backgroundColor: 'white',
          padding: '20px',
          borderRadius: '8px',
          boxShadow: '0 2px 4px rgba(0,0,0,0.1)',
          marginBottom: '20px',
        }}>
          <h3 style={{ marginTop: 0, marginBottom: '15px' }}>📦 Inventory Metrics (Market Making)</h3>
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
            gap: '15px',
          }}>
            <MetricCard
              title="Total Volume"
              value={`$${formatLargeNumber(result.inventory_metrics.total_volume)}`}
              color="#007bff"
              tooltip="Total dollar volume traded (buys + sells). Higher = more trading activity."
            />
            <MetricCard
              title="Avg Inventory"
              value={`$${formatLargeNumber(result.inventory_metrics.avg_inventory_value)}`}
              color="#6c757d"
              tooltip="Average inventory value held. Lower = less capital at risk."
            />
            <MetricCard
              title="Turnover/Day"
              value={`${safeToFixed(result.inventory_metrics.turnover_per_day, 2)}x`}
              color={result.inventory_metrics.turnover_per_day > 1 ? '#28a745' : '#ffc107'}
              tooltip="Daily inventory turnover rate. Higher = faster inventory cycling."
            />
            <MetricCard
              title="Buy Fills"
              value={(result.inventory_metrics.buy_fills ?? 0).toString()}
              color="#28a745"
              tooltip="Number of buy orders that got filled."
            />
            <MetricCard
              title="Sell Fills"
              value={(result.inventory_metrics.sell_fills ?? 0).toString()}
              color="#dc3545"
              tooltip="Number of sell orders that got filled."
            />
            <MetricCard
              title="Buy Volume"
              value={`$${formatLargeNumber(result.inventory_metrics.buy_volume)}`}
              color="#28a745"
              tooltip="Total dollar value of all buy orders filled."
            />
            <MetricCard
              title="Sell Volume"
              value={`$${formatLargeNumber(result.inventory_metrics.sell_volume)}`}
              color="#dc3545"
              tooltip="Total dollar value of all sell orders filled."
            />
            <MetricCard
              title="Imbalance Ratio"
              value={`${safeToFixed(result.inventory_metrics.imbalance_ratio * 100, 2)}%`}
              color={Math.abs(result.inventory_metrics.imbalance_ratio) < 0.1 ? '#28a745' : Math.abs(result.inventory_metrics.imbalance_ratio) < 0.25 ? '#ffc107' : '#dc3545'}
              tooltip="Buy vs Sell volume imbalance. Near 0% = balanced trading."
            />
          </div>
          
          {/* Position Breakdown - Starting vs Ending */}
          {(result.inventory_metrics.starting_capital !== undefined || result.inventory_metrics.final_equity !== undefined) && (
            <>
              <h4 style={{ margin: '20px 0 10px', fontSize: '14px', color: '#666' }}>💰 Position Breakdown</h4>
              <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(2, 1fr)',
                gap: '20px',
                marginBottom: '10px',
              }}>
                {/* Starting Position */}
                <div style={{
                  backgroundColor: '#f8f9fa',
                  padding: '15px',
                  borderRadius: '6px',
                  border: '1px solid #e9ecef',
                }}>
                  <h5 style={{ margin: '0 0 10px', color: '#495057' }}>📊 Starting Position</h5>
                  <div style={{ fontSize: '14px', lineHeight: '1.8' }}>
                    <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                      <span style={{ color: '#6c757d' }}>Cash:</span>
                      <span style={{ fontWeight: 600 }}>${formatCurrency(result.inventory_metrics.starting_capital ?? 0)}</span>
                    </div>
                    <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                      <span style={{ color: '#6c757d' }}>Inventory ({result.symbol?.split('/')[0] ?? 'Asset'}):</span>
                      <span style={{ fontWeight: 600 }}>{safeToFixed(result.inventory_metrics.starting_inventory ?? 0, 6)}</span>
                    </div>
                    <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                      <span style={{ color: '#6c757d' }}>Inventory Value:</span>
                      <span style={{ fontWeight: 600 }}>${formatCurrency(result.inventory_metrics.starting_inventory_value ?? 0)}</span>
                    </div>
                    <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                      <span style={{ color: '#6c757d' }}>Mark Price:</span>
                      <span style={{ fontWeight: 600 }}>${formatCurrency(result.inventory_metrics.starting_mark_price ?? 0, 4)}</span>
                    </div>
                    <div style={{ display: 'flex', justifyContent: 'space-between', borderTop: '1px solid #dee2e6', paddingTop: '5px', marginTop: '5px' }}>
                      <span style={{ color: '#495057', fontWeight: 600 }}>Total Equity:</span>
                      <span style={{ fontWeight: 700, color: '#007bff' }}>${formatCurrency((result.inventory_metrics.starting_capital ?? 0) + (result.inventory_metrics.starting_inventory_value ?? 0))}</span>
                    </div>
                  </div>
                </div>
                
                {/* Ending Position */}
                <div style={{
                  backgroundColor: '#f8f9fa',
                  padding: '15px',
                  borderRadius: '6px',
                  border: '1px solid #e9ecef',
                }}>
                  <h5 style={{ margin: '0 0 10px', color: '#495057' }}>📊 Ending Position</h5>
                  <div style={{ fontSize: '14px', lineHeight: '1.8' }}>
                    <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                      <span style={{ color: '#6c757d' }}>Cash:</span>
                      <span style={{ fontWeight: 600 }}>${formatCurrency(result.inventory_metrics.ending_capital ?? 0)}</span>
                    </div>
                    <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                      <span style={{ color: '#6c757d' }}>Inventory ({result.symbol?.split('/')[0] ?? 'Asset'}):</span>
                      <span style={{ fontWeight: 600 }}>{safeToFixed(result.inventory_metrics.ending_inventory ?? 0, 6)}</span>
                    </div>
                    <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                      <span style={{ color: '#6c757d' }}>Inventory Value:</span>
                      <span style={{ fontWeight: 600 }}>${formatCurrency(result.inventory_metrics.ending_inventory_value ?? 0)}</span>
                    </div>
                    <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                      <span style={{ color: '#6c757d' }}>Mark Price:</span>
                      <span style={{ fontWeight: 600 }}>${formatCurrency(result.inventory_metrics.ending_mark_price ?? 0, 4)}</span>
                    </div>
                    <div style={{ display: 'flex', justifyContent: 'space-between', borderTop: '1px solid #dee2e6', paddingTop: '5px', marginTop: '5px' }}>
                      <span style={{ color: '#495057', fontWeight: 600 }}>Final Equity:</span>
                      <span style={{ fontWeight: 700, color: (result.inventory_metrics.final_equity ?? 0) >= (result.inventory_metrics.starting_capital ?? 0) ? '#28a745' : '#dc3545' }}>
                        ${formatCurrency(result.inventory_metrics.final_equity ?? 0)}
                      </span>
                    </div>
                  </div>
                </div>
              </div>
              
              {/* P&L Summary */}
              <div style={{
                backgroundColor: (result.total_pnl ?? 0) >= 0 ? 'rgba(40, 167, 69, 0.1)' : 'rgba(220, 53, 69, 0.1)',
                padding: '15px',
                borderRadius: '6px',
                border: `1px solid ${(result.total_pnl ?? 0) >= 0 ? '#28a745' : '#dc3545'}`,
                textAlign: 'center',
              }}>
                <span style={{ fontSize: '14px', color: '#6c757d' }}>Net P&L (After All Fees): </span>
                <span style={{ fontSize: '18px', fontWeight: 700, color: (result.total_pnl ?? 0) >= 0 ? '#28a745' : '#dc3545' }}>
                  {(result.total_pnl ?? 0) >= 0 ? '+' : ''}${formatCurrency(result.total_pnl ?? 0)}
                </span>
                <span style={{ fontSize: '14px', color: '#6c757d', marginLeft: '10px' }}>
                  ({safeToFixed(((result.inventory_metrics.final_equity ?? 0) / (result.inventory_metrics.starting_capital ?? 1) - 1) * 100, 2)}%)
                </span>
              </div>
            </>
          )}
        </div>
      )}

      {/* Comprehensive Execution Metrics */}
      {result.execution_metrics && (
        <div style={{
          backgroundColor: 'white',
          padding: '20px',
          borderRadius: '8px',
          boxShadow: '0 2px 4px rgba(0,0,0,0.1)',
          marginBottom: '20px',
        }}>
          <h3 style={{ marginTop: 0, marginBottom: '15px' }}>⚡ Execution Quality Metrics</h3>
          
          {/* Order Lifecycle */}
          <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>Order Lifecycle</h4>
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))',
            gap: '15px',
            marginBottom: '20px',
          }}>
            <MetricCard
              title="Orders Placed"
              value={(result.execution_metrics.orders_placed ?? 0).toString()}
              color="#007bff"
            />
            <MetricCard
              title="Orders Filled"
              value={(result.execution_metrics.orders_filled ?? 0).toString()}
              color="#28a745"
            />
            <MetricCard
              title="Orders Cancelled"
              value={(result.execution_metrics.orders_cancelled ?? 0).toString()}
              color={(result.execution_metrics.orders_cancelled ?? 0) === 0 ? '#28a745' : '#ffc107'}
            />
            <MetricCard
              title="Total Fills"
              value={(result.execution_metrics.total_fills ?? 0).toString()}
              color="#17a2b8"
            />
            <MetricCard
              title="Fill Rate"
              value={`${safeToFixed(result.execution_metrics.fill_rate * 100, 1)}%`}
              color={result.execution_metrics.fill_rate > 0.8 ? '#28a745' : result.execution_metrics.fill_rate > 0.5 ? '#ffc107' : '#dc3545'}
            />
            <MetricCard
              title="Cancel Rate"
              value={`${safeToFixed(result.execution_metrics.cancellation_rate * 100, 1)}%`}
              color={result.execution_metrics.cancellation_rate < 0.2 ? '#28a745' : result.execution_metrics.cancellation_rate < 0.5 ? '#ffc107' : '#dc3545'}
            />
          </div>
          
          {/* Timing Metrics */}
          <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>Timing & Speed</h4>
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))',
            gap: '15px',
            marginBottom: '20px',
          }}>
            <MetricCard
              title="Avg Fills/Order"
              value={safeToFixed(result.execution_metrics.avg_fills_per_order, 2)}
              color="#6f42c1"
            />
            <MetricCard
              title="Avg Time to Fill"
              value={`${safeToFixed(result.execution_metrics.avg_time_to_fill_seconds, 1)}s`}
              color={result.execution_metrics.avg_time_to_fill_seconds < 10 ? '#28a745' : result.execution_metrics.avg_time_to_fill_seconds < 60 ? '#ffc107' : '#dc3545'}
            />
            <MetricCard
              title="Median Time to Fill"
              value={`${safeToFixed(result.execution_metrics.median_time_to_fill_seconds, 1)}s`}
              color={result.execution_metrics.median_time_to_fill_seconds < 5 ? '#28a745' : result.execution_metrics.median_time_to_fill_seconds < 30 ? '#ffc107' : '#dc3545'}
            />
            <MetricCard
              title="Avg Quote Spread"
              value={`${safeToFixed(result.execution_metrics.avg_quote_spread_bps, 1)} bps`}
              color="#17a2b8"
            />
          </div>
          
          {/* Bid/Ask Breakdown */}
          <h4 style={{ margin: '10px 0', fontSize: '14px', color: '#666' }}>Bid/Ask Breakdown</h4>
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))',
            gap: '15px',
          }}>
            <MetricCard
              title="Bid Orders"
              value={(result.execution_metrics.bid_orders ?? 0).toString()}
              color="#28a745"
            />
            <MetricCard
              title="Ask Orders"
              value={(result.execution_metrics.ask_orders ?? 0).toString()}
              color="#dc3545"
            />
            <MetricCard
              title="Bid Fills"
              value={(result.execution_metrics.bid_fills ?? 0).toString()}
              color="#28a745"
            />
            <MetricCard
              title="Ask Fills"
              value={(result.execution_metrics.ask_fills ?? 0).toString()}
              color="#dc3545"
            />
            <MetricCard
              title="Bid Fill Rate"
              value={`${safeToFixed(result.execution_metrics.bid_fill_rate * 100, 1)}%`}
              color={result.execution_metrics.bid_fill_rate > 0.7 ? '#28a745' : '#ffc107'}
            />
            <MetricCard
              title="Ask Fill Rate"
              value={`${safeToFixed(result.execution_metrics.ask_fill_rate * 100, 1)}%`}
              color={result.execution_metrics.ask_fill_rate > 0.7 ? '#28a745' : '#ffc107'}
            />
          </div>
          
          {/* Market-Making Win Rate Metrics (Round-Trip Analysis) */}
          {result.execution_metrics.round_trip_win_rate !== undefined && (
            <>
              <h4 style={{ margin: '20px 0 10px 0', fontSize: '14px', color: '#666' }}>🎯 Round-Trip Win Rate (Market Making)</h4>
              <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))',
                gap: '15px',
              }}>
                <MetricCard
                  title="Round-Trip Win Rate"
                  value={`${safeToFixed((result.execution_metrics.round_trip_win_rate ?? 0) * 100, 1)}%`}
                  color={(result.execution_metrics.round_trip_win_rate ?? 0) > 0.6 ? '#28a745' : (result.execution_metrics.round_trip_win_rate ?? 0) > 0.5 ? '#ffc107' : '#dc3545'}
                />
                <MetricCard
                  title="Total Round-Trips"
                  value={(result.execution_metrics.total_round_trips ?? 0).toString()}
                  color="#007bff"
                />
                <MetricCard
                  title="Profitable RTs"
                  value={(result.execution_metrics.profitable_round_trips ?? 0).toString()}
                  color="#28a745"
                />
                {result.execution_metrics.avg_pnl_per_round_trip !== undefined && (
                  <MetricCard
                    title="Avg P&L per RT"
                    value={`$${safeToFixed(result.execution_metrics.avg_pnl_per_round_trip, 4)}`}
                    color={result.execution_metrics.avg_pnl_per_round_trip >= 0 ? '#28a745' : '#dc3545'}
                  />
                )}
                {result.execution_metrics.spread_capture_rate !== undefined && (
                  <MetricCard
                    title="Spread Capture"
                    value={`${safeToFixed(result.execution_metrics.spread_capture_rate * 100, 1)}%`}
                    color={result.execution_metrics.spread_capture_rate > 0.7 ? '#28a745' : result.execution_metrics.spread_capture_rate > 0.5 ? '#ffc107' : '#dc3545'}
                  />
                )}
                {result.execution_metrics.adverse_selection_rate !== undefined && (
                  <MetricCard
                    title="Adverse Selection"
                    value={`${safeToFixed(result.execution_metrics.adverse_selection_rate * 100, 1)}%`}
                    color={result.execution_metrics.adverse_selection_rate < 0.3 ? '#28a745' : result.execution_metrics.adverse_selection_rate < 0.5 ? '#ffc107' : '#dc3545'}
                  />
                )}
              </div>
            </>
          )}
        </div>
      )}

      {/* Trade Breakdown (for Market Making) */}
      {(result.closed_trades !== undefined || result.open_positions !== undefined) && (
        <div style={{
          backgroundColor: 'white',
          padding: '20px',
          borderRadius: '8px',
          boxShadow: '0 2px 4px rgba(0,0,0,0.1)',
          marginBottom: '20px',
        }}>
          <h3 style={{ marginTop: 0, marginBottom: '15px' }}>📊 Trade Breakdown</h3>
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
            gap: '15px',
          }}>
            {result.closed_trades !== undefined && result.closed_trades !== null && (
              <MetricCard
                title="Closed Trades"
                value={result.closed_trades.toString()}
                color="#28a745"
              />
            )}
            {result.open_positions !== undefined && result.open_positions !== null && (
              <MetricCard
                title="Open Positions"
                value={result.open_positions.toString()}
                color={result.open_positions === 0 ? '#28a745' : '#ffc107'}
              />
            )}
            {result.realized_pnl !== undefined && (
              <MetricCard
                title="Realized P&L"
                value={`$${safeToFixed(result.realized_pnl, 2)}`}
                color={result.realized_pnl >= 0 ? '#28a745' : '#dc3545'}
              />
            )}
            {result.unrealized_pnl !== undefined && (
              <MetricCard
                title="Unrealized P&L"
                value={`$${safeToFixed(result.unrealized_pnl, 2)}`}
                color={result.unrealized_pnl >= 0 ? '#28a745' : '#dc3545'}
              />
            )}
          </div>
        </div>
      )}

      {/* Risk Halt Warning */}
      {result.risk_halt_reason && (
        <div style={{
          backgroundColor: '#fff3cd',
          padding: '15px 20px',
          borderRadius: '8px',
          border: '1px solid #ffc107',
          marginBottom: '20px',
        }}>
          <h4 style={{ margin: '0 0 10px 0', color: '#856404' }}>⚠️ Risk Management Alert</h4>
          <p style={{ margin: 0, color: '#856404' }}>{result.risk_halt_reason}</p>
        </div>
      )}

      {/* Equity Curve Chart */}
      <div style={{
        backgroundColor: 'white',
        padding: '20px',
        borderRadius: '8px',
        boxShadow: '0 2px 4px rgba(0,0,0,0.1)',
        marginBottom: '30px',
      }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '15px' }}>
          <h3 style={{ margin: 0 }}>Equity Curve</h3>
          <div style={{ display: 'flex', gap: '20px', fontSize: '14px' }}>
            <div>
              <span style={{ color: '#666' }}>Starting Capital: </span>
              <span style={{ fontWeight: 600, color: '#007bff' }}>
                ${formatLargeNumber(jobDetails?.initial_capital ?? 10000)}
              </span>
            </div>
            <div>
              <span style={{ color: '#666' }}>Ending Capital: </span>
              <span style={{ fontWeight: 600, color: (result.total_pnl ?? 0) >= 0 ? '#28a745' : '#dc3545' }}>
                ${formatLargeNumber((jobDetails?.initial_capital ?? 10000) + (result.total_pnl ?? 0))}
              </span>
            </div>
            <div>
              <span style={{ color: '#666' }}>Return: </span>
              <span style={{ fontWeight: 600, color: (result.total_pnl ?? 0) >= 0 ? '#28a745' : '#dc3545' }}>
                {safeToFixed(((result.total_pnl ?? 0) / (jobDetails?.initial_capital ?? 10000)) * 100, 2)}%
              </span>
            </div>
          </div>
        </div>
        <EquityCurveChart 
          data={result.equity_curve ?? []} 
          startDate={result.start_date}
          endDate={result.end_date}
        />
      </div>

      {/* Strategy Metrics */}
      {result.strategy_metrics && (
        <div style={{
          backgroundColor: 'white',
          padding: '20px',
          borderRadius: '8px',
          boxShadow: '0 2px 4px rgba(0,0,0,0.1)',
        }}>
          <h3 style={{ marginTop: 0 }}>Strategy Metrics</h3>
          <pre style={{
            backgroundColor: '#f8f9fa',
            padding: '15px',
            borderRadius: '4px',
            overflow: 'auto',
            fontSize: '12px',
          }}>
            {JSON.stringify(result.strategy_metrics, null, 2)}
          </pre>
        </div>
      )}
    </div>
  );
};

interface MetricCardProps {
  title: string;
  value: string;
  color: string;
  tooltip?: string;
}

const MetricCard: React.FC<MetricCardProps> = ({ title, value, color, tooltip }) => {
  const [showTooltip, setShowTooltip] = React.useState(false);
  
  return (
    <div style={{
      backgroundColor: 'white',
      padding: '20px',
      borderRadius: '8px',
      boxShadow: '0 2px 4px rgba(0,0,0,0.1)',
      textAlign: 'center',
      position: 'relative',
    }}>
      <div style={{ fontSize: '14px', color: '#666', marginBottom: '8px', display: 'flex', alignItems: 'center', justifyContent: 'center', gap: '6px' }}>
        {title}
        {tooltip && (
          <span
            onMouseEnter={() => setShowTooltip(true)}
            onMouseLeave={() => setShowTooltip(false)}
            style={{
              display: 'inline-flex',
              alignItems: 'center',
              justifyContent: 'center',
              width: '16px',
              height: '16px',
              borderRadius: '50%',
              backgroundColor: '#e9ecef',
              color: '#6c757d',
              fontSize: '11px',
              fontWeight: 600,
              cursor: 'help',
              position: 'relative',
            }}
          >
            ?
            {showTooltip && (
              <div style={{
                position: 'absolute',
                bottom: '100%',
                left: '50%',
                transform: 'translateX(-50%)',
                marginBottom: '8px',
                padding: '10px 12px',
                backgroundColor: '#333',
                color: 'white',
                borderRadius: '6px',
                fontSize: '12px',
                lineHeight: '1.4',
                whiteSpace: 'normal',
                width: '220px',
                textAlign: 'left',
                zIndex: 1000,
                boxShadow: '0 4px 12px rgba(0,0,0,0.3)',
              }}>
                {tooltip}
                <div style={{
                  position: 'absolute',
                  top: '100%',
                  left: '50%',
                  transform: 'translateX(-50%)',
                  borderLeft: '6px solid transparent',
                  borderRight: '6px solid transparent',
                  borderTop: '6px solid #333',
                }} />
              </div>
            )}
          </span>
        )}
      </div>
      <div style={{ fontSize: '28px', fontWeight: 'bold', color }}>
        {value}
      </div>
    </div>
  );
};

interface EquityCurveChartProps {
  data: Array<number | { timestamp?: string; value?: number }>;
  startDate?: string;
  endDate?: string;
}

const EquityCurveChart: React.FC<EquityCurveChartProps> = ({ data, startDate, endDate }) => {
  if (!data || data.length === 0) {
    return <div style={{ padding: '20px', textAlign: 'center', color: '#666' }}>No data available</div>;
  }

  // Helper to get the equity value (supports numbers and objects)
  const getValue = (d: number | { timestamp?: string; value?: number }): number => {
    if (typeof d === 'number') return d;
    return d.value ?? 0;
  };

  // Calculate intermediate date labels based on start and end dates
  const getDateAtRatio = (ratio: number): string => {
    if (!startDate || !endDate) return `${Math.round(ratio * 100)}%`;
    try {
      const start = new Date(startDate).getTime();
      const end = new Date(endDate).getTime();
      const targetTime = start + (end - start) * ratio;
      const targetDate = new Date(targetTime);
      return targetDate.toLocaleDateString('en-US', { 
        month: 'short', 
        day: 'numeric'
      });
    } catch {
      return `${Math.round(ratio * 100)}%`;
    }
  };

  // Simple SVG line chart implementation
  const width = 900;
  const height = 400;
  const padding = 60;
  
  const values = data.map(getValue).filter(v => !isNaN(v) && isFinite(v));
  
  if (values.length === 0) {
    return <div style={{ padding: '20px', textAlign: 'center', color: '#666' }}>No valid data available</div>;
  }
  
  const minValue = Math.min(...values);
  const maxValue = Math.max(...values);
  const valueRange = maxValue - minValue || 1;

  const xScale = (index: number) => padding + (index / (values.length - 1 || 1)) * (width - 2 * padding);
  const yScale = (value: number) => height - padding - ((value - minValue) / valueRange) * (height - 2 * padding);

  // Format currency for Y-axis labels
  const formatCurrency = (value: number): string => {
    if (Math.abs(value) >= 1000000) {
      return `$${(value / 1000000).toFixed(1)}M`;
    } else if (Math.abs(value) >= 1000) {
      return `$${(value / 1000).toFixed(1)}K`;
    }
    return `$${value.toFixed(0)}`;
  };

  // Generate path from values
  const pathData = values
    .map((value, index) => {
      const x = xScale(index);
      const y = yScale(value);
      return index === 0 ? `M ${x} ${y}` : `L ${x} ${y}`;
    })
    .join(' ');

  return (
    <div style={{ width: '100%', overflow: 'auto' }}>
      <svg width={width} height={height} style={{ display: 'block' }}>
        {/* Grid lines */}
        <line x1={padding} y1={padding} x2={padding} y2={height - padding} stroke="#dee2e6" strokeWidth={1} />
        <line x1={padding} y1={height - padding} x2={width - padding} y2={height - padding} stroke="#dee2e6" strokeWidth={1} />
        
        {/* Horizontal grid lines and Y-axis labels */}
        {[0, 0.25, 0.5, 0.75, 1].map((ratio) => {
          const y = height - padding - ratio * (height - 2 * padding);
          const value = minValue + ratio * valueRange;
          return (
            <g key={ratio}>
              <line x1={padding} y1={y} x2={width - padding} y2={y} stroke="#e9ecef" strokeWidth={1} />
              <text x={padding - 5} y={y + 4} textAnchor="end" fontSize="11" fill="#666">
                {formatCurrency(value)}
              </text>
            </g>
          );
        })}
        
        {/* Vertical grid lines and X-axis labels */}
        {[0, 0.25, 0.5, 0.75, 1].map((ratio, i) => {
          const x = padding + ratio * (width - 2 * padding);
          return (
            <g key={`x-${i}`}>
              <line x1={x} y1={padding} x2={x} y2={height - padding} stroke="#e9ecef" strokeWidth={1} />
              <text x={x} y={height - padding + 15} textAnchor="middle" fontSize="11" fill="#666">
                {getDateAtRatio(ratio)}
              </text>
            </g>
          );
        })}
        
        {/* Equity curve line */}
        <path d={pathData} fill="none" stroke="#007bff" strokeWidth={2} />
        
        {/* Axis labels */}
        <text x={width / 2} y={height - 5} textAnchor="middle" fontSize="12" fill="#333">
          {startDate && endDate ? 'Date' : 'Time Progress'}
        </text>
        <text x={15} y={height / 2} textAnchor="middle" fontSize="12" fill="#333" transform={`rotate(-90, 15, ${height / 2})`}>
          Portfolio Value
        </text>
      </svg>
    </div>
  );
};

export default ResultsVisualization;
