import React, { useState, useEffect, useCallback } from 'react';

interface PortfolioAsset {
  symbol: string;
  exchange: string;
  weight?: number;
}

interface BacktestJob {
  job_id: string;
  symbol: string;
  exchange: string;
  portfolio_assets?: PortfolioAsset[];
  status: string;
  progress: number;
  created_at: string;
  started_at?: string;
  completed_at?: string;
  sharpe_ratio?: number;
  max_drawdown?: number;
  total_pnl?: number;
  win_rate?: number;
  current_generation?: number;
  total_generations?: number;
  current_phase?: string;
  phase_details?: {
    current_generation?: number;
    total_generations?: number;
    best_fitness?: number;
    chromosomes_evaluated?: number;
    total_chromosomes?: number;
    elapsed_time?: string;
    stage?: string;
  };
  error_message?: string;
}

interface JobListProps {
  onJobClick?: (jobId: string) => void;
  refreshInterval?: number; // milliseconds
}

export const JobList: React.FC<JobListProps> = ({ 
  onJobClick, 
  refreshInterval = 5000 
}) => {
  const [jobs, setJobs] = useState<BacktestJob[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [cancelConfirm, setCancelConfirm] = useState<string | null>(null); // job_id to cancel
  const [cancelling, setCancelling] = useState<string | null>(null); // job_id being cancelled
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null); // job_id to delete
  const [deleting, setDeleting] = useState<string | null>(null); // job_id being deleted
  
  // Filters
  const [statusFilter, setStatusFilter] = useState('');
  const [symbolFilter, setSymbolFilter] = useState('');
  const [exchangeFilter, setExchangeFilter] = useState('');
  
  // Dropdown options
  const [symbols, setSymbols] = useState<string[]>([]);
  const [exchanges, setExchanges] = useState<string[]>([]);
  
  // Pagination
  const [limit] = useState(50);
  const [offset, setOffset] = useState(0);
  const currentPage = Math.floor(offset / limit) + 1;
  const totalPages = Math.ceil(total / limit);

  // Fetch symbols and exchanges on mount
  useEffect(() => {
    const fetchMetadata = async () => {
      try {
        const [symbolsRes, exchangesRes] = await Promise.all([
          fetch('/api/symbols'),
          fetch('/api/exchanges'),
        ]);

        if (symbolsRes.ok) {
          const symbolsData = await symbolsRes.json();
          setSymbols(symbolsData.data.map((item: any) => item.symbol));
        }

        if (exchangesRes.ok) {
          const exchangesData = await exchangesRes.json();
          setExchanges(exchangesData.data.map((item: any) => item.exchange));
        }
      } catch (err) {
        console.error('Failed to fetch metadata:', err);
      }
    };

    fetchMetadata();
  }, []);

  const fetchJobs = useCallback(async () => {
    setLoading(true);
    setError(null);

    try {
      // Build query parameters
      const params = new URLSearchParams();
      if (statusFilter) params.append('status', statusFilter);
      if (symbolFilter) params.append('symbol', symbolFilter);
      if (exchangeFilter) params.append('exchange', exchangeFilter);
      params.append('limit', limit.toString());
      params.append('offset', offset.toString());

      const response = await fetch(`/api/backtests?${params.toString()}`);
      
      if (!response.ok) {
        throw new Error('Failed to fetch backtests');
      }

      const data = await response.json();
      const newJobs = data.backtests || [];
      
      // Prevent progress from going backwards (race condition guard)
      setJobs(prevJobs => {
        const prevJobMap = new Map(prevJobs.map(j => [j.job_id, j]));
        return newJobs.map((newJob: BacktestJob) => {
          const prevJob = prevJobMap.get(newJob.job_id);
          if (prevJob && newJob.status === 'running' && prevJob.status === 'running') {
            // Keep the higher progress value to prevent UI glitches
            if (newJob.progress < prevJob.progress) {
              return { ...newJob, progress: prevJob.progress };
            }
          }
          return newJob;
        });
      });
      setTotal(data.total || 0);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'An error occurred');
    } finally {
      setLoading(false);
    }
  }, [statusFilter, symbolFilter, exchangeFilter, limit, offset]);

  // Initial fetch
  useEffect(() => {
    fetchJobs();
  }, [fetchJobs]);

  // Auto-refresh for running jobs
  useEffect(() => {
    const hasRunningJobs = jobs.some(job => job.status === 'running' || job.status === 'queued');
    
    if (hasRunningJobs && refreshInterval > 0) {
      const interval = setInterval(fetchJobs, refreshInterval);
      return () => clearInterval(interval);
    }
  }, [jobs, refreshInterval, fetchJobs]);

  const formatNumber = (num?: number, decimals = 2) => {
    if (num === undefined || num === null) return '-';
    return num.toFixed(decimals);
  };

  const formatElapsedTime = (startedAt: string): string => {
    const start = new Date(startedAt).getTime();
    const now = Date.now();
    const elapsedSeconds = Math.floor((now - start) / 1000);
    
    if (elapsedSeconds < 60) return `${elapsedSeconds}s elapsed`;
    const minutes = Math.floor(elapsedSeconds / 60);
    const seconds = elapsedSeconds % 60;
    if (minutes < 60) return `${minutes}m ${seconds}s elapsed`;
    const hours = Math.floor(minutes / 60);
    const remainingMinutes = minutes % 60;
    return `${hours}h ${remainingMinutes}m elapsed`;
  };

  const estimateTimeRemaining = (startedAt: string, progress: number): string => {
    if (progress <= 5 || progress >= 99) return '';
    
    const start = new Date(startedAt).getTime();
    const now = Date.now();
    const elapsedMs = now - start;
    const progressRate = progress / 100;
    const totalEstimatedMs = elapsedMs / progressRate;
    const remainingMs = totalEstimatedMs - elapsedMs;
    const remainingSeconds = Math.floor(remainingMs / 1000);
    
    if (remainingSeconds < 60) return `~${remainingSeconds}s remaining`;
    const minutes = Math.floor(remainingSeconds / 60);
    if (minutes < 60) return `~${minutes}m remaining`;
    const hours = Math.floor(minutes / 60);
    const remainingMinutes = minutes % 60;
    return `~${hours}h ${remainingMinutes}m remaining`;
  };

  const getProgressStage = (job: BacktestJob): string => {
    // Use actual phase from backend if available
    if (job.current_phase) {
      const phaseIcons: { [key: string]: string } = {
        'initializing': '⏳',
        'loading_data': '📂',
        'data_loading': '📂',
        'data_ready': '✅',
        'preparing': '⚙️',
        'running_backtest': '▶️',
        'processing_results': '📊',
        'optimizing': '🧬',
        'genetic_optimization': '🧬',
        'post_optimization': '🎯',
        'monte_carlo': '🎲',
        'walk_forward': '🔄',
        'final_analysis': '📊',
        'final_evaluation': '🎯',
        'calculating_metrics': '📊',
        'portfolio_loading': '📂',
        'portfolio_simulation': '📈',
        'portfolio_ga_loading': '📂',
        'portfolio_ga_symbol_optimization': '🧬',
        'portfolio_ga_weight_optimization': '⚖️',
        'portfolio_final_simulation': '📊',
        'completed': '✅',
        'failed': '❌',
        'pending': '⏳',
        'cancelled': '🚫',
      };
      const icon = phaseIcons[job.current_phase] || '🔄';
      const phaseName = job.current_phase.replace(/_/g, ' ').replace(/\b\w/g, c => c.toUpperCase());
      return `${icon} ${phaseName}`;
    }
    
    // Fallback to progress-based estimation (matches backend progress ranges)
    const progress = job.progress;
    if (progress < 5) return '📂 Loading data...';
    if (progress < 85) return '🧬 Genetic optimization...';
    if (progress < 95) return '📊 Final analysis...';
    return '✅ Finalizing...';
  };

  const getPhaseDetails = (job: BacktestJob): string | null => {
    const details = job.phase_details;
    
    // Debug logging for generation values
    if (process.env.NODE_ENV === 'development') {
      console.log(`[JobList] Job ${job.job_id?.substring(0, 8)} generation debug:`, {
        'job.current_generation': job.current_generation,
        'job.total_generations': job.total_generations,
        'details?.current_generation': details?.current_generation,
        'details?.total_generations': details?.total_generations,
      });
    }
    
    const parts: string[] = [];
    
    // Generation progress - check for both undefined AND null
    // Priority: phase_details > job fields
    const currentGen = details?.current_generation ?? job.current_generation;
    const totalGen = details?.total_generations ?? job.total_generations;
    
    if (currentGen != null && totalGen != null && totalGen > 0) {
      parts.push(`Gen ${currentGen}/${totalGen}`);
    }
    
    // Chromosome progress within generation
    if (details?.chromosomes_evaluated != null && details?.total_chromosomes != null) {
      parts.push(`Chromo ${details.chromosomes_evaluated}/${details.total_chromosomes}`);
    }
    
    // Best fitness so far
    if (details?.best_fitness != null) {
      parts.push(`Best: ${details.best_fitness.toFixed(2)}`);
    }
    
    // Stage within phase
    if (details?.stage) {
      parts.push(details.stage);
    }
    
    return parts.length > 0 ? parts.join(' • ') : null;
  };

  const handlePageChange = (newPage: number) => {
    setOffset((newPage - 1) * limit);
  };

  const handleCancelClick = (e: React.MouseEvent, jobId: string) => {
    e.stopPropagation(); // Prevent row click
    setCancelConfirm(jobId);
  };

  const handleDeleteClick = (e: React.MouseEvent, jobId: string) => {
    e.stopPropagation(); // Prevent row click
    setDeleteConfirm(jobId);
  };

  const handleCancelConfirm = async (jobId: string) => {
    setCancelling(jobId);
    setError(null);

    try {
      const response = await fetch(`/api/backtest/${jobId}/cancel`, {
        method: 'POST',
      });

      if (!response.ok) {
        const data = await response.json();
        throw new Error(data.error || 'Failed to cancel backtest');
      }

      // Refresh the list
      await fetchJobs();
      setCancelConfirm(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to cancel backtest');
    } finally {
      setCancelling(null);
    }
  };

  const handleDeleteConfirm = async (jobId: string) => {
    setDeleting(jobId);
    setError(null);

    try {
      const response = await fetch(`/api/backtest/${jobId}`, {
        method: 'DELETE',
      });

      if (!response.ok) {
        const data = await response.json();
        throw new Error(data.error || 'Failed to delete backtest');
      }

      // Refresh the list
      await fetchJobs();
      setDeleteConfirm(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete backtest');
    } finally {
      setDeleting(null);
    }
  };

  const handleCancelCancel = () => {
    setCancelConfirm(null);
  };

  const handleDeleteCancel = () => {
    setDeleteConfirm(null);
  };

  const isJobCancellable = (status: string) => {
    return ['running', 'queued'].includes(status.toLowerCase());
  };

  const isJobDeletable = (status: string) => {
    return ['completed', 'failed', 'cancelled'].includes(status.toLowerCase());
  };

  return (
    <div className="job-list-container" style={{ padding: '20px', maxWidth: '1400px', margin: '0 auto' }}>
      {/* Header */}
      <div style={{ 
        display: 'flex', 
        justifyContent: 'space-between', 
        alignItems: 'center',
        marginBottom: '24px'
      }}>
        <div>
          <h2 style={{ margin: '0 0 4px 0' }}>Backtest Jobs</h2>
          <div style={{ fontSize: '13px', color: '#666' }}>
            {total} total job{total !== 1 ? 's' : ''} • 
            {jobs.filter(j => j.status === 'running').length} running • 
            {jobs.filter(j => j.status === 'queued').length} queued
          </div>
        </div>
        <button
          onClick={fetchJobs}
          disabled={loading}
          style={{
            padding: '10px 20px',
            backgroundColor: loading ? '#ccc' : '#007bff',
            color: 'white',
            border: 'none',
            borderRadius: '6px',
            cursor: loading ? 'not-allowed' : 'pointer',
            fontWeight: 600,
            display: 'flex',
            alignItems: 'center',
            gap: '6px'
          }}
        >
          {loading ? '⏳ Loading...' : '🔄 Refresh'}
        </button>
      </div>

      {/* Status Summary Cards */}
      <div style={{ 
        display: 'grid', 
        gridTemplateColumns: 'repeat(5, 1fr)', 
        gap: '12px',
        marginBottom: '24px'
      }}>
        {[
          { status: '', label: 'All', icon: '📋', color: '#6c757d' },
          { status: 'running', label: 'Running', icon: '▶️', color: '#17a2b8' },
          { status: 'queued', label: 'Queued', icon: '⏳', color: '#ffc107' },
          { status: 'completed', label: 'Completed', icon: '✅', color: '#28a745' },
          { status: 'failed', label: 'Failed', icon: '❌', color: '#dc3545' },
        ].map(s => {
          const count = s.status === '' ? total : jobs.filter(j => j.status === s.status).length;
          const isActive = statusFilter === s.status;
          return (
            <button
              key={s.status}
              onClick={() => { setStatusFilter(s.status); setOffset(0); }}
              style={{
                padding: '12px 16px',
                border: isActive ? `2px solid ${s.color}` : '1px solid #e0e0e0',
                borderRadius: '8px',
                backgroundColor: isActive ? `${s.color}15` : 'white',
                cursor: 'pointer',
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'center',
                gap: '4px',
                transition: 'all 0.2s'
              }}
            >
              <span style={{ fontSize: '20px' }}>{s.icon}</span>
              <span style={{ fontSize: '12px', color: '#666' }}>{s.label}</span>
              <span style={{ fontSize: '18px', fontWeight: 700, color: s.color }}>{count}</span>
            </button>
          );
        })}
      </div>

      {/* Filters Row */}
      <div style={{ 
        display: 'flex', 
        gap: '12px', 
        marginBottom: '20px',
        padding: '16px',
        backgroundColor: '#f8f9fa',
        borderRadius: '8px',
        alignItems: 'center'
      }}>
        <span style={{ fontSize: '13px', fontWeight: 600, color: '#666' }}>Filter by:</span>
        
        <select
          id="symbol-filter"
          value={symbolFilter}
          onChange={(e) => { setSymbolFilter(e.target.value); setOffset(0); }}
          style={{ 
            padding: '8px 12px', 
            borderRadius: '6px', 
            border: '1px solid #ccc',
            fontSize: '13px',
            minWidth: '140px'
          }}
        >
          <option value="">All Symbols</option>
          {symbols.map(symbol => (
            <option key={symbol} value={symbol}>{symbol}</option>
          ))}
        </select>

        <select
          id="exchange-filter"
          value={exchangeFilter}
          onChange={(e) => { setExchangeFilter(e.target.value); setOffset(0); }}
          style={{ 
            padding: '8px 12px', 
            borderRadius: '6px', 
            border: '1px solid #ccc',
            fontSize: '13px',
            minWidth: '140px'
          }}
        >
          <option value="">All Exchanges</option>
          {exchanges.map(exchange => (
            <option key={exchange} value={exchange}>{exchange}</option>
          ))}
        </select>

        {(symbolFilter || exchangeFilter || statusFilter) && (
          <button
            onClick={() => { setSymbolFilter(''); setExchangeFilter(''); setStatusFilter(''); setOffset(0); }}
            style={{
              padding: '8px 12px',
              backgroundColor: '#e9ecef',
              border: 'none',
              borderRadius: '6px',
              cursor: 'pointer',
              fontSize: '13px',
              color: '#666'
            }}
          >
            ✕ Clear Filters
          </button>
        )}
      </div>

      {/* Error Display */}
      {error && (
        <div style={{
          padding: '14px 18px',
          backgroundColor: '#f8d7da',
          border: '1px solid #f5c6cb',
          borderRadius: '8px',
          color: '#721c24',
          marginBottom: '20px',
        }}>
          <strong>Error:</strong> {error}
        </div>
      )}

      {/* Jobs List - Card Based */}
      <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
        {jobs.length === 0 ? (
          <div style={{ 
            padding: '60px 20px', 
            textAlign: 'center', 
            color: '#666',
            backgroundColor: 'white',
            borderRadius: '12px',
            border: '2px dashed #e0e0e0'
          }}>
            <div style={{ fontSize: '48px', marginBottom: '16px' }}>📭</div>
            <div style={{ fontSize: '18px', fontWeight: 600, marginBottom: '8px' }}>No backtests found</div>
            <div style={{ fontSize: '14px' }}>Create a new backtest to get started</div>
          </div>
        ) : (
          jobs.map((job) => {
            const statusColors: { [key: string]: { bg: string; border: string; text: string } } = {
              queued: { bg: '#fff8e1', border: '#ffc107', text: '#856404' },
              running: { bg: '#e3f2fd', border: '#17a2b8', text: '#0c5460' },
              completed: { bg: '#e8f5e9', border: '#28a745', text: '#155724' },
              failed: { bg: '#ffebee', border: '#dc3545', text: '#721c24' },
              cancelled: { bg: '#f5f5f5', border: '#6c757d', text: '#495057' },
            };
            const colors = statusColors[job.status] ?? statusColors.cancelled ?? { bg: '#f5f5f5', border: '#6c757d', text: '#495057' };
            
            return (
              <div
                key={job.job_id}
                onClick={() => onJobClick && onJobClick(job.job_id)}
                style={{
                  backgroundColor: 'white',
                  borderRadius: '12px',
                  border: `1px solid #e0e0e0`,
                  borderLeft: `4px solid ${colors.border}`,
                  cursor: onJobClick ? 'pointer' : 'default',
                  transition: 'box-shadow 0.2s, transform 0.1s',
                  overflow: 'hidden'
                }}
                onMouseEnter={(e) => {
                  if (onJobClick) {
                    e.currentTarget.style.boxShadow = '0 4px 12px rgba(0,0,0,0.1)';
                    e.currentTarget.style.transform = 'translateY(-1px)';
                  }
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.boxShadow = 'none';
                  e.currentTarget.style.transform = 'none';
                }}
              >
                {/* Card Header */}
                <div style={{ 
                  padding: '16px 20px',
                  borderBottom: '1px solid #f0f0f0',
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center'
                }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: '16px' }}>
                    {/* Status Badge */}
                    <div style={{
                      padding: '6px 12px',
                      borderRadius: '6px',
                      backgroundColor: colors.bg,
                      color: colors.text,
                      fontSize: '12px',
                      fontWeight: 600,
                      textTransform: 'uppercase',
                      letterSpacing: '0.5px'
                    }}>
                      {job.status}
                    </div>
                    
                    {/* Symbol & Exchange */}
                    <div>
                      {job.portfolio_assets && job.portfolio_assets.length > 1 ? (
                        <>
                          <div style={{ fontWeight: 700, fontSize: '16px', color: '#333' }}>
                            📊 Portfolio ({job.portfolio_assets.length} assets)
                          </div>
                          <div style={{ fontSize: '12px', color: '#666', display: 'flex', flexWrap: 'wrap', gap: '4px', maxWidth: '300px' }}>
                            {job.portfolio_assets.map((asset, idx) => (
                              <span key={idx} style={{ 
                                backgroundColor: '#f0f0f0', 
                                padding: '2px 6px', 
                                borderRadius: '4px',
                                whiteSpace: 'nowrap'
                              }}>
                                {asset.symbol}
                                <span style={{ color: '#999', fontSize: '10px', marginLeft: '2px' }}>
                                  @{asset.exchange}
                                </span>
                              </span>
                            ))}
                          </div>
                        </>
                      ) : (
                        <>
                          <div style={{ fontWeight: 700, fontSize: '16px', color: '#333' }}>
                            {job.symbol}
                          </div>
                          <div style={{ fontSize: '12px', color: '#666' }}>
                            {job.exchange}
                          </div>
                        </>
                      )}
                    </div>
                  </div>
                  
                  <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
                    {/* Job ID */}
                    <code style={{ 
                      fontSize: '11px', 
                      color: '#999',
                      backgroundColor: '#f5f5f5',
                      padding: '4px 8px',
                      borderRadius: '4px'
                    }}>
                      {job.job_id.substring(0, 8)}...
                    </code>
                    
                    {/* Action Button */}
                    {isJobCancellable(job.status) ? (
                      <button
                        onClick={(e) => handleCancelClick(e, job.job_id)}
                        disabled={cancelling === job.job_id}
                        style={{
                          padding: '6px 14px',
                          backgroundColor: '#fff3cd',
                          color: '#856404',
                          border: '1px solid #ffc107',
                          borderRadius: '6px',
                          cursor: cancelling === job.job_id ? 'not-allowed' : 'pointer',
                          fontSize: '12px',
                          fontWeight: 500,
                          opacity: cancelling === job.job_id ? 0.6 : 1,
                        }}
                      >
                        {cancelling === job.job_id ? '⏳' : '⏹️'} Cancel
                      </button>
                    ) : isJobDeletable(job.status) ? (
                      <button
                        onClick={(e) => handleDeleteClick(e, job.job_id)}
                        disabled={deleting === job.job_id}
                        style={{
                          padding: '6px 14px',
                          backgroundColor: '#ffebee',
                          color: '#c62828',
                          border: '1px solid #ef9a9a',
                          borderRadius: '6px',
                          cursor: deleting === job.job_id ? 'not-allowed' : 'pointer',
                          fontSize: '12px',
                          fontWeight: 500,
                          opacity: deleting === job.job_id ? 0.6 : 1,
                        }}
                      >
                        {deleting === job.job_id ? '⏳' : '🗑️'} Delete
                      </button>
                    ) : null}
                  </div>
                </div>

                {/* Card Body */}
                <div style={{ padding: '16px 20px' }}>
                  {/* Progress Section (for running/queued) */}
                  {(job.status === 'running' || job.status === 'queued') && (
                    <div style={{ marginBottom: '16px' }}>
                      <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '8px' }}>
                        <span style={{ fontSize: '13px', fontWeight: 600, color: '#17a2b8' }}>
                          {getProgressStage(job)}
                        </span>
                        <span style={{ fontSize: '13px', fontWeight: 600 }}>
                          {job.progress.toFixed(0)}%
                        </span>
                      </div>
                      <div style={{
                        height: '10px',
                        backgroundColor: '#e9ecef',
                        borderRadius: '5px',
                        overflow: 'hidden',
                      }}>
                        <div style={{
                          width: `${job.progress}%`,
                          height: '100%',
                          backgroundColor: '#17a2b8',
                          transition: 'width 0.3s',
                          borderRadius: '5px',
                        }} />
                      </div>
                      {/* Phase details */}
                      {getPhaseDetails(job) && (
                        <div style={{ fontSize: '12px', color: '#666', marginTop: '8px' }}>
                          {getPhaseDetails(job)}
                        </div>
                      )}
                      {/* Elapsed & ETA */}
                      {job.started_at && (
                        <div style={{ fontSize: '12px', color: '#888', marginTop: '4px' }}>
                          {formatElapsedTime(job.started_at)}
                          {job.progress > 5 && ` • ${estimateTimeRemaining(job.started_at, job.progress)}`}
                        </div>
                      )}
                    </div>
                  )}

                  {/* Error Message (for failed) */}
                  {job.status === 'failed' && job.error_message && (
                    <div style={{ 
                      marginBottom: '16px',
                      padding: '12px',
                      backgroundColor: '#ffebee',
                      borderRadius: '6px',
                      fontSize: '13px',
                      color: '#c62828'
                    }}>
                      <strong>Error:</strong> {job.error_message}
                    </div>
                  )}

                  {/* Metrics Grid */}
                  <div style={{ 
                    display: 'grid', 
                    gridTemplateColumns: 'repeat(5, 1fr)', 
                    gap: '16px' 
                  }}>
                    <div>
                      <div style={{ fontSize: '11px', color: '#888', marginBottom: '2px' }}>Sharpe Ratio</div>
                      <div style={{ 
                        fontSize: '18px', 
                        fontWeight: 700,
                        color: (job.sharpe_ratio ?? 0) > 1.5 ? '#28a745' : (job.sharpe_ratio ?? 0) > 0 ? '#333' : '#dc3545'
                      }}>
                        {job.sharpe_ratio !== undefined ? formatNumber(job.sharpe_ratio) : '—'}
                      </div>
                    </div>
                    <div>
                      <div style={{ fontSize: '11px', color: '#888', marginBottom: '2px' }}>Max Drawdown</div>
                      <div style={{ 
                        fontSize: '18px', 
                        fontWeight: 700,
                        color: (job.max_drawdown ?? 0) > 0.2 ? '#dc3545' : '#28a745'
                      }}>
                        {job.max_drawdown !== undefined ? `${formatNumber(job.max_drawdown * 100)}%` : '—'}
                      </div>
                    </div>
                    <div>
                      <div style={{ fontSize: '11px', color: '#888', marginBottom: '2px' }}>Total P&L</div>
                      <div style={{ 
                        fontSize: '18px', 
                        fontWeight: 700,
                        color: (job.total_pnl ?? 0) >= 0 ? '#28a745' : '#dc3545'
                      }}>
                        {job.total_pnl !== undefined ? `$${formatNumber(job.total_pnl, 0)}` : '—'}
                      </div>
                    </div>
                    <div>
                      <div style={{ fontSize: '11px', color: '#888', marginBottom: '2px' }}>Win Rate</div>
                      <div style={{ 
                        fontSize: '18px', 
                        fontWeight: 700,
                        color: (job.win_rate ?? 0) > 0.5 ? '#28a745' : '#333'
                      }}>
                        {job.win_rate !== undefined ? `${formatNumber(job.win_rate * 100)}%` : '—'}
                      </div>
                    </div>
                    <div>
                      <div style={{ fontSize: '11px', color: '#888', marginBottom: '2px' }}>Created</div>
                      <div style={{ fontSize: '13px', fontWeight: 500, color: '#333' }}>
                        {new Date(job.created_at).toLocaleDateString()}
                      </div>
                      <div style={{ fontSize: '11px', color: '#888' }}>
                        {new Date(job.created_at).toLocaleTimeString()}
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            );
          })
        )}
      </div>

      {/* Cancel Confirmation Modal */}
      {cancelConfirm && (
        <div style={{
          position: 'fixed',
          top: 0,
          left: 0,
          right: 0,
          bottom: 0,
          backgroundColor: 'rgba(0,0,0,0.5)',
          display: 'flex',
          justifyContent: 'center',
          alignItems: 'center',
          zIndex: 1000,
        }}>
          <div style={{
            backgroundColor: 'white',
            padding: '30px',
            borderRadius: '8px',
            boxShadow: '0 4px 6px rgba(0,0,0,0.1)',
            maxWidth: '500px',
            width: '90%',
          }}>
            <h3 style={{ marginTop: 0, marginBottom: '20px' }}>Confirm Cancel</h3>
            <p style={{ marginBottom: '20px' }}>
              Are you sure you want to cancel this running backtest?
            </p>
            <p style={{ 
              fontSize: '12px', 
              color: '#666', 
              fontFamily: 'monospace',
              marginBottom: '20px',
              wordBreak: 'break-all',
            }}>
              Job ID: {cancelConfirm}
            </p>
            <div style={{ display: 'flex', gap: '10px', justifyContent: 'flex-end' }}>
              <button
                onClick={handleCancelCancel}
                disabled={cancelling === cancelConfirm}
                style={{
                  padding: '8px 16px',
                  backgroundColor: '#6c757d',
                  color: 'white',
                  border: 'none',
                  borderRadius: '4px',
                  cursor: cancelling === cancelConfirm ? 'not-allowed' : 'pointer',
                }}
              >
                No, Keep Running
              </button>
              <button
                onClick={() => handleCancelConfirm(cancelConfirm)}
                disabled={cancelling === cancelConfirm}
                style={{
                  padding: '8px 16px',
                  backgroundColor: '#ffc107',
                  color: '#212529',
                  border: 'none',
                  borderRadius: '4px',
                  cursor: cancelling === cancelConfirm ? 'not-allowed' : 'pointer',
                  opacity: cancelling === cancelConfirm ? 0.6 : 1,
                }}
              >
                {cancelling === cancelConfirm ? 'Cancelling...' : 'Yes, Cancel Job'}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Delete Confirmation Modal */}
      {deleteConfirm && (
        <div style={{
          position: 'fixed',
          top: 0,
          left: 0,
          right: 0,
          bottom: 0,
          backgroundColor: 'rgba(0,0,0,0.5)',
          display: 'flex',
          justifyContent: 'center',
          alignItems: 'center',
          zIndex: 1000,
        }}>
          <div style={{
            backgroundColor: 'white',
            padding: '30px',
            borderRadius: '8px',
            boxShadow: '0 4px 6px rgba(0,0,0,0.1)',
            maxWidth: '500px',
            width: '90%',
          }}>
            <h3 style={{ marginTop: 0, marginBottom: '20px' }}>Confirm Delete</h3>
            <p style={{ marginBottom: '20px' }}>
              Are you sure you want to permanently delete this backtest and all its results?
            </p>
            <p style={{ 
              fontSize: '12px', 
              color: '#666', 
              fontFamily: 'monospace',
              marginBottom: '20px',
              wordBreak: 'break-all',
            }}>
              Job ID: {deleteConfirm}
            </p>
            <div style={{ display: 'flex', gap: '10px', justifyContent: 'flex-end' }}>
              <button
                onClick={handleDeleteCancel}
                disabled={deleting === deleteConfirm}
                style={{
                  padding: '8px 16px',
                  backgroundColor: '#6c757d',
                  color: 'white',
                  border: 'none',
                  borderRadius: '4px',
                  cursor: deleting === deleteConfirm ? 'not-allowed' : 'pointer',
                }}
              >
                Cancel
              </button>
              <button
                onClick={() => handleDeleteConfirm(deleteConfirm)}
                disabled={deleting === deleteConfirm}
                style={{
                  padding: '8px 16px',
                  backgroundColor: '#dc3545',
                  color: 'white',
                  border: 'none',
                  borderRadius: '4px',
                  cursor: deleting === deleteConfirm ? 'not-allowed' : 'pointer',
                  opacity: deleting === deleteConfirm ? 0.6 : 1,
                }}
              >
                {deleting === deleteConfirm ? 'Deleting...' : 'Delete'}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Pagination */}
      {totalPages > 1 && (
        <div style={{
          display: 'flex',
          justifyContent: 'center',
          alignItems: 'center',
          gap: '10px',
          marginTop: '20px',
        }}>
          <button
            onClick={() => handlePageChange(currentPage - 1)}
            disabled={currentPage === 1}
            style={{
              padding: '6px 12px',
              border: '1px solid #dee2e6',
              borderRadius: '4px',
              backgroundColor: currentPage === 1 ? '#e9ecef' : 'white',
              cursor: currentPage === 1 ? 'not-allowed' : 'pointer',
            }}
          >
            Previous
          </button>
          
          <span style={{ color: '#666' }}>
            Page {currentPage} of {totalPages} ({total} total)
          </span>
          
          <button
            onClick={() => handlePageChange(currentPage + 1)}
            disabled={currentPage === totalPages}
            style={{
              padding: '6px 12px',
              border: '1px solid #dee2e6',
              borderRadius: '4px',
              backgroundColor: currentPage === totalPages ? '#e9ecef' : 'white',
              cursor: currentPage === totalPages ? 'not-allowed' : 'pointer',
            }}
          >
            Next
          </button>
        </div>
      )}
    </div>
  );
};

export default JobList;
