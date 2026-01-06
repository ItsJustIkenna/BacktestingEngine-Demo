import React, { useState, useCallback, useRef, useEffect } from 'react';

// Types for Tardis file upload
interface UploadedFileInfo {
  file_id: string;
  original_name: string;
  file_type: 'trades' | 'orderbook' | 'incremental_l2';
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
  incremental_l2_file: UploadedFileInfo | null;
  start_time: string | null;
  end_time: string | null;
  created_at: string;
}

interface FileUploadResponse {
  success: boolean;
  file: UploadedFileInfo | null;
  session: SymbolUploadSession | null;
  error: string | null;
  details?: {
    detected_type?: string;
    detected_exchange?: string;
    detected_symbol?: string;
    rows_parsed?: number;
    sample_headers?: string[];
    parse_errors?: string[];
  };
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

// Error explanation mapping
const getErrorExplanation = (error: string): { title: string; explanation: string; suggestions: string[] } => {
  const errorLower = error.toLowerCase();
  
  if (errorLower.includes('header') || errorLower.includes('column')) {
    return {
      title: 'Invalid CSV Headers',
      explanation: 'The file headers don\'t match the expected Tardis format.',
      suggestions: [
        'Trades files should have: timestamp, side, price, amount (or similar)',
        'Orderbook files should have: timestamp, bid_price, bid_size, ask_price, ask_size',
        'Check you downloaded the correct data type from Tardis.dev',
      ],
    };
  }
  
  if (errorLower.includes('symbol') || errorLower.includes('detect')) {
    return {
      title: 'Could Not Detect Symbol/Exchange',
      explanation: 'Unable to determine the trading pair or exchange from the file.',
      suggestions: [
        'Ensure the filename contains the symbol (e.g., BTC-USD_trades.csv.gz)',
        'Or the file content should have a symbol column',
        'Supported format: EXCHANGE_SYMBOL_TYPE.csv.gz',
      ],
    };
  }
  
  if (errorLower.includes('empty') || errorLower.includes('no data')) {
    return {
      title: 'Empty or Corrupt File',
      explanation: 'The file appears to be empty or could not be read.',
      suggestions: [
        'Re-download the file from Tardis.dev',
        'Check the file is not corrupted (try opening in a text editor)',
        'Ensure the .gz compression is valid',
      ],
    };
  }
  
  if (errorLower.includes('decompress') || errorLower.includes('gzip')) {
    return {
      title: 'Decompression Error',
      explanation: 'Failed to decompress the .gz file.',
      suggestions: [
        'The file may be corrupted during download',
        'Try re-downloading from Tardis.dev',
        'Ensure the file has a .csv.gz extension',
      ],
    };
  }
  
  if (errorLower.includes('type') || errorLower.includes('unknown file')) {
    return {
      title: 'Unknown File Type',
      explanation: 'Could not determine if this is a trades or orderbook file.',
      suggestions: [
        'Trades files should be named *_trades.csv.gz',
        'Orderbook files should be named *_orderbook.csv.gz or *_incremental_l2.csv.gz',
        'Check the file contains the expected columns for that data type',
      ],
    };
  }
  
  return {
    title: 'Upload Error',
    explanation: error,
    suggestions: [
      'Check the file format matches Tardis.dev export format',
      'Try a different file or re-download',
      'Contact support if the issue persists',
    ],
  };
};

export const TardisUpload: React.FC = () => {
  // Upload state
  const [sessions, setSessions] = useState<SymbolUploadSession[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [uploadProgress, setUploadProgress] = useState<string | null>(null);
  const [uploadResult, setUploadResult] = useState<{
    success: boolean;
    message: string;
    errorDetails?: { title: string; explanation: string; suggestions: string[] };
    fileInfo?: UploadedFileInfo;
  } | null>(null);
  
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Load existing sessions on mount
  useEffect(() => {
    const loadSessions = async () => {
      try {
        const response = await fetch('/api/tardis/sessions');
        if (response.ok) {
          const data = await response.json();
          setSessions(data.sessions || []);
        }
      } catch (err) {
        console.error('Failed to load sessions:', err);
      }
    };
    loadSessions();
  }, []);

  // Handle file upload
  const uploadFile = useCallback(async (file: File) => {
    setUploading(true);
    setUploadResult(null);
    setUploadProgress(`Uploading ${file.name}...`);
    
    try {
      const formData = new FormData();
      formData.append('file', file);
      
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
        
        setUploadResult({
          success: true,
          message: `Successfully uploaded ${file.name}`,
          fileInfo: result.file || undefined,
        });
      } else {
        const errorDetails = getErrorExplanation(result.error || 'Unknown error');
        setUploadResult({
          success: false,
          message: result.error || 'Upload failed',
          errorDetails,
        });
      }
    } catch (err) {
      const errorDetails = getErrorExplanation(`Network error: ${err}`);
      setUploadResult({
        success: false,
        message: `Failed to upload: ${err}`,
        errorDetails,
      });
    } finally {
      setUploading(false);
      setUploadProgress(null);
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
      setUploadResult({
        success: false,
        message: 'No valid files found',
        errorDetails: {
          title: 'Invalid File Type',
          explanation: 'Only CSV and CSV.GZ files are supported.',
          suggestions: [
            'Download data from Tardis.dev in CSV format',
            'Files should have .csv or .csv.gz extension',
          ],
        },
      });
      return;
    }
    
    // Upload files sequentially
    for (const file of csvFiles) {
      await uploadFile(file);
    }
  }, [uploadFile]);

  // Handle file input change
  const handleFileSelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (!files) return;
    
    for (const file of Array.from(files)) {
      await uploadFile(file);
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
      }
    } catch (err) {
      console.error('Failed to delete session:', err);
    }
  };

  return (
    <div style={{ padding: '20px', maxWidth: '900px', margin: '0 auto' }}>
      <h2 style={{ marginBottom: '8px', display: 'flex', alignItems: 'center', gap: '10px' }}>
        <span>📁</span>
        <span>Upload Market Data</span>
      </h2>
      
      <p style={{ color: '#666', marginBottom: '20px', fontSize: '14px' }}>
        Upload Tardis.dev CSV files to make them available for backtesting. 
        Data is automatically imported into the database.
      </p>
      
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
          {uploading ? 'Uploading...' : 'Drag & Drop Tardis CSV Files'}
        </div>
        
        <div style={{ color: '#666', fontSize: '14px' }}>
          Supported files: <code>*_trades.csv.gz</code>, <code>*_orderbook.csv.gz</code>, <code>*_incremental_l2.csv.gz</code>
          <br />
          Exchange and symbol are auto-detected from file content
        </div>
        
        {uploadProgress && (
          <div style={{ marginTop: '15px', color: '#17a2b8', fontWeight: 500 }}>
            ⏳ {uploadProgress}
          </div>
        )}
      </div>
      
      {/* Upload Result */}
      {uploadResult && (
        <div style={{ 
          backgroundColor: uploadResult.success ? '#d4edda' : '#f8d7da',
          border: `1px solid ${uploadResult.success ? '#c3e6cb' : '#f5c6cb'}`,
          borderRadius: '8px',
          padding: '16px',
          marginBottom: '20px',
        }}>
          <div style={{ 
            display: 'flex', 
            alignItems: 'center', 
            gap: '8px',
            fontWeight: 600,
            color: uploadResult.success ? '#155724' : '#721c24',
            marginBottom: uploadResult.errorDetails ? '12px' : '0',
          }}>
            {uploadResult.success ? '✅' : '❌'} {uploadResult.success ? 'Success' : uploadResult.errorDetails?.title || 'Error'}
          </div>
          
          {uploadResult.success && uploadResult.fileInfo && (
            <div style={{ fontSize: '14px', color: '#155724' }}>
              <strong>{uploadResult.fileInfo.symbol}</strong> on <strong>{uploadResult.fileInfo.exchange}</strong>
              <br />
              Type: {uploadResult.fileInfo.file_type} • {formatNumber(uploadResult.fileInfo.row_count)} rows • {formatBytes(uploadResult.fileInfo.file_size_bytes)}
            </div>
          )}
          
          {uploadResult.errorDetails && (
            <div>
              <div style={{ color: '#721c24', marginBottom: '8px' }}>
                {uploadResult.errorDetails.explanation}
              </div>
              <div style={{ fontSize: '13px' }}>
                <strong>Suggestions:</strong>
                <ul style={{ margin: '4px 0 0 0', paddingLeft: '20px' }}>
                  {uploadResult.errorDetails.suggestions.map((s, i) => (
                    <li key={i} style={{ marginBottom: '2px' }}>{s}</li>
                  ))}
                </ul>
              </div>
            </div>
          )}
          
          <button
            onClick={() => setUploadResult(null)}
            style={{
              marginTop: '10px',
              padding: '4px 12px',
              fontSize: '12px',
              backgroundColor: 'transparent',
              border: `1px solid ${uploadResult.success ? '#155724' : '#721c24'}`,
              borderRadius: '4px',
              color: uploadResult.success ? '#155724' : '#721c24',
              cursor: 'pointer',
            }}
          >
            Dismiss
          </button>
        </div>
      )}
      
      {/* Uploaded Sessions */}
      {sessions.length > 0 && (
        <div style={{ marginBottom: '20px' }}>
          <h3 style={{ marginBottom: '12px' }}>📊 Uploaded Data Sets</h3>
          <p style={{ fontSize: '13px', color: '#666', marginBottom: '12px' }}>
            These datasets are available for backtesting. Go to <strong>Create Backtest</strong> to run a strategy on them.
          </p>
          
          <div style={{ display: 'grid', gap: '12px' }}>
            {sessions.map(session => {
              const orderbookSource = session.orderbook_file || session.incremental_l2_file;
              const isComplete = session.trades_file && orderbookSource;
              
              return (
                <div
                  key={session.session_id}
                  style={{
                    border: `2px solid ${isComplete ? '#28a745' : '#ffc107'}`,
                    borderRadius: '8px',
                    padding: '16px',
                    backgroundColor: 'white',
                  }}
                >
                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
                    <div style={{ flex: 1 }}>
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
                        {isComplete && (
                          <span style={{ 
                            fontSize: '12px', 
                            backgroundColor: '#28a745', 
                            color: 'white', 
                            padding: '2px 8px', 
                            borderRadius: '4px',
                            marginLeft: '8px'
                          }}>
                            Ready
                          </span>
                        )}
                      </div>
                      
                      {session.start_time && session.end_time && (
                        <div style={{ fontSize: '13px', color: '#666', marginBottom: '8px' }}>
                          📅 {session.start_time} → {session.end_time}
                        </div>
                      )}
                      
                      <div style={{ display: 'flex', gap: '20px', fontSize: '13px', flexWrap: 'wrap' }}>
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
                              ({formatNumber(session.trades_file.row_count)} rows)
                            </span>
                          )}
                        </div>
                        
                        {/* Orderbook file status */}
                        <div style={{ 
                          display: 'flex', 
                          alignItems: 'center', 
                          gap: '6px',
                          color: orderbookSource ? '#28a745' : '#dc3545'
                        }}>
                          {orderbookSource ? '✅' : '❌'} {session.incremental_l2_file ? 'L2 Updates' : 'Orderbook'}
                          {orderbookSource && (
                            <span style={{ color: '#666' }}>
                              ({formatNumber(orderbookSource.row_count)} {session.incremental_l2_file ? 'updates' : 'snapshots'})
                            </span>
                          )}
                        </div>
                      </div>
                      
                      {!isComplete && (
                        <div style={{ 
                          marginTop: '8px', 
                          fontSize: '12px', 
                          color: '#856404',
                          backgroundColor: '#fff3cd',
                          padding: '6px 10px',
                          borderRadius: '4px',
                          display: 'inline-block'
                        }}>
                          ⚠️ Upload {!session.trades_file ? 'trades' : 'orderbook'} file to complete this dataset
                        </div>
                      )}
                    </div>
                    
                    <button
                      onClick={() => handleDeleteSession(session.session_id)}
                      style={{
                        background: 'none',
                        border: 'none',
                        cursor: 'pointer',
                        fontSize: '18px',
                        color: '#dc3545',
                        padding: '4px 8px',
                      }}
                      title="Delete dataset"
                    >
                      🗑️
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}
      
      {/* Help Section */}
      <div style={{ 
        marginTop: '20px', 
        padding: '16px', 
        backgroundColor: '#e7f3ff', 
        borderRadius: '8px',
        border: '1px solid #b3d9ff'
      }}>
        <h4 style={{ marginBottom: '12px', display: 'flex', alignItems: 'center', gap: '8px' }}>
          <span>📚</span> How to Get Data from Tardis.dev
        </h4>
        
        <ol style={{ margin: 0, paddingLeft: '20px', lineHeight: 1.8, fontSize: '14px' }}>
          <li>Go to <a href="https://tardis.dev/datasets" target="_blank" rel="noopener noreferrer">tardis.dev/datasets</a></li>
          <li>Select an exchange (Binance US, Kraken, Coinbase, etc.)</li>
          <li>Choose a trading pair (e.g., BTC-USD)</li>
          <li>Download both <strong>trades</strong> and <strong>book_snapshot</strong> (or <strong>incremental_book_L2</strong>) data</li>
          <li>Upload the .csv.gz files here</li>
        </ol>
        
        <div style={{ marginTop: '16px', padding: '12px', backgroundColor: '#fff', borderRadius: '6px' }}>
          <strong style={{ fontSize: '13px' }}>Expected File Format:</strong>
          <table style={{ width: '100%', marginTop: '8px', fontSize: '13px', borderCollapse: 'collapse' }}>
            <thead>
              <tr style={{ backgroundColor: '#f8f9fa' }}>
                <th style={{ padding: '6px 10px', textAlign: 'left', borderBottom: '1px solid #dee2e6' }}>File Type</th>
                <th style={{ padding: '6px 10px', textAlign: 'left', borderBottom: '1px solid #dee2e6' }}>Naming Pattern</th>
                <th style={{ padding: '6px 10px', textAlign: 'left', borderBottom: '1px solid #dee2e6' }}>Required Columns</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td style={{ padding: '6px 10px', borderBottom: '1px solid #eee' }}>Trades</td>
                <td style={{ padding: '6px 10px', borderBottom: '1px solid #eee' }}><code>*_trades.csv.gz</code></td>
                <td style={{ padding: '6px 10px', borderBottom: '1px solid #eee' }}>timestamp, side, price, amount</td>
              </tr>
              <tr>
                <td style={{ padding: '6px 10px', borderBottom: '1px solid #eee' }}>Orderbook</td>
                <td style={{ padding: '6px 10px', borderBottom: '1px solid #eee' }}><code>*_orderbook.csv.gz</code></td>
                <td style={{ padding: '6px 10px', borderBottom: '1px solid #eee' }}>timestamp, bids, asks</td>
              </tr>
              <tr>
                <td style={{ padding: '6px 10px' }}>L2 Updates</td>
                <td style={{ padding: '6px 10px' }}><code>*_incremental_l2.csv.gz</code></td>
                <td style={{ padding: '6px 10px' }}>timestamp, side, price, amount, is_snapshot</td>
              </tr>
            </tbody>
          </table>
        </div>
        
        <div style={{ marginTop: '12px', fontSize: '13px', color: '#666' }}>
          <strong>Supported Exchanges:</strong> Binance US, Kraken, Coinbase, Binance, OKX, Bybit
        </div>
      </div>
    </div>
  );
};

export default TardisUpload;
