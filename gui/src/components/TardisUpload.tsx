import React, { useState, useCallback, useRef } from 'react';

// Supported exchanges from Tardis.dev
const TARDIS_EXCHANGES = [
  { id: 'binance-us', name: 'Binance US' },
  { id: 'binance', name: 'Binance' },
  { id: 'coinbase', name: 'Coinbase' },
  { id: 'kraken', name: 'Kraken' },
  { id: 'okx', name: 'OKX' },
  { id: 'bybit', name: 'Bybit' },
  { id: 'deribit', name: 'Deribit' },
  { id: 'ftx', name: 'FTX' },
  { id: 'huobi', name: 'Huobi' },
  { id: 'kucoin', name: 'KuCoin' },
  { id: 'gate', name: 'Gate.io' },
  { id: 'bitfinex', name: 'Bitfinex' },
];

// Types for Tardis file upload
interface UploadedFileInfo {
  file_id: string;
  original_name: string;
  file_type: 'trades' | 'orderbook';
  symbol: string;
  exchange: string;
  start_time: string;
  end_time: string;
  row_count: number;
  file_size_bytes: number;
}

interface SymbolUploadSession {
  session_id: string;
  symbol: string;
  exchange: string;
  trades_file: UploadedFileInfo | null;
  orderbook_file: UploadedFileInfo | null;
  start_time: string | null;
  end_time: string | null;
  created_at: string;
}

interface FileUploadResponse {
  success: boolean;
  file: UploadedFileInfo | null;
  session: SymbolUploadSession | null;
  error: string | null;
}

interface TardisBacktestRequest {
  session_id: string;
  initial_capital: number;
  commission_rate: number;
  strategy_type: string;
  optimize: boolean;
  population_size?: number;
  generations?: number;
  risk_aversion?: number;
  min_spread_bps?: number;
}

interface TardisBacktestResponse {
  success: boolean;
  job_id: string | null;
  message: string;
  error: string | null;
}

interface TardisUploadProps {
  onBacktestSubmitted?: (jobId: string, session: SymbolUploadSession) => void;
}

// Format bytes to human readable
const formatBytes = (bytes: number): string => {
  if (bytes === 0) return '0 Bytes';
  const k = 1024;
  const sizes = ['Bytes', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
};

// Format number with commas
const formatNumber = (n: number): string => {
  return n.toLocaleString();
};

export const TardisUpload: React.FC<TardisUploadProps> = ({ onBacktestSubmitted }) => {
  // Upload state
  const [sessions, setSessions] = useState<SymbolUploadSession[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [uploadError, setUploadError] = useState<string | null>(null);
  const [uploadProgress, setUploadProgress] = useState<string | null>(null);
  const [selectedExchange, setSelectedExchange] = useState('binance-us');
  
  // Backtest form state
  const [selectedSession, setSelectedSession] = useState<string | null>(null);
  const [backtestConfig, setBacktestConfig] = useState({
    initial_capital: 10000,
    commission_rate: 0.001,
    strategy_type: 'avellaneda_stoikov',
    optimize: true,
    population_size: 30,
    generations: 15,
    risk_aversion: 0.5,
    min_spread_bps: 10,
  });
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Handle file upload
  const uploadFile = useCallback(async (file: File, exchange: string) => {
    setUploading(true);
    setUploadError(null);
    setUploadProgress(`Uploading ${file.name}...`);
    
    try {
      const formData = new FormData();
      formData.append('file', file);
      formData.append('exchange', exchange);
      
      const response = await fetch('/api/tardis/upload', {
        method: 'POST',
        body: formData,
      });
      
      const result: FileUploadResponse = await response.json();
      
      if (result.success && result.session) {
        // Update or add session
        setSessions(prev => {
          const existing = prev.findIndex(s => s.session_id === result.session!.session_id);
          if (existing >= 0) {
            const updated = [...prev];
            updated[existing] = result.session!;
            return updated;
          } else {
            return [...prev, result.session!];
          }
        });
        
        // Auto-select if first session
        setSessions(prev => {
          if (prev.length === 1 && prev[0]) {
            setSelectedSession(prev[0].session_id);
          }
          return prev;
        });
        
        setUploadProgress(`✅ ${file.name} uploaded successfully`);
        setTimeout(() => setUploadProgress(null), 3000);
      } else {
        setUploadError(result.error || 'Upload failed');
      }
    } catch (err) {
      setUploadError(`Failed to upload: ${err}`);
    } finally {
      setUploading(false);
    }
  }, []);

  // Handle drag events
  const handleDragEnter = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(true);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(false);
  }, []);

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
  }, []);

  const handleDrop = useCallback(async (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(false);
    
    const files = Array.from(e.dataTransfer.files);
    const csvFiles = files.filter(f => 
      f.name.endsWith('.csv') || 
      f.name.endsWith('.csv.gz') || 
      f.name.endsWith('.gz')
    );
    
    if (csvFiles.length === 0) {
      setUploadError('Please drop CSV or CSV.GZ files');
      return;
    }
    
    // Upload files sequentially
    for (const file of csvFiles) {
      await uploadFile(file, selectedExchange);
    }
  }, [uploadFile, selectedExchange]);

  // Handle file input change
  const handleFileSelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (!files) return;
    
    for (const file of Array.from(files)) {
      await uploadFile(file, selectedExchange);
    }
    
    // Reset input
    if (fileInputRef.current) {
      fileInputRef.current.value = '';
    }
  };

  // Delete a session
  const handleDeleteSession = async (sessionId: string) => {
    try {
      const response = await fetch(`/api/tardis/sessions/${sessionId}`, {
        method: 'DELETE',
      });
      
      if (response.ok) {
        setSessions(prev => prev.filter(s => s.session_id !== sessionId));
        if (selectedSession === sessionId) {
          setSelectedSession(null);
        }
      }
    } catch (err) {
      console.error('Failed to delete session:', err);
    }
  };

  // Submit backtest
  const handleSubmitBacktest = async () => {
    if (!selectedSession) return;
    
    setSubmitting(true);
    setSubmitError(null);
    
    try {
      const request: TardisBacktestRequest = {
        session_id: selectedSession,
        initial_capital: backtestConfig.initial_capital,
        commission_rate: backtestConfig.commission_rate,
        strategy_type: backtestConfig.strategy_type,
        optimize: backtestConfig.optimize,
        population_size: backtestConfig.optimize ? backtestConfig.population_size : undefined,
        generations: backtestConfig.optimize ? backtestConfig.generations : undefined,
        risk_aversion: backtestConfig.risk_aversion,
        min_spread_bps: backtestConfig.min_spread_bps,
      };
      
      const response = await fetch('/api/tardis/backtest', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(request),
      });
      
      const result: TardisBacktestResponse = await response.json();
      
      if (result.success && result.job_id) {
        const session = sessions.find(s => s.session_id === selectedSession);
        if (session && onBacktestSubmitted) {
          onBacktestSubmitted(result.job_id, session);
        }
      } else {
        setSubmitError(result.error || 'Failed to submit backtest');
      }
    } catch (err) {
      setSubmitError(`Failed to submit: ${err}`);
    } finally {
      setSubmitting(false);
    }
  };

  const selectedSessionData = sessions.find(s => s.session_id === selectedSession);
  const isReadyToBacktest = selectedSessionData?.trades_file && selectedSessionData?.orderbook_file;

  return (
    <div style={{ padding: '20px', maxWidth: '900px', margin: '0 auto' }}>
      <h2 style={{ marginBottom: '20px', display: 'flex', alignItems: 'center', gap: '10px' }}>
        <span>📁</span>
        <span>Tardis Data Upload</span>
      </h2>
      
      {/* Exchange Selector */}
      <div style={{ 
        marginBottom: '20px', 
        padding: '16px', 
        backgroundColor: '#f8f9fa', 
        borderRadius: '8px',
        border: '1px solid #e0e0e0'
      }}>
        <label style={{ display: 'block', marginBottom: '8px', fontWeight: 600, fontSize: '14px' }}>
          📊 Data Source Exchange
        </label>
        <select
          value={selectedExchange}
          onChange={(e) => setSelectedExchange(e.target.value)}
          style={{
            width: '100%',
            padding: '10px 12px',
            fontSize: '15px',
            border: '1px solid #ccc',
            borderRadius: '6px',
            backgroundColor: 'white',
            cursor: 'pointer',
          }}
        >
          {TARDIS_EXCHANGES.map(ex => (
            <option key={ex.id} value={ex.id}>{ex.name}</option>
          ))}
        </select>
        <div style={{ marginTop: '6px', fontSize: '12px', color: '#666' }}>
          Select the exchange your Tardis.dev data was downloaded from
        </div>
      </div>
      
      {/* Drop Zone */}
      <div
        onDragEnter={handleDragEnter}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
        onClick={() => fileInputRef.current?.click()}
        style={{
          border: `3px dashed ${isDragging ? '#007bff' : '#ccc'}`,
          borderRadius: '12px',
          padding: '40px',
          textAlign: 'center',
          cursor: 'pointer',
          backgroundColor: isDragging ? '#f0f8ff' : '#fafafa',
          transition: 'all 0.2s ease',
          marginBottom: '20px',
        }}
      >
        <input
          ref={fileInputRef}
          type="file"
          multiple
          accept=".csv,.gz,.csv.gz"
          onChange={handleFileSelect}
          style={{ display: 'none' }}
        />
        
        <div style={{ fontSize: '48px', marginBottom: '10px' }}>
          {uploading ? '⏳' : isDragging ? '📥' : '📂'}
        </div>
        
        <div style={{ fontSize: '18px', fontWeight: 600, marginBottom: '8px' }}>
          {uploading ? 'Uploading...' : `Drag & Drop ${TARDIS_EXCHANGES.find(e => e.id === selectedExchange)?.name} CSV Files`}
        </div>
        
        <div style={{ color: '#666', fontSize: '14px' }}>
          Drop <code>*_trades.csv.gz</code> and <code>*_orderbook.csv.gz</code> files here
          <br />
          or click to browse
        </div>
        
        {uploadProgress && (
          <div style={{ marginTop: '15px', color: '#28a745', fontWeight: 500 }}>
            {uploadProgress}
          </div>
        )}
        
        {uploadError && (
          <div style={{ marginTop: '15px', color: '#dc3545', fontWeight: 500 }}>
            ❌ {uploadError}
          </div>
        )}
      </div>
      
      {/* Uploaded Sessions */}
      {sessions.length > 0 && (
        <div style={{ marginBottom: '20px' }}>
          <h3 style={{ marginBottom: '12px' }}>Uploaded Data Sets</h3>
          
          <div style={{ display: 'grid', gap: '12px' }}>
            {sessions.map(session => (
              <div
                key={session.session_id}
                onClick={() => setSelectedSession(session.session_id)}
                style={{
                  border: `2px solid ${selectedSession === session.session_id ? '#007bff' : '#e0e0e0'}`,
                  borderRadius: '8px',
                  padding: '16px',
                  cursor: 'pointer',
                  backgroundColor: selectedSession === session.session_id ? '#f0f8ff' : 'white',
                  transition: 'all 0.2s ease',
                }}
              >
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
                  <div>
                    <div style={{ fontSize: '18px', fontWeight: 600, marginBottom: '4px' }}>
                      {session.symbol} 
                      <span style={{ 
                        fontSize: '12px', 
                        backgroundColor: '#6c757d', 
                        color: 'white', 
                        padding: '2px 8px', 
                        borderRadius: '4px',
                        marginLeft: '8px'
                      }}>
                        {session.exchange}
                      </span>
                    </div>
                    
                    {session.start_time && session.end_time && (
                      <div style={{ fontSize: '13px', color: '#666', marginBottom: '8px' }}>
                        📅 {session.start_time} → {session.end_time}
                      </div>
                    )}
                    
                    <div style={{ display: 'flex', gap: '20px', fontSize: '13px' }}>
                      {/* Trades file status */}
                      <div style={{ 
                        display: 'flex', 
                        alignItems: 'center', 
                        gap: '6px',
                        color: session.trades_file ? '#28a745' : '#dc3545'
                      }}>
                        {session.trades_file ? '✅' : '❌'} Trades
                        {session.trades_file && (
                          <span style={{ color: '#666' }}>
                            ({formatNumber(session.trades_file.row_count)} rows, {formatBytes(session.trades_file.file_size_bytes)})
                          </span>
                        )}
                      </div>
                      
                      {/* Orderbook file status */}
                      <div style={{ 
                        display: 'flex', 
                        alignItems: 'center', 
                        gap: '6px',
                        color: session.orderbook_file ? '#28a745' : '#dc3545'
                      }}>
                        {session.orderbook_file ? '✅' : '❌'} Orderbook
                        {session.orderbook_file && (
                          <span style={{ color: '#666' }}>
                            ({formatNumber(session.orderbook_file.row_count)} snapshots, {formatBytes(session.orderbook_file.file_size_bytes)})
                          </span>
                        )}
                      </div>
                    </div>
                  </div>
                  
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      handleDeleteSession(session.session_id);
                    }}
                    style={{
                      background: 'none',
                      border: 'none',
                      cursor: 'pointer',
                      fontSize: '18px',
                      color: '#dc3545',
                      padding: '4px 8px',
                    }}
                    title="Delete session"
                  >
                    🗑️
                  </button>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
      
      {/* Backtest Configuration */}
      {selectedSessionData && (
        <div style={{ 
          border: '1px solid #e0e0e0', 
          borderRadius: '8px', 
          padding: '20px',
          backgroundColor: 'white'
        }}>
          <h3 style={{ marginBottom: '16px' }}>
            Backtest Configuration - {selectedSessionData.symbol}
          </h3>
          
          {!isReadyToBacktest && (
            <div style={{ 
              backgroundColor: '#fff3cd', 
              border: '1px solid #ffc107',
              borderRadius: '6px',
              padding: '12px',
              marginBottom: '16px',
              color: '#856404'
            }}>
              ⚠️ Please upload both trades and orderbook files to run a backtest.
            </div>
          )}
          
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '16px', marginBottom: '20px' }}>
            {/* Initial Capital */}
            <div>
              <label style={{ display: 'block', marginBottom: '4px', fontWeight: 500 }}>
                Initial Capital ($)
              </label>
              <input
                type="number"
                value={backtestConfig.initial_capital}
                onChange={e => setBacktestConfig(prev => ({ ...prev, initial_capital: parseFloat(e.target.value) || 0 }))}
                style={{
                  width: '100%',
                  padding: '8px 12px',
                  border: '1px solid #ccc',
                  borderRadius: '4px',
                  fontSize: '14px',
                }}
              />
            </div>
            
            {/* Commission Rate */}
            <div>
              <label style={{ display: 'block', marginBottom: '4px', fontWeight: 500 }}>
                Commission Rate (%)
              </label>
              <input
                type="number"
                step="0.01"
                value={backtestConfig.commission_rate * 100}
                onChange={e => setBacktestConfig(prev => ({ ...prev, commission_rate: (parseFloat(e.target.value) || 0) / 100 }))}
                style={{
                  width: '100%',
                  padding: '8px 12px',
                  border: '1px solid #ccc',
                  borderRadius: '4px',
                  fontSize: '14px',
                }}
              />
            </div>
            
            {/* Strategy Type */}
            <div>
              <label style={{ display: 'block', marginBottom: '4px', fontWeight: 500 }}>
                Strategy
              </label>
              <select
                value={backtestConfig.strategy_type}
                onChange={e => setBacktestConfig(prev => ({ ...prev, strategy_type: e.target.value }))}
                style={{
                  width: '100%',
                  padding: '8px 12px',
                  border: '1px solid #ccc',
                  borderRadius: '4px',
                  fontSize: '14px',
                }}
              >
                <option value="avellaneda_stoikov">Avellaneda-Stoikov Market Making</option>
                <option value="grid">Grid Trading</option>
                <option value="twap">TWAP</option>
              </select>
            </div>
            
            {/* Optimization Toggle */}
            <div>
              <label style={{ display: 'block', marginBottom: '4px', fontWeight: 500 }}>
                Optimization
              </label>
              <select
                value={backtestConfig.optimize ? 'genetic' : 'manual'}
                onChange={e => setBacktestConfig(prev => ({ ...prev, optimize: e.target.value === 'genetic' }))}
                style={{
                  width: '100%',
                  padding: '8px 12px',
                  border: '1px solid #ccc',
                  borderRadius: '4px',
                  fontSize: '14px',
                }}
              >
                <option value="genetic">Genetic Algorithm</option>
                <option value="manual">Manual Parameters</option>
              </select>
            </div>
            
            {/* GA Settings (if optimize enabled) */}
            {backtestConfig.optimize && (
              <>
                <div>
                  <label style={{ display: 'block', marginBottom: '4px', fontWeight: 500 }}>
                    Population Size
                  </label>
                  <input
                    type="number"
                    value={backtestConfig.population_size}
                    onChange={e => setBacktestConfig(prev => ({ ...prev, population_size: parseInt(e.target.value) || 30 }))}
                    style={{
                      width: '100%',
                      padding: '8px 12px',
                      border: '1px solid #ccc',
                      borderRadius: '4px',
                      fontSize: '14px',
                    }}
                  />
                </div>
                
                <div>
                  <label style={{ display: 'block', marginBottom: '4px', fontWeight: 500 }}>
                    Generations
                  </label>
                  <input
                    type="number"
                    value={backtestConfig.generations}
                    onChange={e => setBacktestConfig(prev => ({ ...prev, generations: parseInt(e.target.value) || 15 }))}
                    style={{
                      width: '100%',
                      padding: '8px 12px',
                      border: '1px solid #ccc',
                      borderRadius: '4px',
                      fontSize: '14px',
                    }}
                  />
                </div>
              </>
            )}
            
            {/* Manual Parameters (if not optimize) */}
            {!backtestConfig.optimize && (
              <>
                <div>
                  <label style={{ display: 'block', marginBottom: '4px', fontWeight: 500 }}>
                    Risk Aversion (γ)
                  </label>
                  <input
                    type="number"
                    step="0.1"
                    value={backtestConfig.risk_aversion}
                    onChange={e => setBacktestConfig(prev => ({ ...prev, risk_aversion: parseFloat(e.target.value) || 0.5 }))}
                    style={{
                      width: '100%',
                      padding: '8px 12px',
                      border: '1px solid #ccc',
                      borderRadius: '4px',
                      fontSize: '14px',
                    }}
                  />
                </div>
                
                <div>
                  <label style={{ display: 'block', marginBottom: '4px', fontWeight: 500 }}>
                    Min Spread (bps)
                  </label>
                  <input
                    type="number"
                    value={backtestConfig.min_spread_bps}
                    onChange={e => setBacktestConfig(prev => ({ ...prev, min_spread_bps: parseFloat(e.target.value) || 10 }))}
                    style={{
                      width: '100%',
                      padding: '8px 12px',
                      border: '1px solid #ccc',
                      borderRadius: '4px',
                      fontSize: '14px',
                    }}
                  />
                </div>
              </>
            )}
          </div>
          
          {submitError && (
            <div style={{ 
              backgroundColor: '#f8d7da', 
              border: '1px solid #f5c6cb',
              borderRadius: '6px',
              padding: '12px',
              marginBottom: '16px',
              color: '#721c24'
            }}>
              ❌ {submitError}
            </div>
          )}
          
          <button
            onClick={handleSubmitBacktest}
            disabled={!isReadyToBacktest || submitting}
            style={{
              width: '100%',
              padding: '14px 20px',
              fontSize: '16px',
              fontWeight: 600,
              backgroundColor: isReadyToBacktest && !submitting ? '#007bff' : '#ccc',
              color: 'white',
              border: 'none',
              borderRadius: '6px',
              cursor: isReadyToBacktest && !submitting ? 'pointer' : 'not-allowed',
              transition: 'background-color 0.2s',
            }}
          >
            {submitting ? '⏳ Submitting...' : '🚀 Run Backtest'}
          </button>
        </div>
      )}
      
      {/* Help text */}
      <div style={{ marginTop: '20px', padding: '16px', backgroundColor: '#e7f3ff', borderRadius: '8px' }}>
        <h4 style={{ marginBottom: '8px' }}>📚 How to use</h4>
        <ol style={{ margin: 0, paddingLeft: '20px', lineHeight: 1.8 }}>
          <li>Download L2 orderbook snapshots and trades from <a href="https://tardis.dev" target="_blank" rel="noopener noreferrer">Tardis.dev</a></li>
          <li>Drag and drop both <code>*_trades.csv.gz</code> and <code>*_orderbook.csv.gz</code> files</li>
          <li>Configure backtest parameters (capital, commission, strategy)</li>
          <li>Click "Run Backtest" to start the simulation</li>
        </ol>
        <div style={{ marginTop: '10px', fontSize: '13px', color: '#666' }}>
          Supported exchanges: Binance.US, Binance, Kraken, Coinbase, OKX, Bybit
        </div>
      </div>
    </div>
  );
};

export default TardisUpload;
