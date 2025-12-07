import React, { useState, useEffect } from 'react';

interface BacktestFormData {
  symbol: string;
  exchange: string;
  strategy_type: string;
  initial_capital: number;
  optimization_method: 'genetic' | 'manual';
  // Genetic algorithm parameters
  population_size: number;
  generations: number;
  // Monte Carlo configuration
  monte_carlo_enabled: boolean;
  monte_carlo_runs: number;
  // Walk-Forward configuration
  walk_forward_enabled: boolean;
  // Manual parameters (14 total for Avellaneda-Stoikov)
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
  // Phase 2 adjustment factors
  volatility_adjustment_factor: number;
  inventory_penalty_factor: number;
  trend_sensitivity: number;
  flow_sensitivity: number;
}

interface BacktestFormProps {
  onJobSubmit?: (jobId: string) => void;
}

export const BacktestForm: React.FC<BacktestFormProps> = ({ onJobSubmit }) => {
  const [formData, setFormData] = useState<BacktestFormData>({
    symbol: '',
    exchange: '',
    strategy_type: 'market_making',
    initial_capital: 10000,
    optimization_method: 'genetic',
    // Genetic algorithm defaults
    population_size: 50,
    generations: 20,
    // Monte Carlo defaults
    monte_carlo_enabled: true,
    monte_carlo_runs: 1000,
    // Walk-Forward defaults
    walk_forward_enabled: true,
    // Manual parameter defaults
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
  const [jobId, setJobId] = useState<string | null>(null);
  
  // State for dropdown options
  const [symbols, setSymbols] = useState<string[]>([]);
  const [exchanges, setExchanges] = useState<string[]>([]);
  const [loadingMetadata, setLoadingMetadata] = useState(true);

  // Fetch symbols and exchanges on component mount
  useEffect(() => {
    const fetchMetadata = async () => {
      try {
        const [symbolsRes, exchangesRes] = await Promise.all([
          fetch('/api/symbols'),
          fetch('/api/exchanges'),
        ]);

        if (symbolsRes.ok) {
          const symbolsData = await symbolsRes.json();
          const symbolList = symbolsData.data.map((item: any) => item.symbol);
          setSymbols(symbolList);
          if (symbolList.length > 0) {
            setFormData(prev => ({ ...prev, symbol: symbolList[0] }));
          }
        }

        if (exchangesRes.ok) {
          const exchangesData = await exchangesRes.json();
          const exchangeList = exchangesData.data.map((item: any) => item.exchange);
          setExchanges(exchangeList);
          if (exchangeList.length > 0) {
            setFormData(prev => ({ ...prev, exchange: exchangeList[0] }));
          }
        }
      } catch (err) {
        console.error('Failed to fetch metadata:', err);
      } finally {
        setLoadingMetadata(false);
      }
    };

    fetchMetadata();
  }, []);

  const handleChange = (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement>) => {
    const { name, value, type } = e.target;
    setFormData(prev => ({
      ...prev,
      [name]: type === 'number' ? parseFloat(value) : value,
    }));
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    setError(null);
    setJobId(null);

    try {
      // Build parameters object based on optimization method
      const params: any = {
        symbol: formData.symbol,
        exchange: formData.exchange,
        strategy_type: formData.strategy_type,
        initial_capital: formData.initial_capital,
        optimization_method: formData.optimization_method,
        // Monte Carlo and Walk-Forward settings
        monte_carlo_enabled: formData.monte_carlo_enabled,
        monte_carlo_runs: formData.monte_carlo_runs,
        walk_forward_enabled: formData.walk_forward_enabled,
      };

      if (formData.optimization_method === 'genetic') {
        params.population_size = formData.population_size;
        params.generations = formData.generations;
      } else {
        // Manual mode - send all 14 parameters
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

      const response = await fetch('/api/backtest', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(params),
      });

      if (!response.ok) {
        const errorData = await response.json();
        throw new Error(errorData.error || 'Failed to submit backtest');
      }

      const result = await response.json();
      setJobId(result.job_id);
      
      if (onJobSubmit) {
        onJobSubmit(result.job_id);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'An error occurred');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="backtest-form-container" style={{ maxWidth: '600px', margin: '0 auto', padding: '20px' }}>
      <h2>Create New Backtest</h2>
      
      <form onSubmit={handleSubmit} style={{ display: 'flex', flexDirection: 'column', gap: '15px' }}>
        {/* Symbol and Exchange */}
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

        {/* Strategy Type */}
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

        {/* Initial Capital */}
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

        {/* Optimization Method */}
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

          {/* Genetic Algorithm Options */}
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
                <div style={{ fontSize: '11px', color: '#6c757d', marginTop: '2px' }}>
                  Default: 50
                </div>
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
                <div style={{ fontSize: '11px', color: '#6c757d', marginTop: '2px' }}>
                  Default: 20
                </div>
              </div>
            </div>
          )}

          {/* Advanced Analysis Settings */}
          <div style={{ 
            marginTop: '15px', 
            paddingTop: '15px', 
            borderTop: '1px solid #dee2e6'
          }}>
            <h4 style={{ margin: '0 0 10px 0', fontSize: '14px' }}>🔬 Advanced Analysis</h4>
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '15px' }}>
              {/* Monte Carlo Settings */}
              <div style={{ 
                padding: '10px', 
                backgroundColor: '#f8f9fa', 
                borderRadius: '4px',
                border: '1px solid #dee2e6'
              }}>
                <div style={{ display: 'flex', alignItems: 'center', marginBottom: '8px' }}>
                  <input
                    type="checkbox"
                    id="monte_carlo_enabled"
                    name="monte_carlo_enabled"
                    checked={formData.monte_carlo_enabled}
                    onChange={(e) => setFormData({...formData, monte_carlo_enabled: e.target.checked})}
                    style={{ marginRight: '8px' }}
                  />
                  <label htmlFor="monte_carlo_enabled" style={{ fontSize: '13px', fontWeight: 500 }}>
                    🎲 Monte Carlo Simulation
                  </label>
                </div>
                {formData.monte_carlo_enabled && (
                  <div>
                    <label htmlFor="monte_carlo_runs" style={{ display: 'block', marginBottom: '3px', fontSize: '12px' }}>
                      Number of Runs
                    </label>
                    <input
                      type="number"
                      id="monte_carlo_runs"
                      name="monte_carlo_runs"
                      value={formData.monte_carlo_runs}
                      onChange={handleChange}
                      min="100"
                      max="5000"
                      step="100"
                      style={{ width: '100%', padding: '4px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '12px' }}
                    />
                    <div style={{ fontSize: '10px', color: '#6c757d', marginTop: '2px' }}>
                      Validates strategy robustness (default: 500)
                    </div>
                  </div>
                )}
              </div>

              {/* Walk-Forward Settings */}
              <div style={{ 
                padding: '10px', 
                backgroundColor: '#f8f9fa', 
                borderRadius: '4px',
                border: '1px solid #dee2e6'
              }}>
                <div style={{ display: 'flex', alignItems: 'center' }}>
                  <input
                    type="checkbox"
                    id="walk_forward_enabled"
                    name="walk_forward_enabled"
                    checked={formData.walk_forward_enabled}
                    onChange={(e) => setFormData({...formData, walk_forward_enabled: e.target.checked})}
                    style={{ marginRight: '8px' }}
                  />
                  <label htmlFor="walk_forward_enabled" style={{ fontSize: '13px', fontWeight: 500 }}>
                    🔄 Walk-Forward Analysis
                  </label>
                </div>
                <div style={{ fontSize: '10px', color: '#6c757d', marginTop: '5px' }}>
                  Tests out-of-sample performance over rolling windows
                </div>
              </div>
            </div>
          </div>

          {/* Manual Parameters */}
          {formData.optimization_method === 'manual' && (
            <div style={{ 
              marginTop: '15px', 
              paddingTop: '15px', 
              borderTop: '1px solid #dee2e6'
            }}>
              <div style={{ marginBottom: '15px' }}>
                <h4 style={{ margin: '0 0 10px 0', fontSize: '14px' }}>Core Parameters</h4>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: '10px' }}>
                  <div>
                    <label htmlFor="risk_aversion" style={{ display: 'block', marginBottom: '3px', fontSize: '12px' }}>
                      Risk Aversion (γ)
                    </label>
                    <input
                      type="number"
                      id="risk_aversion"
                      name="risk_aversion"
                      value={formData.risk_aversion}
                      onChange={handleChange}
                      step="0.1"
                      min="0.1"
                      max="10"
                      style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '13px' }}
                    />
                  </div>
                  <div>
                    <label htmlFor="inventory_target" style={{ display: 'block', marginBottom: '3px', fontSize: '12px' }}>
                      Inventory Target (q*)
                    </label>
                    <input
                      type="number"
                      id="inventory_target"
                      name="inventory_target"
                      value={formData.inventory_target}
                      onChange={handleChange}
                      step="0.1"
                      min="-1"
                      max="1"
                      style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '13px' }}
                    />
                  </div>
                  <div>
                    <label htmlFor="order_size" style={{ display: 'block', marginBottom: '3px', fontSize: '12px' }}>
                      Order Size
                    </label>
                    <input
                      type="number"
                      id="order_size"
                      name="order_size"
                      value={formData.order_size}
                      onChange={handleChange}
                      step="0.001"
                      min="0.001"
                      style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '13px' }}
                    />
                  </div>
                </div>
              </div>

              <div style={{ marginBottom: '15px' }}>
                <h4 style={{ margin: '0 0 10px 0', fontSize: '14px' }}>Timing & Risk Parameters</h4>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: '10px' }}>
                  <div>
                    <label htmlFor="window_ms" style={{ display: 'block', marginBottom: '3px', fontSize: '12px' }}>
                      Window (ms)
                    </label>
                    <input
                      type="number"
                      id="window_ms"
                      name="window_ms"
                      value={formData.window_ms}
                      onChange={handleChange}
                      step="100"
                      min="100"
                      style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '13px' }}
                    />
                  </div>
                  <div>
                    <label htmlFor="min_quote_lifetime_ms" style={{ display: 'block', marginBottom: '3px', fontSize: '12px' }}>
                      Min Quote Life (ms)
                    </label>
                    <input
                      type="number"
                      id="min_quote_lifetime_ms"
                      name="min_quote_lifetime_ms"
                      value={formData.min_quote_lifetime_ms}
                      onChange={handleChange}
                      step="10"
                      min="10"
                      style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '13px' }}
                    />
                  </div>
                  <div>
                    <label htmlFor="fee_rate" style={{ display: 'block', marginBottom: '3px', fontSize: '12px' }}>
                      Fee Rate
                    </label>
                    <input
                      type="number"
                      id="fee_rate"
                      name="fee_rate"
                      value={formData.fee_rate}
                      onChange={handleChange}
                      step="0.0001"
                      min="0"
                      max="0.01"
                      style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '13px' }}
                    />
                  </div>
                </div>
              </div>

              <div style={{ marginBottom: '15px' }}>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr 1fr', gap: '10px' }}>
                  <div>
                    <label htmlFor="latency_buffer" style={{ display: 'block', marginBottom: '3px', fontSize: '12px' }}>
                      Latency Buffer (ms)
                    </label>
                    <input
                      type="number"
                      id="latency_buffer"
                      name="latency_buffer"
                      value={formData.latency_buffer}
                      onChange={handleChange}
                      step="1"
                      min="0"
                      style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '13px' }}
                    />
                  </div>
                  <div>
                    <label htmlFor="volatility_cap" style={{ display: 'block', marginBottom: '3px', fontSize: '12px' }}>
                      Volatility Cap
                    </label>
                    <input
                      type="number"
                      id="volatility_cap"
                      name="volatility_cap"
                      value={formData.volatility_cap}
                      onChange={handleChange}
                      step="0.01"
                      min="0"
                      style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '13px' }}
                    />
                  </div>
                  <div>
                    <label htmlFor="time_horizon_minutes" style={{ display: 'block', marginBottom: '3px', fontSize: '12px' }}>
                      Time Horizon (min)
                    </label>
                    <input
                      type="number"
                      id="time_horizon_minutes"
                      name="time_horizon_minutes"
                      value={formData.time_horizon_minutes}
                      onChange={handleChange}
                      step="1"
                      min="1"
                      style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '13px' }}
                    />
                  </div>
                  <div>
                    <label htmlFor="market_depth_k" style={{ display: 'block', marginBottom: '3px', fontSize: '12px' }}>
                      Market Depth (k)
                    </label>
                    <input
                      type="number"
                      id="market_depth_k"
                      name="market_depth_k"
                      value={formData.market_depth_k}
                      onChange={handleChange}
                      step="0.1"
                      min="0.1"
                      style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '13px' }}
                    />
                  </div>
                </div>
              </div>

              <div>
                <h4 style={{ margin: '0 0 10px 0', fontSize: '14px', color: '#6c757d' }}>
                  Phase 2 Adjustment Factors (Advanced)
                </h4>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '10px' }}>
                  <div>
                    <label htmlFor="volatility_adjustment_factor" style={{ display: 'block', marginBottom: '3px', fontSize: '12px' }}>
                      Volatility Adjustment
                    </label>
                    <input
                      type="number"
                      id="volatility_adjustment_factor"
                      name="volatility_adjustment_factor"
                      value={formData.volatility_adjustment_factor}
                      onChange={handleChange}
                      step="0.1"
                      min="0"
                      max="1"
                      style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '13px' }}
                    />
                  </div>
                  <div>
                    <label htmlFor="inventory_penalty_factor" style={{ display: 'block', marginBottom: '3px', fontSize: '12px' }}>
                      Inventory Penalty
                    </label>
                    <input
                      type="number"
                      id="inventory_penalty_factor"
                      name="inventory_penalty_factor"
                      value={formData.inventory_penalty_factor}
                      onChange={handleChange}
                      step="0.1"
                      min="0"
                      max="0.8"
                      style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '13px' }}
                    />
                  </div>
                  <div>
                    <label htmlFor="trend_sensitivity" style={{ display: 'block', marginBottom: '3px', fontSize: '12px' }}>
                      Trend Sensitivity
                    </label>
                    <input
                      type="number"
                      id="trend_sensitivity"
                      name="trend_sensitivity"
                      value={formData.trend_sensitivity}
                      onChange={handleChange}
                      step="0.1"
                      min="-0.5"
                      max="0.5"
                      style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '13px' }}
                    />
                  </div>
                  <div>
                    <label htmlFor="flow_sensitivity" style={{ display: 'block', marginBottom: '3px', fontSize: '12px' }}>
                      Flow Sensitivity
                    </label>
                    <input
                      type="number"
                      id="flow_sensitivity"
                      name="flow_sensitivity"
                      value={formData.flow_sensitivity}
                      onChange={handleChange}
                      step="0.1"
                      min="-0.3"
                      max="0.3"
                      style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '13px' }}
                    />
                  </div>
                </div>
              </div>
            </div>
          )}
        </div>

        {/* Submit Button */}
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

        {/* Error Message */}
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

        {/* Success Message */}
        {jobId && (
          <div style={{
            padding: '10px',
            backgroundColor: '#d4edda',
            border: '1px solid #c3e6cb',
            borderRadius: '4px',
            color: '#155724',
          }}>
            <strong>Success!</strong> Backtest submitted with Job ID: <code>{jobId}</code>
          </div>
        )}
      </form>
    </div>
  );
};

export default BacktestForm;
