import React, { useState, useEffect, useCallback } from 'react';

interface BacktestJob {
  job_id: string;
  symbol: string;
  exchange: string;
  status: string;
  progress: number;
  created_at: string;
  started_at?: string;
  completed_at?: string;
  sharpe_ratio?: number;
  max_drawdown?: number;
  total_pnl?: number;
  win_rate?: number;
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
      setJobs(data.backtests || []);
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

  const getStatusBadge = (status: string) => {
    const colors: { [key: string]: string } = {
      queued: '#ffc107',
      running: '#17a2b8',
      completed: '#28a745',
      failed: '#dc3545',
    };

    return (
      <span
        style={{
          padding: '4px 8px',
          borderRadius: '4px',
          backgroundColor: colors[status] || '#6c757d',
          color: 'white',
          fontSize: '12px',
          fontWeight: 'bold',
        }}
      >
        {status.toUpperCase()}
      </span>
    );
  };

  const formatDate = (dateStr?: string) => {
    if (!dateStr) return '-';
    return new Date(dateStr).toLocaleString();
  };

  const formatNumber = (num?: number, decimals = 2) => {
    if (num === undefined || num === null) return '-';
    return num.toFixed(decimals);
  };

  const handlePageChange = (newPage: number) => {
    setOffset((newPage - 1) * limit);
  };

  const handleDeleteClick = (e: React.MouseEvent, jobId: string) => {
    e.stopPropagation(); // Prevent row click
    setDeleteConfirm(jobId);
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

  const handleDeleteCancel = () => {
    setDeleteConfirm(null);
  };

  return (
    <div className="job-list-container" style={{ padding: '20px' }}>
      <h2>Backtest Jobs</h2>

      {/* Filters */}
      <div style={{ 
        display: 'flex', 
        gap: '10px', 
        marginBottom: '20px',
        flexWrap: 'wrap',
      }}>
        <div>
          <label htmlFor="status-filter" style={{ marginRight: '5px' }}>Status:</label>
          <select
            id="status-filter"
            value={statusFilter}
            onChange={(e) => { setStatusFilter(e.target.value); setOffset(0); }}
            style={{ padding: '6px', borderRadius: '4px', border: '1px solid #ccc' }}
          >
            <option value="">All</option>
            <option value="queued">Queued</option>
            <option value="running">Running</option>
            <option value="completed">Completed</option>
            <option value="failed">Failed</option>
          </select>
        </div>

        <div>
          <label htmlFor="symbol-filter" style={{ marginRight: '5px' }}>Symbol:</label>
          <select
            id="symbol-filter"
            value={symbolFilter}
            onChange={(e) => { setSymbolFilter(e.target.value); setOffset(0); }}
            style={{ padding: '6px', borderRadius: '4px', border: '1px solid #ccc', minWidth: '120px' }}
          >
            <option value="">All Symbols</option>
            {symbols.map(symbol => (
              <option key={symbol} value={symbol}>{symbol}</option>
            ))}
          </select>
        </div>

        <div>
          <label htmlFor="exchange-filter" style={{ marginRight: '5px' }}>Exchange:</label>
          <select
            id="exchange-filter"
            value={exchangeFilter}
            onChange={(e) => { setExchangeFilter(e.target.value); setOffset(0); }}
            style={{ padding: '6px', borderRadius: '4px', border: '1px solid #ccc', minWidth: '120px' }}
          >
            <option value="">All Exchanges</option>
            {exchanges.map(exchange => (
              <option key={exchange} value={exchange}>{exchange}</option>
            ))}
          </select>
        </div>

        <button
          onClick={fetchJobs}
          style={{
            padding: '6px 12px',
            backgroundColor: '#007bff',
            color: 'white',
            border: 'none',
            borderRadius: '4px',
            cursor: 'pointer',
          }}
        >
          Refresh
        </button>
      </div>

      {/* Error Display */}
      {error && (
        <div style={{
          padding: '10px',
          backgroundColor: '#f8d7da',
          border: '1px solid #f5c6cb',
          borderRadius: '4px',
          color: '#721c24',
          marginBottom: '20px',
        }}>
          <strong>Error:</strong> {error}
        </div>
      )}

      {/* Loading Indicator */}
      {loading && <div style={{ marginBottom: '10px', color: '#666' }}>Loading...</div>}

      {/* Jobs Table */}
      <div style={{ overflowX: 'auto' }}>
        <table style={{
          width: '100%',
          borderCollapse: 'collapse',
          backgroundColor: 'white',
          boxShadow: '0 2px 4px rgba(0,0,0,0.1)',
        }}>
          <thead>
            <tr style={{ backgroundColor: '#f8f9fa', borderBottom: '2px solid #dee2e6' }}>
              <th style={{ padding: '12px', textAlign: 'left' }}>Job ID</th>
              <th style={{ padding: '12px', textAlign: 'left' }}>Symbol</th>
              <th style={{ padding: '12px', textAlign: 'left' }}>Exchange</th>
              <th style={{ padding: '12px', textAlign: 'left' }}>Status</th>
              <th style={{ padding: '12px', textAlign: 'left' }}>Progress</th>
              <th style={{ padding: '12px', textAlign: 'left' }}>Sharpe</th>
              <th style={{ padding: '12px', textAlign: 'left' }}>Max DD</th>
              <th style={{ padding: '12px', textAlign: 'left' }}>Total P&L</th>
              <th style={{ padding: '12px', textAlign: 'left' }}>Win Rate</th>
              <th style={{ padding: '12px', textAlign: 'left' }}>Created</th>
              <th style={{ padding: '12px', textAlign: 'left' }}>Actions</th>
            </tr>
          </thead>
          <tbody>
            {jobs.length === 0 ? (
              <tr>
                <td colSpan={11} style={{ padding: '20px', textAlign: 'center', color: '#666' }}>
                  No backtests found
                </td>
              </tr>
            ) : (
              jobs.map((job) => (
                <tr
                  key={job.job_id}
                  onClick={() => onJobClick && onJobClick(job.job_id)}
                  style={{
                    borderBottom: '1px solid #dee2e6',
                    cursor: onJobClick ? 'pointer' : 'default',
                  }}
                  onMouseEnter={(e) => {
                    if (onJobClick) e.currentTarget.style.backgroundColor = '#f8f9fa';
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.backgroundColor = 'white';
                  }}
                >
                  <td style={{ padding: '12px' }}>
                    <code style={{ fontSize: '12px' }}>{job.job_id.substring(0, 8)}...</code>
                  </td>
                  <td style={{ padding: '12px' }}>{job.symbol}</td>
                  <td style={{ padding: '12px' }}>{job.exchange}</td>
                  <td style={{ padding: '12px' }}>{getStatusBadge(job.status)}</td>
                  <td style={{ padding: '12px' }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                      <div style={{
                        flex: 1,
                        height: '8px',
                        backgroundColor: '#e9ecef',
                        borderRadius: '4px',
                        overflow: 'hidden',
                      }}>
                        <div style={{
                          width: `${job.progress}%`,
                          height: '100%',
                          backgroundColor: job.status === 'completed' ? '#28a745' : '#17a2b8',
                          transition: 'width 0.3s',
                        }} />
                      </div>
                      <span style={{ fontSize: '12px', minWidth: '35px' }}>
                        {job.progress.toFixed(0)}%
                      </span>
                    </div>
                  </td>
                  <td style={{ padding: '12px' }}>{formatNumber(job.sharpe_ratio)}</td>
                  <td style={{ padding: '12px' }}>{formatNumber(job.max_drawdown)}%</td>
                  <td style={{ padding: '12px', color: (job.total_pnl || 0) >= 0 ? '#28a745' : '#dc3545' }}>
                    {formatNumber(job.total_pnl, 2)}
                  </td>
                  <td style={{ padding: '12px' }}>{formatNumber(job.win_rate)}%</td>
                  <td style={{ padding: '12px', fontSize: '12px' }}>{formatDate(job.created_at)}</td>
                  <td style={{ padding: '12px' }}>
                    <button
                      onClick={(e) => handleDeleteClick(e, job.job_id)}
                      disabled={deleting === job.job_id}
                      style={{
                        padding: '4px 8px',
                        backgroundColor: '#dc3545',
                        color: 'white',
                        border: 'none',
                        borderRadius: '4px',
                        cursor: deleting === job.job_id ? 'not-allowed' : 'pointer',
                        fontSize: '12px',
                        opacity: deleting === job.job_id ? 0.6 : 1,
                      }}
                    >
                      {deleting === job.job_id ? 'Deleting...' : 'Delete'}
                    </button>
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>

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
              Are you sure you want to delete this backtest?
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
