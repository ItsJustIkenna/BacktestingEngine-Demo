import React, { useState, useEffect } from 'react';
import { apiClient, ApiError } from '../utils/api';
import { validateBacktestParams, sanitizeString } from '../utils/validation';
import { useAbortController } from '../utils/hooks';

interface BacktestFormData {
  symbol: string;
  exchange: string;
  strategy_type: string;
  initial_capital: number;
  optimization_method: 'genetic' | 'manual';
  population_size: number;
  generations: number;
  risk_aversion: number;
  inventory_target: number;
  order_size: number;
  window_ms: number;
  min_quote_lifetime_ms: number;
  fee_rate: number;
  latency_buffer: number;
  volatility_cap: number;
  time_horizon_minutes: number;
  market_depth_k: number;
  volatility_adjustment_factor: number;
  inventory_penalty_factor: number;
  trend_sensitivity: number;
  flow_sensitivity: number;
}

interface BacktestFormProps {
  onJobSubmit?: (jobId: string) => void;
}

export const BacktestForm: React.FC<BacktestFormProps> = ({ onJobSubmit }) => {
  const { getSignal, abort } = useAbortController();
  
  const [formData, setFormData] = useState<BacktestFormData>({
    symbol: '',
    exchange: '',
    strategy_type: 'market_making',
    initial_capital: 100000,
    optimization_method: 'genetic',
    population_size: 50,
    generations: 20,
    risk_aversion: 0.5,
    inventory_target: 0.5,
    order_size: 0.01,
    window_ms: 5000,
    min_quote_lifetime_ms: 100,
    fee_rate: 0.001,
    latency_buffer: 10,
    volatility_cap: 0.05,
    time_horizon_minutes: 60,
    market_depth_k: 1.5,
    volatility_adjustment_factor: 0.5,
    inventory_penalty_factor: 0.3,
    trend_sensitivity: 0.2,
    flow_sensitivity: -0.1,
  });

  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [validationErrors, setValidationErrors] = useState<string[]>([]);
  const [jobId, setJobId] = useState<string | null>(null);
  
  const [symbols, setSymbols] = useState<string[]>([]);
  const [exchanges, setExchanges] = useState<string[]>([]);
  const [loadingMetadata, setLoadingMetadata] = useState(true);

  useEffect(() => {
    const fetchMetadata = async () => {
      try {
        const [symbolsData, exchangesData] = await Promise.all([
          apiClient.get<{ data: Array<{ symbol: string }> }>('/symbols', getSignal()),
          apiClient.get<{ data: Array<{ exchange: string }> }>('/exchanges', getSignal()),
        ]);

        const symbolList = symbolsData.data.map((item) => sanitizeString(item.symbol || ''));
        const exchangeList = exchangesData.data.map((item) => sanitizeString(item.exchange || ''));
        
        setSymbols(symbolList);
        setExchanges(exchangeList);
        
        if (symbolList.length > 0 && symbolList[0]) {
          setFormData(prev => ({ ...prev, symbol: symbolList[0] || '' }));
        }
        if (exchangeList.length > 0 && exchangeList[0]) {
          setFormData(prev => ({ ...prev, exchange: exchangeList[0] || '' }));
        }
      } catch (err) {
        if ((err as ApiError).message !== 'The user aborted a request.') {
          console.error('Failed to fetch metadata:', err);
        }
      } finally {
        setLoadingMetadata(false);
      }
    };

    fetchMetadata();
    
    return () => {
      abort();
    };
  }, [getSignal, abort]);

  const handleChange = (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement>) => {
    const { name, value, type } = e.target;
    setFormData(prev => ({
      ...prev,
      [name]: type === 'number' ? parseFloat(value) : value,
    }));
    
    // Clear validation errors when user changes input
    if (validationErrors.length > 0) {
      setValidationErrors([]);
    }
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    setError(null);
    setJobId(null);
    setValidationErrors([]);

    try {
      const params: any = {
        symbol: formData.symbol,
        exchange: formData.exchange,
        strategy_type: formData.strategy_type,
        initial_capital: formData.initial_capital,
        optimization_method: formData.optimization_method,
      };

      if (formData.optimization_method === 'genetic') {
        params.population_size = formData.population_size;
        params.generations = formData.generations;
      } else {
        params.risk_aversion = formData.risk_aversion;
        params.inventory_target = formData.inventory_target;
        params.order_size = formData.order_size;
        params.window_ms = formData.window_ms;
        params.min_quote_lifetime_ms = formData.min_quote_lifetime_ms;
        params.fee_rate = formData.fee_rate;
        params.latency_buffer = formData.latency_buffer;
        params.volatility_cap = formData.volatility_cap;
        params.time_horizon_minutes = formData.time_horizon_minutes;
        params.market_depth_k = formData.market_depth_k;
        params.volatility_adjustment_factor = formData.volatility_adjustment_factor;
        params.inventory_penalty_factor = formData.inventory_penalty_factor;
        params.trend_sensitivity = formData.trend_sensitivity;
        params.flow_sensitivity = formData.flow_sensitivity;
      }

      // Validate parameters
      const validation = validateBacktestParams(params);
      if (!validation.valid) {
        setValidationErrors(validation.errors);
        return;
      }

      const result = await apiClient.post<{ job_id: string }>('/backtest', params, getSignal());
      setJobId(result.job_id);
      
      if (onJobSubmit) {
        onJobSubmit(result.job_id);
      }
    } catch (err) {
      const apiError = err as ApiError;
      setError(sanitizeString(apiError.message || 'An error occurred'));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="backtest-form-container" style={{ maxWidth: '600px', margin: '0 auto', padding: '20px' }}>
      <h2>Create New Backtest</h2>
      
      <form onSubmit={handleSubmit} style={{ display: 'flex', flexDirection: 'column', gap: '15px' }}>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '10px' }}>
          <div>
            <label htmlFor="symbol" style={{ display: 'block', marginBottom: '5px' }}>
              Symbol
            </label>
            <select
              id="symbol"
              name="symbol"
              value={formData.symbol}
              onChange={handleChange}
              required
              disabled={loadingMetadata}
              style={{ width: '100%', padding: '8px', border: '1px solid #ccc', borderRadius: '4px' }}
            >
              {loadingMetadata ? (
                <option>Loading...</option>
              ) : (
                <>
                  {symbols.length === 0 && <option value="">No symbols available</option>}
                  {symbols.map(symbol => (
                    <option key={symbol} value={symbol}>{symbol}</option>
                  ))}
                </>
              )}
            </select>
          </div>
          
          <div>
            <label htmlFor="exchange" style={{ display: 'block', marginBottom: '5px' }}>
              Exchange
            </label>
            <select
              id="exchange"
              name="exchange"
              value={formData.exchange}
              onChange={handleChange}
              required
              disabled={loadingMetadata}
              style={{ width: '100%', padding: '8px', border: '1px solid #ccc', borderRadius: '4px' }}
            >
              {loadingMetadata ? (
                <option>Loading...</option>
              ) : (
                <>
                  {exchanges.length === 0 && <option value="">No exchanges available</option>}
                  {exchanges.map(exchange => (
                    <option key={exchange} value={exchange}>{exchange}</option>
                  ))}
                </>
              )}
            </select>
          </div>
        </div>

        <div>
          <label htmlFor="strategy_type" style={{ display: 'block', marginBottom: '5px' }}>
            Strategy Type
          </label>
          <select
            id="strategy_type"
            name="strategy_type"
            value={formData.strategy_type}
            onChange={handleChange}
            required
            style={{ width: '100%', padding: '8px', border: '1px solid #ccc', borderRadius: '4px' }}
          >
            <option value="market_making">Avellaneda-Stoikov (Market Making)</option>
          </select>
        </div>

        <div>
          <label htmlFor="initial_capital" style={{ display: 'block', marginBottom: '5px' }}>
            Initial Capital ($)
          </label>
          <input
            type="number"
            id="initial_capital"
            name="initial_capital"
            value={formData.initial_capital}
            onChange={handleChange}
            step="1000"
            min="1000"
            required
            style={{ width: '100%', padding: '8px', border: '1px solid #ccc', borderRadius: '4px' }}
          />
        </div>

        <div style={{ 
          padding: '15px', 
          backgroundColor: '#f8f9fa', 
          borderRadius: '4px', 
          border: '1px solid #dee2e6' 
        }}>
          <label style={{ display: 'block', marginBottom: '10px', fontWeight: 'bold' }}>
            Optimization Method
          </label>
          
          <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
            <label style={{ display: 'flex', alignItems: 'center', cursor: 'pointer' }}>
              <input
                type="radio"
                name="optimization_method"
                value="genetic"
                checked={formData.optimization_method === 'genetic'}
                onChange={handleChange}
                style={{ marginRight: '8px' }}
              />
              <div>
                <strong>Genetic Algorithm (Recommended)</strong>
                <div style={{ fontSize: '12px', color: '#6c757d', marginTop: '2px' }}>
                  Automatically finds optimal parameters. Takes longer but maximizes performance.
                </div>
              </div>
            </label>

            <label style={{ display: 'flex', alignItems: 'center', cursor: 'pointer' }}>
              <input
                type="radio"
                name="optimization_method"
                value="manual"
                checked={formData.optimization_method === 'manual'}
                onChange={handleChange}
                style={{ marginRight: '8px' }}
              />
              <div>
                <strong>Manual Parameters (Advanced)</strong>
                <div style={{ fontSize: '12px', color: '#6c757d', marginTop: '2px' }}>
                  Specify exact parameter values. Faster but requires expertise.
                </div>
              </div>
            </label>
          </div>

          {formData.optimization_method === 'genetic' && (
            <div style={{ 
              marginTop: '15px', 
              paddingTop: '15px', 
              borderTop: '1px solid #dee2e6',
              display: 'grid',
              gridTemplateColumns: '1fr 1fr',
              gap: '10px'
            }}>
              <div>
                <label htmlFor="population_size" style={{ display: 'block', marginBottom: '5px', fontSize: '14px' }}>
                  Population Size
                </label>
                <input
                  type="number"
                  id="population_size"
                  name="population_size"
                  value={formData.population_size}
                  onChange={handleChange}
                  min="5"
                  max="100"
                  step="5"
                  style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px' }}
                />
              </div>

              <div>
                <label htmlFor="generations" style={{ display: 'block', marginBottom: '5px', fontSize: '14px' }}>
                  Generations
                </label>
                <input
                  type="number"
                  id="generations"
                  name="generations"
                  value={formData.generations}
                  onChange={handleChange}
                  min="3"
                  max="50"
                  step="1"
                  style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px' }}
                />
              </div>
            </div>
          )}
        </div>

        <button
          type="submit"
          disabled={loading}
          style={{
            padding: '12px',
            backgroundColor: loading ? '#ccc' : '#007bff',
            color: 'white',
            border: 'none',
            borderRadius: '4px',
            cursor: loading ? 'not-allowed' : 'pointer',
            fontSize: '16px',
            fontWeight: 'bold',
          }}
        >
          {loading ? 'Submitting...' : 'Submit Backtest'}
        </button>

        {validationErrors.length > 0 && (
          <div style={{
            padding: '10px',
            backgroundColor: '#fff3cd',
            border: '1px solid #ffc107',
            borderRadius: '4px',
            color: '#856404',
          }}>
            <strong>Validation Errors:</strong>
            <ul style={{ margin: '5px 0 0 20px', padding: 0 }}>
              {validationErrors.map((err, idx) => (
                <li key={idx}>{err}</li>
              ))}
            </ul>
          </div>
        )}

        {error && (
          <div style={{
            padding: '10px',
            backgroundColor: '#f8d7da',
            border: '1px solid #f5c6cb',
            borderRadius: '4px',
            color: '#721c24',
          }}>
            <strong>Error:</strong> {error}
          </div>
        )}

        {jobId && (
          <div style={{
            padding: '10px',
            backgroundColor: '#d4edda',
            border: '1px solid #c3e6cb',
            borderRadius: '4px',
            color: '#155724',
          }}>
            <strong>Success!</strong> Backtest submitted with Job ID: <code>{sanitizeString(jobId)}</code>
          </div>
        )}
      </form>
    </div>
  );
};

export default BacktestForm;
