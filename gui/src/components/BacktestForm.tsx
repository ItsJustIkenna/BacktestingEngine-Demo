import React, { useState, useEffect } from 'react';

// Fitness weight presets
const FITNESS_PRESETS = {
  balanced: { sharpe: 10, sortino: 15, profit_factor: 15, win_rate: 10, net_profit: 35, calmar: 15 },
  maxSharpe: { sharpe: 35, sortino: 25, profit_factor: 10, win_rate: 5, net_profit: 15, calmar: 10 },
  maxProfit: { sharpe: 5, sortino: 10, profit_factor: 15, win_rate: 10, net_profit: 50, calmar: 10 },
  lowDrawdown: { sharpe: 15, sortino: 15, profit_factor: 15, win_rate: 10, net_profit: 15, calmar: 30 },
  highWinRate: { sharpe: 10, sortino: 10, profit_factor: 15, win_rate: 35, net_profit: 20, calmar: 10 },
};

interface FitnessWeights {
  sharpe: number;
  sortino: number;
  profit_factor: number;
  win_rate: number;
  net_profit: number;
  calmar: number;
}

// Portfolio asset: symbol + exchange pair with weight
interface PortfolioAsset {
  symbol: string;
  exchange: string;
  weight: number;
}

interface BacktestFormData {
  symbol: string;
  exchange: string;
  strategy_type: string;
  initial_capital: number;
  initial_inventory_pct: number;  // Percentage of capital as base asset (e.g., 0.5 = 50% BTC, 50% USD)
  initial_30d_volume: number;     // Starting 30-day volume for fee tier calculation
  optimization_method: 'genetic' | 'manual';
  // Portfolio mode - multi-symbol/multi-exchange backtesting
  portfolio_mode: boolean;
  portfolio_assets: PortfolioAsset[];  // Symbol+exchange pairs with weights
  // Legacy fields (deprecated)
  additional_symbols: string[];
  symbol_exchanges: { [symbol: string]: string };
  allocation_weights: number[];
  // Genetic algorithm parameters
  population_size: number;
  generations: number;
  // Monte Carlo configuration
  monte_carlo_enabled: boolean;
  monte_carlo_runs: number;
  // Walk-Forward configuration
  walk_forward_enabled: boolean;
  // Fitness function customization
  custom_fitness_weights: boolean;
  fitness_weights: FitnessWeights;
  // Profitability criteria for auto-saving strategies
  auto_save_profitable: boolean;
  min_sharpe_ratio: number;
  min_profit_factor: number;
  max_drawdown: number;
  min_win_rate: number;
  min_trades: number;
  min_sortino_ratio: number | null;
  min_calmar_ratio: number | null;
  min_total_pnl: number | null;
  // Manual parameters (14 total for Avellaneda-Stoikov)
  risk_aversion: number;
  inventory_target: number;
  order_size: number;
  window_ms: number;
  min_quote_lifetime_ms: number;
  fee_rate: number;
  volatility_cap: number;
  time_horizon_minutes: number;
  market_depth_k: number;
  // Phase 2 adjustment factors
  volatility_adjustment_factor: number;
  inventory_penalty_factor: number;
  trend_sensitivity: number;
  flow_sensitivity: number;
  // Smart Order Routing configuration
  routing_enabled: boolean;
  routing_mode: 'best_venue' | 'multi_venue' | 'arbitrage';
  routing_exchanges: string[];
  max_venues_per_order: number;
  min_venue_allocation: number;
  execution_style: 'aggressive' | 'passive' | 'balanced' | 'twap' | 'vwap';
  execution_duration_ms: number;
  min_arb_profit_bps: number;
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
    initial_inventory_pct: 0.5,   // Default 50% base asset
    initial_30d_volume: 0,        // Default 0 = use exchange default fee tier
    optimization_method: 'genetic',
    // Portfolio mode defaults
    portfolio_mode: false,
    portfolio_assets: [],         // Symbol+exchange pairs with weights
    additional_symbols: [],       // Legacy
    symbol_exchanges: {},         // Legacy
    allocation_weights: [],       // Legacy
    // Genetic algorithm defaults
    population_size: 50,
    generations: 20,
    // Monte Carlo defaults
    monte_carlo_enabled: true,
    monte_carlo_runs: 1000,
    // Walk-Forward defaults
    walk_forward_enabled: true,
    // Fitness weights defaults (balanced)
    custom_fitness_weights: false,
    fitness_weights: { ...FITNESS_PRESETS.balanced },
    // Profitability criteria defaults
    auto_save_profitable: true,
    min_sharpe_ratio: 1.0,
    min_profit_factor: 1.2,
    max_drawdown: 0.25,
    min_win_rate: 0.40,
    min_trades: 30,
    min_sortino_ratio: null,
    min_calmar_ratio: null,
    min_total_pnl: null,
    // Manual parameter defaults
    risk_aversion: 0.5,
    inventory_target: 0.5,
    order_size: 0.01,
    window_ms: 5000,
    min_quote_lifetime_ms: 100,
    fee_rate: 0.001,
    volatility_cap: 0.05,
    time_horizon_minutes: 60,
    market_depth_k: 1.5,
    volatility_adjustment_factor: 0.5,
    inventory_penalty_factor: 0.3,
    trend_sensitivity: 0.2,
    flow_sensitivity: -0.1,
    // Smart Order Routing defaults
    routing_enabled: false,
    routing_mode: 'best_venue',
    routing_exchanges: [],
    max_venues_per_order: 3,
    min_venue_allocation: 0.1,
    execution_style: 'balanced',
    execution_duration_ms: 1000,
    min_arb_profit_bps: 10,
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
        initial_inventory_pct: formData.initial_inventory_pct,
        initial_30d_volume: formData.initial_30d_volume,
        optimization_method: formData.optimization_method,
        // Monte Carlo and Walk-Forward settings
        monte_carlo_enabled: formData.monte_carlo_enabled,
        monte_carlo_runs: formData.monte_carlo_runs,
        walk_forward_enabled: formData.walk_forward_enabled,
        // Profitability criteria for auto-saving
        auto_save_profitable: formData.auto_save_profitable,
        min_sharpe_ratio: formData.min_sharpe_ratio,
        min_profit_factor: formData.min_profit_factor,
        max_drawdown: formData.max_drawdown,
        min_win_rate: formData.min_win_rate,
        min_trades: formData.min_trades,
        min_sortino_ratio: formData.min_sortino_ratio,
        min_calmar_ratio: formData.min_calmar_ratio,
        min_total_pnl: formData.min_total_pnl,
      };

      // Add portfolio mode parameters if enabled
      if (formData.portfolio_mode && formData.portfolio_assets.length > 0) {
        // Use new portfolio_assets format for symbol+exchange pairs
        params.portfolio_assets = formData.portfolio_assets.map(asset => ({
          symbol: asset.symbol,
          exchange: asset.exchange,
          weight: asset.weight > 0 ? asset.weight : null,  // null weight = equal distribution
        }));
      }

      if (formData.optimization_method === 'genetic') {
        params.population_size = formData.population_size;
        params.generations = formData.generations;
        
        // Include custom fitness weights if enabled
        if (formData.custom_fitness_weights) {
          // Normalize weights to sum to 1.0
          const total = Object.values(formData.fitness_weights).reduce((a, b) => a + b, 0);
          params.fitness_weights = {
            sharpe: formData.fitness_weights.sharpe / total,
            sortino: formData.fitness_weights.sortino / total,
            profit_factor: formData.fitness_weights.profit_factor / total,
            win_rate: formData.fitness_weights.win_rate / total,
            net_profit: formData.fitness_weights.net_profit / total,
            calmar: formData.fitness_weights.calmar / total,
          };
        }
      } else {
        // Manual mode - send all 14 parameters
        params.risk_aversion = formData.risk_aversion;
        params.inventory_target = formData.inventory_target;
        params.order_size = formData.order_size;
        params.window_ms = formData.window_ms;
        params.min_quote_lifetime_ms = formData.min_quote_lifetime_ms;
        params.fee_rate = formData.fee_rate;
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

        {/* Portfolio Mode Toggle */}
        <div style={{ padding: '15px', backgroundColor: '#f8f9fa', borderRadius: '8px', border: '1px solid #e9ecef' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '10px' }}>
            <input
              type="checkbox"
              id="portfolio_mode"
              checked={formData.portfolio_mode}
              onChange={(e) => setFormData(prev => ({ 
                ...prev, 
                portfolio_mode: e.target.checked,
                // Reset portfolio assets when disabling
                portfolio_assets: e.target.checked ? prev.portfolio_assets : [],
              }))}
              style={{ width: '18px', height: '18px' }}
            />
            <label htmlFor="portfolio_mode" style={{ fontWeight: 'bold', cursor: 'pointer' }}>
              Portfolio Mode (Multi-Symbol / Multi-Exchange Backtesting)
            </label>
          </div>
          
          {formData.portfolio_mode && (
            <div style={{ marginTop: '15px' }}>
              <p style={{ fontSize: '0.9em', color: '#666', marginBottom: '15px' }}>
                Add symbol+exchange pairs to your portfolio. You can add the same symbol from different exchanges 
                (e.g., BTC-USD on Kraken AND Coinbase) to compare performance.
              </p>
              
              {/* Add New Asset Row */}
              <div style={{ 
                display: 'grid', 
                gridTemplateColumns: '1fr 1fr 80px auto',
                gap: '10px',
                marginBottom: '15px',
                padding: '10px',
                backgroundColor: '#e7f3ff',
                borderRadius: '6px'
              }}>
                <select
                  id="new_asset_symbol"
                  style={{ padding: '8px', border: '1px solid #ccc', borderRadius: '4px' }}
                  disabled={loadingMetadata}
                >
                  {symbols.map(s => <option key={s} value={s}>{s}</option>)}
                </select>
                <select
                  id="new_asset_exchange"
                  style={{ padding: '8px', border: '1px solid #ccc', borderRadius: '4px' }}
                  disabled={loadingMetadata}
                >
                  {exchanges.map(e => <option key={e} value={e}>{e}</option>)}
                </select>
                <input
                  id="new_asset_weight"
                  type="number"
                  min="0"
                  max="1"
                  step="0.01"
                  placeholder="Weight"
                  defaultValue=""
                  style={{ padding: '8px', border: '1px solid #ccc', borderRadius: '4px' }}
                />
                <button
                  type="button"
                  onClick={() => {
                    const symEl = document.getElementById('new_asset_symbol') as HTMLSelectElement;
                    const exchEl = document.getElementById('new_asset_exchange') as HTMLSelectElement;
                    const weightEl = document.getElementById('new_asset_weight') as HTMLInputElement;
                    
                    const symbol = symEl?.value || symbols[0];
                    const exchange = exchEl?.value || exchanges[0];
                    const weight = parseFloat(weightEl?.value) || 0;
                    
                    // Check if this exact pair already exists
                    const exists = formData.portfolio_assets.some(
                      a => a.symbol === symbol && a.exchange === exchange
                    );
                    
                    if (!exists && symbol && exchange) {
                      setFormData(prev => ({
                        ...prev,
                        portfolio_assets: [...prev.portfolio_assets, { symbol, exchange, weight }]
                      }));
                      if (weightEl) weightEl.value = '';
                    }
                  }}
                  style={{
                    padding: '8px 16px',
                    backgroundColor: '#0066cc',
                    color: 'white',
                    border: 'none',
                    borderRadius: '4px',
                    cursor: 'pointer',
                    fontWeight: 500
                  }}
                >
                  + Add
                </button>
              </div>

              {/* Portfolio Assets List */}
              {formData.portfolio_assets.length > 0 && (
                <div style={{ marginBottom: '15px' }}>
                  <label style={{ display: 'block', marginBottom: '10px', fontWeight: 500 }}>
                    Portfolio Assets ({formData.portfolio_assets.length} pairs)
                  </label>
                  <div style={{ 
                    display: 'grid', 
                    gridTemplateColumns: '1fr 1fr 80px 40px',
                    gap: '5px',
                    fontSize: '0.9em'
                  }}>
                    {/* Header row */}
                    <div style={{ fontWeight: 500, padding: '5px', color: '#666' }}>Symbol</div>
                    <div style={{ fontWeight: 500, padding: '5px', color: '#666' }}>Exchange</div>
                    <div style={{ fontWeight: 500, padding: '5px', color: '#666' }}>Weight</div>
                    <div></div>
                    
                    {formData.portfolio_assets.map((asset, idx) => (
                      <React.Fragment key={`${asset.symbol}@${asset.exchange}`}>
                        <span style={{ 
                          padding: '8px 10px', 
                          backgroundColor: '#f0f0f0',
                          borderRadius: '4px',
                          fontWeight: 500
                        }}>
                          {asset.symbol}
                        </span>
                        <span style={{ 
                          padding: '8px 10px', 
                          backgroundColor: '#f8f8f8',
                          borderRadius: '4px',
                          color: '#666'
                        }}>
                          {asset.exchange}
                        </span>
                        <input
                          type="number"
                          min="0"
                          max="1"
                          step="0.01"
                          value={asset.weight || ''}
                          onChange={(e) => {
                            const newAssets = [...formData.portfolio_assets];
                            const currentAsset = newAssets[idx];
                            if (currentAsset) {
                              newAssets[idx] = { 
                                symbol: currentAsset.symbol, 
                                exchange: currentAsset.exchange, 
                                weight: parseFloat(e.target.value) || 0 
                              };
                              setFormData(prev => ({ ...prev, portfolio_assets: newAssets }));
                            }
                          }}
                          style={{ 
                            padding: '5px', 
                            border: '1px solid #ccc', 
                            borderRadius: '4px',
                            width: '100%'
                          }}
                          placeholder="Auto"
                        />
                        <button
                          type="button"
                          onClick={() => {
                            setFormData(prev => ({
                              ...prev,
                              portfolio_assets: prev.portfolio_assets.filter((_, i) => i !== idx)
                            }));
                          }}
                          style={{
                            padding: '5px 10px',
                            backgroundColor: '#dc3545',
                            color: 'white',
                            border: 'none',
                            borderRadius: '4px',
                            cursor: 'pointer'
                          }}
                          title="Remove asset"
                        >
                          ✕
                        </button>
                      </React.Fragment>
                    ))}
                  </div>
                  
                  {/* Weight summary */}
                  <div style={{ marginTop: '10px', fontSize: '0.85em', color: '#666' }}>
                    {(() => {
                      const totalWeight = formData.portfolio_assets.reduce((sum, a) => sum + (a.weight || 0), 0);
                      const assetsWithWeight = formData.portfolio_assets.filter(a => a.weight > 0).length;
                      const assetsWithoutWeight = formData.portfolio_assets.length - assetsWithWeight;
                      
                      if (assetsWithoutWeight === formData.portfolio_assets.length) {
                        return <span>⚖️ Equal weight will be applied: {(100 / formData.portfolio_assets.length).toFixed(1)}% each</span>;
                      } else if (Math.abs(totalWeight - 1) > 0.01) {
                        return <span style={{ color: '#cc0000' }}>⚠️ Weights sum to {(totalWeight * 100).toFixed(1)}% (should equal 100%)</span>;
                      } else {
                        return <span style={{ color: '#28a745' }}>✓ Weights sum to 100%</span>;
                      }
                    })()}
                  </div>
                  
                  {/* Quick actions */}
                  <div style={{ marginTop: '10px', display: 'flex', gap: '10px' }}>
                    <button
                      type="button"
                      onClick={() => {
                        const equalWeight = 1 / formData.portfolio_assets.length;
                        setFormData(prev => ({
                          ...prev,
                          portfolio_assets: prev.portfolio_assets.map(a => ({ ...a, weight: equalWeight }))
                        }));
                      }}
                      style={{
                        padding: '5px 12px',
                        backgroundColor: '#6c757d',
                        color: 'white',
                        border: 'none',
                        borderRadius: '4px',
                        cursor: 'pointer',
                        fontSize: '0.85em'
                      }}
                    >
                      ⚖️ Equal Weight
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setFormData(prev => ({ ...prev, portfolio_assets: [] }));
                      }}
                      style={{
                        padding: '5px 12px',
                        backgroundColor: '#dc3545',
                        color: 'white',
                        border: 'none',
                        borderRadius: '4px',
                        cursor: 'pointer',
                        fontSize: '0.85em'
                      }}
                    >
                      🗑️ Clear All
                    </button>
                  </div>
                </div>
              )}
              
              {formData.portfolio_assets.length === 0 && (
                <div style={{ 
                  padding: '20px', 
                  textAlign: 'center', 
                  backgroundColor: '#fff',
                  borderRadius: '6px',
                  border: '1px dashed #ccc',
                  color: '#999'
                }}>
                  No assets added yet. Use the form above to add symbol+exchange pairs.
                </div>
              )}
            </div>
          )}
        </div>

        {/* Smart Order Routing Configuration */}
        <div style={{ 
          padding: '15px', 
          backgroundColor: '#f5f0ff', 
          borderRadius: '8px',
          border: '1px solid #d0c4e8'
        }}>
          <div style={{ display: 'flex', alignItems: 'center', marginBottom: '15px' }}>
            <input
              type="checkbox"
              id="routing_enabled"
              name="routing_enabled"
              checked={formData.routing_enabled}
              onChange={(e) => setFormData(prev => ({ ...prev, routing_enabled: e.target.checked }))}
              style={{ marginRight: '10px' }}
            />
            <label htmlFor="routing_enabled" style={{ fontWeight: 'bold', cursor: 'pointer' }}>
              🔀 Enable Smart Order Routing
            </label>
          </div>

          {formData.routing_enabled && (
            <div style={{ display: 'grid', gap: '15px' }}>
              {/* Routing Mode */}
              <div>
                <label htmlFor="routing_mode" style={{ display: 'block', marginBottom: '5px' }}>
                  Routing Mode
                </label>
                <select
                  id="routing_mode"
                  name="routing_mode"
                  value={formData.routing_mode}
                  onChange={handleChange}
                  style={{ width: '100%', padding: '8px', border: '1px solid #ccc', borderRadius: '4px' }}
                >
                  <option value="best_venue">Best Venue - Route to single best venue</option>
                  <option value="multi_venue">Multi-Venue - Split across venues</option>
                  <option value="arbitrage">Arbitrage - Exploit price differences</option>
                </select>
              </div>

              {/* Execution Style */}
              <div>
                <label htmlFor="execution_style" style={{ display: 'block', marginBottom: '5px' }}>
                  Execution Style
                </label>
                <select
                  id="execution_style"
                  name="execution_style"
                  value={formData.execution_style}
                  onChange={handleChange}
                  style={{ width: '100%', padding: '8px', border: '1px solid #ccc', borderRadius: '4px' }}
                >
                  <option value="aggressive">Aggressive - Take liquidity immediately</option>
                  <option value="passive">Passive - Provide liquidity (maker)</option>
                  <option value="balanced">Balanced - Mix of aggressive and passive</option>
                  <option value="twap">TWAP - Time-weighted execution</option>
                  <option value="vwap">VWAP - Volume-weighted execution</option>
                </select>
              </div>

              {/* Multi-venue settings */}
              {formData.routing_mode === 'multi_venue' && (
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '10px' }}>
                  <div>
                    <label htmlFor="max_venues_per_order" style={{ display: 'block', marginBottom: '5px' }}>
                      Max Venues Per Order
                    </label>
                    <input
                      type="number"
                      id="max_venues_per_order"
                      name="max_venues_per_order"
                      value={formData.max_venues_per_order}
                      onChange={handleChange}
                      min="1"
                      max="10"
                      style={{ width: '100%', padding: '8px', border: '1px solid #ccc', borderRadius: '4px' }}
                    />
                  </div>
                  <div>
                    <label htmlFor="min_venue_allocation" style={{ display: 'block', marginBottom: '5px' }}>
                      Min Venue Allocation (%)
                    </label>
                    <input
                      type="number"
                      id="min_venue_allocation"
                      name="min_venue_allocation"
                      value={formData.min_venue_allocation * 100}
                      onChange={(e) => setFormData(prev => ({ 
                        ...prev, 
                        min_venue_allocation: parseFloat(e.target.value) / 100 
                      }))}
                      min="1"
                      max="50"
                      step="1"
                      style={{ width: '100%', padding: '8px', border: '1px solid #ccc', borderRadius: '4px' }}
                    />
                  </div>
                </div>
              )}

              {/* TWAP/VWAP duration */}
              {(formData.execution_style === 'twap' || formData.execution_style === 'vwap') && (
                <div>
                  <label htmlFor="execution_duration_ms" style={{ display: 'block', marginBottom: '5px' }}>
                    Execution Duration (seconds)
                  </label>
                  <input
                    type="number"
                    id="execution_duration_ms"
                    name="execution_duration_ms"
                    value={formData.execution_duration_ms / 1000}
                    onChange={(e) => setFormData(prev => ({ 
                      ...prev, 
                      execution_duration_ms: parseFloat(e.target.value) * 1000 
                    }))}
                    min="1"
                    max="3600"
                    step="1"
                    style={{ width: '100%', padding: '8px', border: '1px solid #ccc', borderRadius: '4px' }}
                  />
                </div>
              )}

              {/* Arbitrage settings */}
              {formData.routing_mode === 'arbitrage' && (
                <div>
                  <label htmlFor="min_arb_profit_bps" style={{ display: 'block', marginBottom: '5px' }}>
                    Min Arbitrage Profit (basis points)
                  </label>
                  <input
                    type="number"
                    id="min_arb_profit_bps"
                    name="min_arb_profit_bps"
                    value={formData.min_arb_profit_bps}
                    onChange={handleChange}
                    min="1"
                    max="100"
                    step="1"
                    style={{ width: '100%', padding: '8px', border: '1px solid #ccc', borderRadius: '4px' }}
                  />
                  <small style={{ color: '#666', fontSize: '12px' }}>
                    10 bps = 0.1% profit threshold
                  </small>
                </div>
              )}

              {/* Routing Exchanges */}
              <div>
                <label style={{ display: 'block', marginBottom: '5px' }}>
                  Routing Exchanges
                </label>
                <div style={{ 
                  display: 'grid', 
                  gridTemplateColumns: 'repeat(3, 1fr)', 
                  gap: '8px',
                  padding: '10px',
                  backgroundColor: '#fff',
                  borderRadius: '4px',
                  border: '1px solid #ccc'
                }}>
                  {['binance', 'coinbase', 'kraken', 'kucoin', 'okx', 'bybit'].map(exchange => (
                    <label key={exchange} style={{ display: 'flex', alignItems: 'center', cursor: 'pointer' }}>
                      <input
                        type="checkbox"
                        checked={formData.routing_exchanges.includes(exchange)}
                        onChange={(e) => {
                          if (e.target.checked) {
                            setFormData(prev => ({ 
                              ...prev, 
                              routing_exchanges: [...prev.routing_exchanges, exchange] 
                            }));
                          } else {
                            setFormData(prev => ({ 
                              ...prev, 
                              routing_exchanges: prev.routing_exchanges.filter(ex => ex !== exchange) 
                            }));
                          }
                        }}
                        style={{ marginRight: '6px' }}
                      />
                      {exchange.charAt(0).toUpperCase() + exchange.slice(1)}
                    </label>
                  ))}
                </div>
                <small style={{ color: '#666', fontSize: '12px' }}>
                  Select venues for order routing. Leave empty to use all available venues.
                </small>
              </div>
            </div>
          )}
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
            <option value="market_making">Market Making</option>
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

        {/* Portfolio Allocation */}
        <div style={{ 
          padding: '15px', 
          backgroundColor: '#f0f8ff', 
          borderRadius: '4px', 
          border: '1px solid #b8d4e8',
          marginTop: '10px'
        }}>
          <label style={{ display: 'block', marginBottom: '10px', fontWeight: 'bold' }}>
            💰 Portfolio Allocation
          </label>
          
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '15px' }}>
            <div>
              <label htmlFor="initial_inventory_pct" style={{ display: 'block', marginBottom: '5px', fontSize: '14px' }}>
                Base Asset Allocation (%)
              </label>
              <input
                type="number"
                id="initial_inventory_pct"
                name="initial_inventory_pct"
                value={formData.initial_inventory_pct * 100}
                onChange={(e) => setFormData(prev => ({ ...prev, initial_inventory_pct: parseFloat(e.target.value) / 100 }))}
                step="5"
                min="0"
                max="100"
                style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px' }}
              />
              <div style={{ fontSize: '11px', color: '#6c757d', marginTop: '2px' }}>
                % of capital as base asset (e.g., BTC). Rest is quote (USD). Default: 50%
              </div>
            </div>

            <div>
              <label htmlFor="initial_30d_volume" style={{ display: 'block', marginBottom: '5px', fontSize: '14px' }}>
                30-Day Volume ($)
              </label>
              <input
                type="number"
                id="initial_30d_volume"
                name="initial_30d_volume"
                value={formData.initial_30d_volume}
                onChange={handleChange}
                step="100000"
                min="0"
                style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px' }}
              />
              <div style={{ fontSize: '11px', color: '#6c757d', marginTop: '2px' }}>
                Starting 30-day volume for fee tier. 0 = default fees.
              </div>
            </div>
          </div>
          
          <div style={{ 
            marginTop: '10px', 
            padding: '8px', 
            backgroundColor: '#e8f4f8', 
            borderRadius: '4px',
            fontSize: '12px',
            color: '#495057'
          }}>
            <strong>Current allocation:</strong>{' '}
            ${((formData.initial_capital * formData.initial_inventory_pct)).toLocaleString()} in base asset,{' '}
            ${((formData.initial_capital * (1 - formData.initial_inventory_pct))).toLocaleString()} in quote
          </div>
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

          {/* Fitness Function Weights Section */}
          {formData.optimization_method === 'genetic' && (
            <div style={{ 
              marginTop: '15px', 
              padding: '15px', 
              backgroundColor: '#fff8e1', 
              borderRadius: '6px',
              border: '1px solid #ffb300'
            }}>
              <div style={{ display: 'flex', alignItems: 'center', marginBottom: '12px' }}>
                <input
                  type="checkbox"
                  id="custom_fitness_weights"
                  name="custom_fitness_weights"
                  checked={formData.custom_fitness_weights}
                  onChange={(e) => setFormData({...formData, custom_fitness_weights: e.target.checked})}
                  style={{ marginRight: '8px' }}
                />
                <label htmlFor="custom_fitness_weights" style={{ fontSize: '14px', fontWeight: 600, color: '#e65100' }}>
                  ⚖️ Custom Fitness Weights
                </label>
              </div>
              
              {formData.custom_fitness_weights && (
                <>
                  <div style={{ fontSize: '11px', color: '#bf360c', marginBottom: '12px' }}>
                    Adjust how the GA evaluates strategies. Weights are normalized automatically. Total: {
                      Object.values(formData.fitness_weights).reduce((a, b) => a + b, 0)
                    }%
                  </div>
                  
                  {/* Preset Buttons */}
                  <div style={{ marginBottom: '15px', display: 'flex', gap: '8px', flexWrap: 'wrap' }}>
                    <button
                      type="button"
                      onClick={() => setFormData({...formData, fitness_weights: {...FITNESS_PRESETS.balanced}})}
                      style={{ padding: '4px 10px', fontSize: '11px', borderRadius: '4px', border: '1px solid #ccc', cursor: 'pointer', backgroundColor: '#fff' }}
                    >
                      📊 Balanced
                    </button>
                    <button
                      type="button"
                      onClick={() => setFormData({...formData, fitness_weights: {...FITNESS_PRESETS.maxSharpe}})}
                      style={{ padding: '4px 10px', fontSize: '11px', borderRadius: '4px', border: '1px solid #ccc', cursor: 'pointer', backgroundColor: '#fff' }}
                    >
                      📈 Max Sharpe
                    </button>
                    <button
                      type="button"
                      onClick={() => setFormData({...formData, fitness_weights: {...FITNESS_PRESETS.maxProfit}})}
                      style={{ padding: '4px 10px', fontSize: '11px', borderRadius: '4px', border: '1px solid #ccc', cursor: 'pointer', backgroundColor: '#fff' }}
                    >
                      💰 Max Profit
                    </button>
                    <button
                      type="button"
                      onClick={() => setFormData({...formData, fitness_weights: {...FITNESS_PRESETS.lowDrawdown}})}
                      style={{ padding: '4px 10px', fontSize: '11px', borderRadius: '4px', border: '1px solid #ccc', cursor: 'pointer', backgroundColor: '#fff' }}
                    >
                      🛡️ Low Drawdown
                    </button>
                    <button
                      type="button"
                      onClick={() => setFormData({...formData, fitness_weights: {...FITNESS_PRESETS.highWinRate}})}
                      style={{ padding: '4px 10px', fontSize: '11px', borderRadius: '4px', border: '1px solid #ccc', cursor: 'pointer', backgroundColor: '#fff' }}
                    >
                      🎯 High Win Rate
                    </button>
                  </div>
                  
                  <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: '12px' }}>
                    <div>
                      <label style={{ display: 'block', marginBottom: '3px', fontSize: '11px', fontWeight: 500 }}>
                        Sharpe Ratio ({formData.fitness_weights.sharpe}%)
                      </label>
                      <input
                        type="range"
                        min="0"
                        max="50"
                        value={formData.fitness_weights.sharpe}
                        onChange={(e) => setFormData({...formData, fitness_weights: {...formData.fitness_weights, sharpe: parseInt(e.target.value)}})}
                        style={{ width: '100%' }}
                      />
                    </div>
                    <div>
                      <label style={{ display: 'block', marginBottom: '3px', fontSize: '11px', fontWeight: 500 }}>
                        Sortino Ratio ({formData.fitness_weights.sortino}%)
                      </label>
                      <input
                        type="range"
                        min="0"
                        max="50"
                        value={formData.fitness_weights.sortino}
                        onChange={(e) => setFormData({...formData, fitness_weights: {...formData.fitness_weights, sortino: parseInt(e.target.value)}})}
                        style={{ width: '100%' }}
                      />
                    </div>
                    <div>
                      <label style={{ display: 'block', marginBottom: '3px', fontSize: '11px', fontWeight: 500 }}>
                        Profit Factor ({formData.fitness_weights.profit_factor}%)
                      </label>
                      <input
                        type="range"
                        min="0"
                        max="50"
                        value={formData.fitness_weights.profit_factor}
                        onChange={(e) => setFormData({...formData, fitness_weights: {...formData.fitness_weights, profit_factor: parseInt(e.target.value)}})}
                        style={{ width: '100%' }}
                      />
                    </div>
                    <div>
                      <label style={{ display: 'block', marginBottom: '3px', fontSize: '11px', fontWeight: 500 }}>
                        Win Rate ({formData.fitness_weights.win_rate}%)
                      </label>
                      <input
                        type="range"
                        min="0"
                        max="50"
                        value={formData.fitness_weights.win_rate}
                        onChange={(e) => setFormData({...formData, fitness_weights: {...formData.fitness_weights, win_rate: parseInt(e.target.value)}})}
                        style={{ width: '100%' }}
                      />
                    </div>
                    <div>
                      <label style={{ display: 'block', marginBottom: '3px', fontSize: '11px', fontWeight: 500 }}>
                        Net Profit ({formData.fitness_weights.net_profit}%)
                      </label>
                      <input
                        type="range"
                        min="0"
                        max="60"
                        value={formData.fitness_weights.net_profit}
                        onChange={(e) => setFormData({...formData, fitness_weights: {...formData.fitness_weights, net_profit: parseInt(e.target.value)}})}
                        style={{ width: '100%' }}
                      />
                    </div>
                    <div>
                      <label style={{ display: 'block', marginBottom: '3px', fontSize: '11px', fontWeight: 500 }}>
                        Calmar Ratio ({formData.fitness_weights.calmar}%)
                      </label>
                      <input
                        type="range"
                        min="0"
                        max="50"
                        value={formData.fitness_weights.calmar}
                        onChange={(e) => setFormData({...formData, fitness_weights: {...formData.fitness_weights, calmar: parseInt(e.target.value)}})}
                        style={{ width: '100%' }}
                      />
                    </div>
                  </div>
                </>
              )}
            </div>
          )}

          {/* Profitability Criteria Section */}
          <div style={{ 
            marginTop: '15px', 
            padding: '15px', 
            backgroundColor: '#f0fff4', 
            borderRadius: '6px',
            border: '1px solid #38a169'
          }}>
            <div style={{ display: 'flex', alignItems: 'center', marginBottom: '12px' }}>
              <input
                type="checkbox"
                id="auto_save_profitable"
                name="auto_save_profitable"
                checked={formData.auto_save_profitable}
                onChange={(e) => setFormData({...formData, auto_save_profitable: e.target.checked})}
                style={{ marginRight: '8px' }}
              />
              <label htmlFor="auto_save_profitable" style={{ fontSize: '14px', fontWeight: 600, color: '#276749' }}>
                💾 Auto-Save Profitable Strategies
              </label>
            </div>
            
            {formData.auto_save_profitable && (
              <>
                <div style={{ fontSize: '11px', color: '#2f855a', marginBottom: '12px' }}>
                  Strategies meeting these criteria will be automatically saved to the database for future use
                </div>
                
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: '10px' }}>
                  <div>
                    <label htmlFor="min_sharpe_ratio" style={{ display: 'block', marginBottom: '3px', fontSize: '11px', fontWeight: 500 }}>
                      Min Sharpe Ratio
                    </label>
                    <input
                      type="number"
                      id="min_sharpe_ratio"
                      name="min_sharpe_ratio"
                      value={formData.min_sharpe_ratio}
                      onChange={handleChange}
                      step="0.1"
                      min="0"
                      max="5"
                      style={{ width: '100%', padding: '5px', border: '1px solid #9ae6b4', borderRadius: '4px', fontSize: '12px' }}
                    />
                  </div>
                  <div>
                    <label htmlFor="min_profit_factor" style={{ display: 'block', marginBottom: '3px', fontSize: '11px', fontWeight: 500 }}>
                      Min Profit Factor
                    </label>
                    <input
                      type="number"
                      id="min_profit_factor"
                      name="min_profit_factor"
                      value={formData.min_profit_factor}
                      onChange={handleChange}
                      step="0.1"
                      min="1"
                      max="10"
                      style={{ width: '100%', padding: '5px', border: '1px solid #9ae6b4', borderRadius: '4px', fontSize: '12px' }}
                    />
                  </div>
                  <div>
                    <label htmlFor="max_drawdown" style={{ display: 'block', marginBottom: '3px', fontSize: '11px', fontWeight: 500 }}>
                      Max Drawdown (%)
                    </label>
                    <input
                      type="number"
                      id="max_drawdown"
                      name="max_drawdown"
                      value={formData.max_drawdown * 100}
                      onChange={(e) => setFormData({...formData, max_drawdown: parseFloat(e.target.value) / 100})}
                      step="1"
                      min="5"
                      max="50"
                      style={{ width: '100%', padding: '5px', border: '1px solid #9ae6b4', borderRadius: '4px', fontSize: '12px' }}
                    />
                  </div>
                  <div>
                    <label htmlFor="min_win_rate" style={{ display: 'block', marginBottom: '3px', fontSize: '11px', fontWeight: 500 }}>
                      Min Win Rate (%)
                    </label>
                    <input
                      type="number"
                      id="min_win_rate"
                      name="min_win_rate"
                      value={formData.min_win_rate * 100}
                      onChange={(e) => setFormData({...formData, min_win_rate: parseFloat(e.target.value) / 100})}
                      step="1"
                      min="30"
                      max="80"
                      style={{ width: '100%', padding: '5px', border: '1px solid #9ae6b4', borderRadius: '4px', fontSize: '12px' }}
                    />
                  </div>
                  <div>
                    <label htmlFor="min_trades" style={{ display: 'block', marginBottom: '3px', fontSize: '11px', fontWeight: 500 }}>
                      Min Trades
                    </label>
                    <input
                      type="number"
                      id="min_trades"
                      name="min_trades"
                      value={formData.min_trades}
                      onChange={handleChange}
                      step="10"
                      min="10"
                      max="500"
                      style={{ width: '100%', padding: '5px', border: '1px solid #9ae6b4', borderRadius: '4px', fontSize: '12px' }}
                    />
                  </div>
                  <div>
                    <label htmlFor="min_total_pnl" style={{ display: 'block', marginBottom: '3px', fontSize: '11px', fontWeight: 500 }}>
                      Min PnL ($)
                    </label>
                    <input
                      type="number"
                      id="min_total_pnl"
                      name="min_total_pnl"
                      value={formData.min_total_pnl ?? 0}
                      onChange={(e) => setFormData({...formData, min_total_pnl: parseFloat(e.target.value) || null})}
                      step="100"
                      style={{ width: '100%', padding: '5px', border: '1px solid #9ae6b4', borderRadius: '4px', fontSize: '12px' }}
                    />
                  </div>
                </div>
              </>
            )}
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
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: '10px' }}>
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
