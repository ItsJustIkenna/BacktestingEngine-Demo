import React, { useState, useEffect, useCallback } from 'react';

interface MonteCarloSummary {
  num_runs: number;
  mean_sharpe: number;
  stddev_sharpe: number;
  var_95: number;
  cvar_95: number;
  confidence_lower: number;
  confidence_upper: number;
}

interface WalkForwardSummary {
  periods_tested: number;
  average_performance: number;
  consistency_score: number;
  out_of_sample_degradation: number;
}

interface BacktestResult {
  job_id?: string;
  symbol?: string;
  exchange?: string;
  sharpe_ratio?: number;
  max_drawdown?: number;
  total_pnl?: number;
  win_rate?: number;
  profit_factor?: number;
  total_trades?: number;
  avg_trade_duration?: number;
  equity_curve?: Array<number | { timestamp?: string; value?: number }>;
  strategy_metrics?: any;
  monte_carlo?: MonteCarloSummary;
  walk_forward?: WalkForwardSummary;
  
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
  
  // Execution metrics
  total_orders?: number;
  filled_orders?: number;
  cancelled_orders?: number;
  avg_slippage?: number;
  total_commission_paid?: number;
  
  // Strategy parameters
  strategy_params?: any;
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

export const ResultsVisualization: React.FC<ResultsVisualizationProps> = ({ 
  jobId, 
  onBack 
}) => {
  const [result, setResult] = useState<BacktestResult | null>(null);
  const [jobStatus, setJobStatus] = useState<JobStatus | null>(null);
  const [jobDetails, setJobDetails] = useState<JobDetails | null>(null);
  const [loading, setLoading] = useState(true); // Start with loading true for initial load
  const [error, setError] = useState<string | null>(null);
  const [estimatedTimeRemaining, setEstimatedTimeRemaining] = useState<number | null>(null);
  const [countdown, setCountdown] = useState<number | null>(null);
  const [displayProgress, setDisplayProgress] = useState<number>(0);
  const [copied, setCopied] = useState(false);
  const [actionLoading, setActionLoading] = useState<'cancel' | 'delete' | null>(null);
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

  const fetchData = useCallback(async () => {
    // Only show loading spinner on first load to avoid page flicker
    if (isFirstLoad.current) {
      setLoading(true);
    }
    setError(null);

    try {
      // First fetch job status
      const statusResponse = await fetch(`/api/backtest/${jobId}/status`);
      const statusData = await statusResponse.json();
      console.log(`[FetchData] Job ${jobId} status: ${statusData.status}, progress: ${statusData.progress}%`);
      setJobStatus(statusData);

      // Calculate estimated time remaining for running jobs
      if (statusData.status === 'running' && statusData.started_at && statusData.progress > 0) {
        const startTime = new Date(statusData.started_at).getTime();
        const now = Date.now();
        const elapsedMs = now - startTime;
        const progressFraction = statusData.progress / 100;
        const estimatedTotalMs = elapsedMs / progressFraction;
        const remainingMs = estimatedTotalMs - elapsedMs;
        setEstimatedTimeRemaining(Math.max(0, remainingMs));
      } else {
        setEstimatedTimeRemaining(null);
      }

      // Mark first load as complete to stop showing loading spinner on subsequent polls
      isFirstLoad.current = false;

      // If completed, fetch results
      if (statusData.status === 'completed') {
        const resultsResponse = await fetch(`/api/backtest/${jobId}/results`);
        const resultsData = await resultsResponse.json();
        setResult(resultsData);
        setLoading(false);
      } else if (statusData.status === 'running' || statusData.status === 'queued') {
        // Job still in progress, auto-retry after 3 seconds
        setLoading(false);
        setTimeout(fetchData, 3000);
      } else if (statusData.status === 'failed') {
        setError(statusData.error_message || 'Job failed');
        setLoading(false);
      } else {
        setLoading(false);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'An error occurred');
      setLoading(false);
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

  // Countdown timer that decrements every second
  useEffect(() => {
    if (estimatedTimeRemaining !== null && estimatedTimeRemaining > 0) {
      setCountdown(estimatedTimeRemaining);
      
      const interval = setInterval(() => {
        setCountdown((prev) => {
          if (prev === null || prev <= 1000) {
            return 0;
          }
          return prev - 1000;
        });
      }, 1000);

      return () => clearInterval(interval);
    }
  }, [estimatedTimeRemaining]);

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
      content = JSON.stringify(result, null, 2);
      filename = `backtest_${jobId.substring(0, 8)}.json`;
      mimeType = 'application/json';
    } else {
      // CSV format
      const headers = [
        'Job ID', 'Symbol', 'Exchange', 'Sharpe Ratio', 'Max Drawdown (%)',
        'Total P&L', 'Win Rate (%)', 'Profit Factor', 'Total Trades', 'Avg Trade Duration (s)'
      ];
      
      const row = [
        result.job_id ?? 'N/A',
        result.symbol ?? 'N/A',
        result.exchange ?? 'N/A',
        safeToFixed(result.sharpe_ratio, 4),
        safeToFixed(result.max_drawdown, 2),
        safeToFixed(result.total_pnl, 2),
        safeToFixed(result.win_rate, 2),
        safeToFixed(result.profit_factor, 2),
        (result.total_trades ?? 0).toString(),
        safeToFixed(result.avg_trade_duration, 2),
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

        {/* Job Details Card */}
        {jobDetails && (
          <div style={{
            padding: '20px',
            backgroundColor: 'white',
            border: '1px solid #dee2e6',
            borderRadius: '8px',
            marginBottom: '20px',
            boxShadow: '0 2px 4px rgba(0,0,0,0.05)',
          }}>
            <h3 style={{ marginTop: 0, marginBottom: '15px' }}>Backtest Configuration</h3>
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '12px', fontSize: '14px' }}>
              <div>
                <strong>Symbol:</strong> {jobDetails.symbol}
              </div>
              <div>
                <strong>Exchange:</strong> {jobDetails.exchange}
              </div>
              <div>
                <strong>Strategy:</strong> {jobDetails.strategy}
              </div>
              <div>
                <strong>Initial Capital:</strong> ${jobDetails.initial_capital.toLocaleString()}
              </div>
              <div>
                <strong>Optimization:</strong> {jobDetails.optimization_method}
              </div>
              {jobDetails.optimization_method === 'genetic' && jobDetails.population_size && (
                <>
                  <div>
                    <strong>Population Size:</strong> {jobDetails.population_size}
                  </div>
                  <div>
                    <strong>Generations:</strong> {jobDetails.generations}
                  </div>
                  <div style={{ gridColumn: '1 / -1', marginTop: '5px', padding: '10px', backgroundColor: '#f8f9fa', borderRadius: '4px' }}>
                    <strong>Total Simulations:</strong> {jobDetails.population_size! * jobDetails.generations!} backtests
                  </div>
                </>
              )}
            </div>
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
                  {jobStatus.current_phase === 'genetic_optimization' && '🧬 Genetic Optimization'}
                  {jobStatus.current_phase === 'monte_carlo' && '🎲 Monte Carlo Simulation'}
                  {jobStatus.current_phase === 'walk_forward' && '🔄 Walk-Forward Analysis'}
                  {jobStatus.current_phase === 'final_analysis' && '📊 Final Analysis'}
                  {jobStatus.current_phase === 'completed' && '✅ Completed'}
                  {jobStatus.current_phase === 'failed' && '❌ Failed'}
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
            {countdown !== null && countdown > 0 && jobStatus.status === 'running' && (
              <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '5px' }}>
                <strong>Time Remaining:</strong>
                <span style={{ color: '#007bff', fontWeight: 'bold', fontSize: '16px' }}>
                  ⏱️ {formatTime(countdown)}
                </span>
              </div>
            )}
            {countdown !== null && countdown <= 0 && jobStatus.status === 'running' && (
              <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '5px' }}>
                <strong>Time Remaining:</strong>
                <span style={{ color: '#28a745', fontWeight: 'bold' }}>
                  Completing soon...
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
              <div><strong>Started:</strong> {new Date(jobStatus.started_at).toLocaleString()}</div>
            )}
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
        />
        <MetricCard
          title="Max Drawdown"
          value={`${safeToFixed(result.max_drawdown, 2)}%`}
          color={(result.max_drawdown ?? 0) < 10 ? '#28a745' : (result.max_drawdown ?? 0) < 20 ? '#ffc107' : '#dc3545'}
        />
        <MetricCard
          title="Total P&L"
          value={safeToFixed(result.total_pnl, 2)}
          color={(result.total_pnl ?? 0) >= 0 ? '#28a745' : '#dc3545'}
        />
        <MetricCard
          title="Win Rate"
          value={`${safeToFixed(result.win_rate, 2)}%`}
          color={(result.win_rate ?? 0) > 50 ? '#28a745' : '#ffc107'}
        />
        <MetricCard
          title="Profit Factor"
          value={safeToFixed(result.profit_factor, 2)}
          color={(result.profit_factor ?? 0) > 1.5 ? '#28a745' : (result.profit_factor ?? 0) > 1 ? '#ffc107' : '#dc3545'}
        />
        <MetricCard
          title="Total Trades"
          value={(result.total_trades ?? 0).toString()}
          color="#007bff"
        />
      </div>

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
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
            gap: '15px',
          }}>
            <MetricCard
              title="Mean Sharpe"
              value={safeToFixed(result.monte_carlo.mean_sharpe, 3)}
              color={result.monte_carlo.mean_sharpe > 1 ? '#28a745' : result.monte_carlo.mean_sharpe > 0 ? '#ffc107' : '#dc3545'}
            />
            <MetricCard
              title="Sharpe Std Dev"
              value={safeToFixed(result.monte_carlo.stddev_sharpe, 3)}
              color={result.monte_carlo.stddev_sharpe < 0.5 ? '#28a745' : '#ffc107'}
            />
            <MetricCard
              title="VaR (95%)"
              value={`${safeToFixed(result.monte_carlo.var_95 * 100, 2)}%`}
              color={result.monte_carlo.var_95 < 0.1 ? '#28a745' : result.monte_carlo.var_95 < 0.2 ? '#ffc107' : '#dc3545'}
            />
            <MetricCard
              title="CVaR (95%)"
              value={`${safeToFixed(result.monte_carlo.cvar_95 * 100, 2)}%`}
              color={result.monte_carlo.cvar_95 < 0.15 ? '#28a745' : result.monte_carlo.cvar_95 < 0.25 ? '#ffc107' : '#dc3545'}
            />
            <MetricCard
              title="95% Confidence"
              value={`[${safeToFixed(result.monte_carlo.confidence_lower, 2)}, ${safeToFixed(result.monte_carlo.confidence_upper, 2)}]`}
              color="#007bff"
            />
          </div>
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
            />
            <MetricCard
              title="Consistency"
              value={`${safeToFixed(result.walk_forward.consistency_score * 100, 1)}%`}
              color={result.walk_forward.consistency_score > 0.7 ? '#28a745' : result.walk_forward.consistency_score > 0.5 ? '#ffc107' : '#dc3545'}
            />
            <MetricCard
              title="OOS Degradation"
              value={`${safeToFixed(result.walk_forward.out_of_sample_degradation * 100, 1)}%`}
              color={result.walk_forward.out_of_sample_degradation < 0.2 ? '#28a745' : result.walk_forward.out_of_sample_degradation < 0.4 ? '#ffc107' : '#dc3545'}
            />
          </div>
        </div>
      )}

      {/* Trade Analysis */}
      {(result.avg_trade_return !== undefined || result.best_trade !== undefined || result.avg_time_in_trade !== undefined) && (
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
            {result.avg_trade_return !== undefined && (
              <MetricCard
                title="Avg Trade Return"
                value={`${safeToFixed(result.avg_trade_return, 2)}%`}
                color={result.avg_trade_return > 0 ? '#28a745' : '#dc3545'}
              />
            )}
            {result.best_trade !== undefined && (
              <MetricCard
                title="Best Trade"
                value={safeToFixed(result.best_trade, 2)}
                color="#28a745"
              />
            )}
            {result.worst_trade !== undefined && (
              <MetricCard
                title="Worst Trade"
                value={safeToFixed(result.worst_trade, 2)}
                color="#dc3545"
              />
            )}
            {result.avg_time_in_trade !== undefined && (
              <MetricCard
                title="Avg Time in Trade"
                value={`${safeToFixed(result.avg_time_in_trade, 1)}h`}
                color="#007bff"
              />
            )}
            {result.volatility !== undefined && (
              <MetricCard
                title="Volatility"
                value={`${safeToFixed(result.volatility, 2)}%`}
                color={result.volatility < 20 ? '#28a745' : result.volatility < 40 ? '#ffc107' : '#dc3545'}
              />
            )}
            {result.sortino_ratio !== undefined && (
              <MetricCard
                title="Sortino Ratio"
                value={safeToFixed(result.sortino_ratio, 2)}
                color={result.sortino_ratio > 1.5 ? '#28a745' : result.sortino_ratio > 1 ? '#ffc107' : '#dc3545'}
              />
            )}
            {result.calmar_ratio !== undefined && (
              <MetricCard
                title="Calmar Ratio"
                value={safeToFixed(result.calmar_ratio, 2)}
                color={result.calmar_ratio > 1 ? '#28a745' : result.calmar_ratio > 0.5 ? '#ffc107' : '#dc3545'}
              />
            )}
          </div>
        </div>
      )}

      {/* Risk Metrics */}
      {(result.value_at_risk_95 !== undefined || result.current_drawdown !== undefined || result.max_drawdown_duration_days !== undefined) && (
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
                value={`${safeToFixed(result.value_at_risk_95 * 100, 2)}%`}
                color={result.value_at_risk_95 < 0.1 ? '#28a745' : result.value_at_risk_95 < 0.2 ? '#ffc107' : '#dc3545'}
              />
            )}
            {result.expected_shortfall_95 !== undefined && (
              <MetricCard
                title="Expected Shortfall"
                value={`${safeToFixed(result.expected_shortfall_95 * 100, 2)}%`}
                color={result.expected_shortfall_95 < 0.15 ? '#28a745' : result.expected_shortfall_95 < 0.25 ? '#ffc107' : '#dc3545'}
              />
            )}
            {result.current_drawdown !== undefined && (
              <MetricCard
                title="Current Drawdown"
                value={`${safeToFixed(result.current_drawdown, 2)}%`}
                color={result.current_drawdown < 5 ? '#28a745' : result.current_drawdown < 15 ? '#ffc107' : '#dc3545'}
              />
            )}
            {result.avg_drawdown !== undefined && (
              <MetricCard
                title="Avg Drawdown"
                value={`${safeToFixed(result.avg_drawdown, 2)}%`}
                color={result.avg_drawdown < 5 ? '#28a745' : result.avg_drawdown < 10 ? '#ffc107' : '#dc3545'}
              />
            )}
            {result.max_drawdown_duration_days !== undefined && (
              <MetricCard
                title="Max DD Duration"
                value={`${result.max_drawdown_duration_days} days`}
                color={result.max_drawdown_duration_days < 30 ? '#28a745' : result.max_drawdown_duration_days < 90 ? '#ffc107' : '#dc3545'}
              />
            )}
          </div>
        </div>
      )}

      {/* Execution Metrics */}
      {(result.total_orders !== undefined || result.avg_slippage !== undefined || result.total_commission_paid !== undefined) && (
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
              />
            )}
            {result.filled_orders !== undefined && (
              <MetricCard
                title="Filled Orders"
                value={result.filled_orders.toString()}
                color="#28a745"
              />
            )}
            {result.cancelled_orders !== undefined && (
              <MetricCard
                title="Cancelled"
                value={result.cancelled_orders.toString()}
                color={result.cancelled_orders === 0 ? '#28a745' : '#ffc107'}
              />
            )}
            {result.avg_slippage !== undefined && (
              <MetricCard
                title="Avg Slippage"
                value={`${safeToFixed(result.avg_slippage * 100, 3)}%`}
                color={result.avg_slippage < 0.001 ? '#28a745' : result.avg_slippage < 0.005 ? '#ffc107' : '#dc3545'}
              />
            )}
            {result.total_commission_paid !== undefined && (
              <MetricCard
                title="Total Commission"
                value={`$${safeToFixed(result.total_commission_paid, 2)}`}
                color="#6c757d"
              />
            )}
            {result.filled_orders !== undefined && result.total_orders !== undefined && result.total_orders > 0 && (
              <MetricCard
                title="Fill Rate"
                value={`${safeToFixed((result.filled_orders / result.total_orders) * 100, 1)}%`}
                color={result.filled_orders / result.total_orders > 0.95 ? '#28a745' : result.filled_orders / result.total_orders > 0.8 ? '#ffc107' : '#dc3545'}
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
          <h3 style={{ marginTop: 0, marginBottom: '15px' }}>🎯 Optimized Strategy Parameters</h3>
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))',
            gap: '15px',
          }}>
            {result.strategy_params.risk_aversion !== undefined && (
              <MetricCard
                title="Risk Aversion (γ)"
                value={safeToFixed(parseFloat(result.strategy_params.risk_aversion), 4)}
                color="#6f42c1"
              />
            )}
            {result.strategy_params.inventory_target !== undefined && (
              <MetricCard
                title="Inventory Target"
                value={safeToFixed(parseFloat(result.strategy_params.inventory_target), 4)}
                color="#6f42c1"
              />
            )}
            {result.strategy_params.order_size !== undefined && (
              <MetricCard
                title="Order Size"
                value={safeToFixed(parseFloat(result.strategy_params.order_size), 4)}
                color="#6f42c1"
              />
            )}
          </div>
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
        <h3 style={{ marginTop: 0 }}>Equity Curve</h3>
        <EquityCurveChart data={result.equity_curve ?? []} />
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
}

const MetricCard: React.FC<MetricCardProps> = ({ title, value, color }) => (
  <div style={{
    backgroundColor: 'white',
    padding: '20px',
    borderRadius: '8px',
    boxShadow: '0 2px 4px rgba(0,0,0,0.1)',
    textAlign: 'center',
  }}>
    <div style={{ fontSize: '14px', color: '#666', marginBottom: '8px' }}>
      {title}
    </div>
    <div style={{ fontSize: '28px', fontWeight: 'bold', color }}>
      {value}
    </div>
  </div>
);

interface EquityCurveChartProps {
  data: Array<number | { timestamp?: string; value?: number }>;
}

const EquityCurveChart: React.FC<EquityCurveChartProps> = ({ data }) => {
  if (!data || data.length === 0) {
    return <div style={{ padding: '20px', textAlign: 'center', color: '#666' }}>No data available</div>;
  }

  // Helper to get the equity value (supports numbers and objects)
  const getValue = (d: number | { timestamp?: string; value?: number }): number => {
    if (typeof d === 'number') return d;
    return d.value ?? 0;
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
                {`${Math.round(ratio * 100)}%`}
              </text>
            </g>
          );
        })}
        
        {/* Equity curve line */}
        <path d={pathData} fill="none" stroke="#007bff" strokeWidth={2} />
        
        {/* Axis labels */}
        <text x={width / 2} y={height - 5} textAnchor="middle" fontSize="12" fill="#333">
          Time Progress
        </text>
        <text x={15} y={height / 2} textAnchor="middle" fontSize="12" fill="#333" transform={`rotate(-90, 15, ${height / 2})`}>
          Portfolio Value
        </text>
      </svg>
    </div>
  );
};

export default ResultsVisualization;
