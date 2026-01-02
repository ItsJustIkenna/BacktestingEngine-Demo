import React, { useState, useEffect } from 'react';

// Fitness weight presets (must sum to 100)
const FITNESS_PRESETS = {
  balanced: { sharpe: 10, sortino: 10, profit_factor: 15, win_rate: 5, net_profit: 30, calmar: 10, num_trades: 20 },
  maxSharpe: { sharpe: 30, sortino: 25, profit_factor: 10, win_rate: 5, net_profit: 10, calmar: 10, num_trades: 10 },
  maxProfit: { sharpe: 5, sortino: 10, profit_factor: 15, win_rate: 5, net_profit: 40, calmar: 5, num_trades: 20 },
  lowDrawdown: { sharpe: 15, sortino: 15, profit_factor: 15, win_rate: 5, net_profit: 15, calmar: 25, num_trades: 10 },
  highFrequency: { sharpe: 10, sortino: 10, profit_factor: 10, win_rate: 5, net_profit: 15, calmar: 15, num_trades: 35 },
};

// Pie chart colors for each weight
const WEIGHT_COLORS: Record<string, string> = {
  sharpe: '#4CAF50',      // Green
  sortino: '#2196F3',     // Blue  
  profit_factor: '#FF9800', // Orange
  win_rate: '#9C27B0',    // Purple
  net_profit: '#F44336',  // Red
  calmar: '#00BCD4',      // Cyan
  num_trades: '#795548',  // Brown
};

const WEIGHT_LABELS: Record<string, string> = {
  sharpe: 'Sharpe',
  sortino: 'Sortino', 
  profit_factor: 'Profit Factor',
  win_rate: 'Win Rate',
  net_profit: 'Net Profit',
  calmar: 'Calmar',
  num_trades: 'Trade Freq',
};

// SVG Pie Chart Component
interface FitnessPieChartProps {
  weights: FitnessWeights;
  size?: number;
}

const FitnessPieChart: React.FC<FitnessPieChartProps> = ({ weights, size = 120 }) => {
  const total = Object.values(weights).reduce((a, b) => a + b, 0);
  if (total === 0) return null;
  
  const radius = size / 2 - 5;
  const centerX = size / 2;
  const centerY = size / 2;
  
  let currentAngle = -90; // Start from top
  
  const slices = (Object.keys(weights) as Array<keyof FitnessWeights>)
    .filter(key => weights[key] > 0)
    .map(key => {
      const value = weights[key];
      const percentage = (value / total) * 100;
      const angle = (value / total) * 360;
      
      const startAngle = currentAngle;
      const endAngle = currentAngle + angle;
      currentAngle = endAngle;
      
      const startRad = (startAngle * Math.PI) / 180;
      const endRad = (endAngle * Math.PI) / 180;
      
      const x1 = centerX + radius * Math.cos(startRad);
      const y1 = centerY + radius * Math.sin(startRad);
      const x2 = centerX + radius * Math.cos(endRad);
      const y2 = centerY + radius * Math.sin(endRad);
      
      const largeArc = angle > 180 ? 1 : 0;
      
      // For a single 100% slice, draw a circle
      if (percentage >= 99.9) {
        return {
          key,
          path: `M ${centerX} ${centerY - radius} A ${radius} ${radius} 0 1 1 ${centerX - 0.01} ${centerY - radius} Z`,
          color: WEIGHT_COLORS[key],
          percentage,
        };
      }
      
      const path = `M ${centerX} ${centerY} L ${x1} ${y1} A ${radius} ${radius} 0 ${largeArc} 1 ${x2} ${y2} Z`;
      
      return { key, path, color: WEIGHT_COLORS[key], percentage };
    });
  
  return (
    <svg width={size} height={size} style={{ display: 'block' }}>
      {slices.map(slice => (
        <path
          key={slice.key}
          d={slice.path}
          fill={slice.color}
          stroke="white"
          strokeWidth="1"
        >
          <title>{WEIGHT_LABELS[slice.key]}: {slice.percentage.toFixed(1)}%</title>
        </path>
      ))}
      {/* Center hole for donut effect */}
      <circle cx={centerX} cy={centerY} r={radius * 0.4} fill="white" />
      {/* Total in center */}
      <text 
        x={centerX} 
        y={centerY} 
        textAnchor="middle" 
        dominantBaseline="middle"
        style={{ fontSize: '12px', fontWeight: 'bold', fill: total === 100 ? '#4CAF50' : '#f44336' }}
      >
        {total}%
      </text>
    </svg>
  );
};

// Accordion Section Component
interface AccordionSectionProps {
  title: string;
  icon: string;
  isExpanded: boolean;
  onToggle: () => void;
  children: React.ReactNode;
  badge?: string;
  badgeColor?: string;
}

const AccordionSection: React.FC<AccordionSectionProps> = ({ 
  title, icon, isExpanded, onToggle, children, badge, badgeColor = '#007bff' 
}) => (
  <div style={{ 
    marginBottom: '12px',
    border: '1px solid #e0e0e0',
    borderRadius: '8px',
    overflow: 'hidden',
    backgroundColor: 'white'
  }}>
    <button
      type="button"
      onClick={onToggle}
      style={{
        width: '100%',
        padding: '14px 16px',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        backgroundColor: isExpanded ? '#f8f9fa' : 'white',
        border: 'none',
        cursor: 'pointer',
        textAlign: 'left',
        transition: 'background-color 0.2s'
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: '10px' }}>
        <span style={{ fontSize: '18px' }}>{icon}</span>
        <span style={{ fontSize: '15px', fontWeight: 600, color: '#333' }}>{title}</span>
        {badge && (
          <span style={{ 
            fontSize: '11px', 
            backgroundColor: badgeColor, 
            color: 'white', 
            padding: '2px 8px', 
            borderRadius: '10px',
            fontWeight: 500
          }}>
            {badge}
          </span>
        )}
      </div>
      <span style={{ 
        fontSize: '12px', 
        color: '#666',
        transform: isExpanded ? 'rotate(180deg)' : 'rotate(0)',
        transition: 'transform 0.2s'
      }}>
        ▼
      </span>
    </button>
    {isExpanded && (
      <div style={{ padding: '16px', borderTop: '1px solid #e0e0e0' }}>
        {children}
      </div>
    )}
  </div>
);

interface FitnessWeights {
  sharpe: number;
  sortino: number;
  profit_factor: number;
  win_rate: number;
  net_profit: number;
  calmar: number;
  num_trades: number;
}

// Portfolio asset: symbol + exchange pair (weights optimized by GA)
interface PortfolioAsset {
  symbol: string;
  exchange: string;
}

interface BacktestFormData {
  symbol: string;
  exchange: string;
  strategy_type: string;
  initial_capital: number;
  initial_inventory_pct: number;  // Percentage of capital as base asset (e.g., 0.5 = 50% BTC, 50% USD)
  initial_30d_volume: number;     // Starting 30-day volume for fee tier calculation
  optimization_method: 'genetic' | 'manual';
  // Infrastructure configuration
  is_colocated: boolean;  // Co-located with exchange (e.g., Beeks Group) for sub-ms latency
  fill_model: 'optimistic' | 'colocated' | 'standard' | 'realistic' | 'pessimistic';
  // Portfolio mode - multi-symbol/multi-exchange backtesting
  portfolio_assets: PortfolioAsset[];  // Symbol+exchange pairs (weights optimized by GA)
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
}

interface BacktestFormProps {
  onJobSubmit?: (jobId: string) => void;
}

export const BacktestForm: React.FC<BacktestFormProps> = ({ onJobSubmit }) => {
  // Accordion state - which sections are expanded
  const [expandedSections, setExpandedSections] = useState<Record<string, boolean>>({
    assets: true,      // Always start with assets expanded
    strategy: true,    // Strategy & routing visible by default
    capital: false,
    optimization: false,
    analysis: false,
  });

  const toggleSection = (section: string) => {
    setExpandedSections(prev => ({ ...prev, [section]: !prev[section] }));
  };

  const [formData, setFormData] = useState<BacktestFormData>({
    symbol: '',
    exchange: '',
    strategy_type: 'market_making',
    initial_capital: 10000,
    initial_inventory_pct: 0.0,
    initial_30d_volume: 0,
    optimization_method: 'genetic',
    // Infrastructure configuration
    is_colocated: false,
    fill_model: 'realistic',
    // Portfolio assets (always used, single asset is a 1-item portfolio)
    portfolio_assets: [],
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
      // Determine effective exchange name (append _colocated if enabled)
      const effectiveExchange = formData.is_colocated 
        ? `${formData.exchange}_colocated` 
        : formData.exchange;
      
      // Determine effective fill model (use colocated if infrastructure is co-located)
      const effectiveFillModel = formData.is_colocated 
        ? 'colocated' 
        : formData.fill_model;

      // Build parameters object based on optimization method
      const params: any = {
        symbol: formData.symbol,
        exchange: effectiveExchange,
        strategy_type: formData.strategy_type,
        initial_capital: formData.initial_capital,
        initial_inventory_pct: formData.initial_inventory_pct,
        initial_30d_volume: formData.initial_30d_volume,
        optimization_method: formData.optimization_method,
        fill_model: effectiveFillModel,
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
      // Always send portfolio_assets - if empty, use the primary symbol/exchange
      if (formData.portfolio_assets.length > 0) {
        // Use explicitly added portfolio assets (weight = null so GA optimizes)
        // Apply co-located suffix to all exchanges if enabled
        params.portfolio_assets = formData.portfolio_assets.map(asset => ({
          symbol: asset.symbol,
          exchange: formData.is_colocated ? `${asset.exchange}_colocated` : asset.exchange,
          weight: null,  // null weight = GA will optimize
        }));
      } else if (formData.symbol && formData.exchange) {
        // Fallback: use primary symbol/exchange as single-asset portfolio
        params.portfolio_assets = [{
          symbol: formData.symbol,
          exchange: effectiveExchange,
          weight: null,
        }];
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
            num_trades: formData.fitness_weights.num_trades / total,
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
    <div className="backtest-form-container" style={{ maxWidth: '700px', margin: '0 auto', padding: '20px' }}>
      <div style={{ 
        display: 'flex', 
        justifyContent: 'space-between', 
        alignItems: 'center',
        marginBottom: '20px'
      }}>
        <h2 style={{ margin: 0 }}>Create Backtest</h2>
        <div style={{ fontSize: '12px', color: '#666' }}>
          {formData.portfolio_assets.length} asset{formData.portfolio_assets.length !== 1 ? 's' : ''} configured
        </div>
      </div>
      
      <form onSubmit={handleSubmit}>
        {/* ==================== SECTION 1: PORTFOLIO ASSETS ==================== */}
        <AccordionSection
          title="Portfolio Assets"
          icon="📊"
          isExpanded={expandedSections.assets ?? true}
          onToggle={() => toggleSection('assets')}
          badge={formData.portfolio_assets.length > 0 ? `${formData.portfolio_assets.length} assets` : undefined}
          badgeColor="#28a745"
        >
          <p style={{ fontSize: '13px', color: '#666', margin: '0 0 15px 0' }}>
            Add the symbols and exchanges you want to backtest. Each asset can be on a different exchange.
          </p>
          
          {/* Add New Asset Row */}
          <div style={{ 
            display: 'grid', 
            gridTemplateColumns: '1fr 1fr auto',
            gap: '10px',
            marginBottom: '15px',
            padding: '12px',
            backgroundColor: '#f0f7ff',
            borderRadius: '8px',
            border: '1px solid #cce0ff'
          }}>
            <div>
              <label style={{ fontSize: '11px', color: '#666', display: 'block', marginBottom: '4px' }}>Symbol</label>
              <select
                id="new_asset_symbol"
                style={{ width: '100%', padding: '8px', border: '1px solid #ccc', borderRadius: '4px' }}
                disabled={loadingMetadata}
              >
                {loadingMetadata ? (
                  <option>Loading...</option>
                ) : (
                  symbols.map(s => <option key={s} value={s}>{s}</option>)
                )}
              </select>
            </div>
            <div>
              <label style={{ fontSize: '11px', color: '#666', display: 'block', marginBottom: '4px' }}>Exchange</label>
              <select
                id="new_asset_exchange"
                style={{ width: '100%', padding: '8px', border: '1px solid #ccc', borderRadius: '4px' }}
                disabled={loadingMetadata}
              >
                {loadingMetadata ? (
                  <option>Loading...</option>
                ) : (
                  exchanges.map(e => <option key={e} value={e}>{e}</option>)
                )}
              </select>
            </div>
            <div style={{ display: 'flex', alignItems: 'flex-end' }}>
              <button
                type="button"
                onClick={() => {
                  const symEl = document.getElementById('new_asset_symbol') as HTMLSelectElement;
                  const exchEl = document.getElementById('new_asset_exchange') as HTMLSelectElement;
                  
                  const symbol = symEl?.value || symbols[0];
                  const exchange = exchEl?.value || exchanges[0];
                  
                  const exists = formData.portfolio_assets.some(
                    a => a.symbol === symbol && a.exchange === exchange
                  );
                  
                  if (!exists && symbol && exchange) {
                    setFormData(prev => ({
                      ...prev,
                      portfolio_assets: [...prev.portfolio_assets, { symbol, exchange }]
                    }));
                  }
                }}
                style={{
                  padding: '8px 16px',
                  backgroundColor: '#0066cc',
                  color: 'white',
                  border: 'none',
                  borderRadius: '4px',
                  cursor: 'pointer',
                  fontWeight: 600,
                  fontSize: '14px'
                }}
              >
                + Add
              </button>
            </div>
          </div>

          {/* Portfolio Assets List */}
          {formData.portfolio_assets.length > 0 ? (
            <div>
              <div style={{ 
                display: 'grid', 
                gridTemplateColumns: '1fr 1fr 36px',
                gap: '8px',
                fontSize: '13px'
              }}>
                {/* Header */}
                <div style={{ fontWeight: 600, color: '#666', padding: '6px 0' }}>Symbol</div>
                <div style={{ fontWeight: 600, color: '#666', padding: '6px 0' }}>Exchange</div>
                <div></div>
                
                {formData.portfolio_assets.map((asset, idx) => (
                  <React.Fragment key={`${asset.symbol}@${asset.exchange}`}>
                    <div style={{ 
                      padding: '10px 12px', 
                      backgroundColor: '#f8f9fa',
                      borderRadius: '6px',
                      fontWeight: 600,
                      display: 'flex',
                      alignItems: 'center'
                    }}>
                      {asset.symbol}
                    </div>
                    <div style={{ 
                      padding: '10px 12px', 
                      backgroundColor: '#f8f9fa',
                      borderRadius: '6px',
                      color: '#555',
                      display: 'flex',
                      alignItems: 'center'
                    }}>
                      {asset.exchange}
                    </div>
                    <button
                      type="button"
                      onClick={() => {
                        const newAssets = formData.portfolio_assets.filter((_, i) => i !== idx);
                        setFormData(prev => ({
                          ...prev,
                          portfolio_assets: newAssets
                        }));
                      }}
                      style={{
                        padding: '8px',
                        backgroundColor: '#fee',
                        color: '#c00',
                        border: '1px solid #fcc',
                        borderRadius: '6px',
                        cursor: 'pointer',
                        fontSize: '14px'
                      }}
                      title="Remove"
                    >
                      ✕
                    </button>
                  </React.Fragment>
                ))}
              </div>
              
              {/* GA Info */}
              <div style={{ 
                marginTop: '12px', 
                padding: '10px 12px', 
                backgroundColor: '#e8f4fd', 
                borderRadius: '6px',
                fontSize: '12px',
                color: '#0066cc'
              }}>
                ⚙️ Weights will be optimized by the Genetic Algorithm
              </div>
            </div>
          ) : (
            <div style={{ 
              padding: '30px', 
              textAlign: 'center', 
              backgroundColor: '#f8f9fa',
              borderRadius: '8px',
              border: '2px dashed #dee2e6',
              color: '#666'
            }}>
              <div style={{ fontSize: '24px', marginBottom: '8px' }}>📈</div>
              <div>No assets added yet. Add at least one symbol+exchange pair above.</div>
            </div>
          )}
        </AccordionSection>

        {/* ==================== SECTION 2: STRATEGY ==================== */}
        <AccordionSection
          title="Strategy"
          icon="📊"
          isExpanded={expandedSections.strategy ?? true}
          onToggle={() => toggleSection('strategy')}
          badge="Market Making"
          badgeColor="#17a2b8"
        >
          <div style={{ display: 'grid', gap: '16px' }}>
            {/* Strategy Info */}
            <div style={{ 
              padding: '16px', 
              backgroundColor: '#f0f7ff', 
              borderRadius: '8px',
              border: '1px solid #cce5ff'
            }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: '12px', marginBottom: '12px' }}>
                <span style={{ fontSize: '24px' }}>📈</span>
                <div>
                  <div style={{ fontWeight: 600, fontSize: '15px', color: '#004085' }}>Avellaneda-Stoikov Market Making</div>
                  <div style={{ fontSize: '12px', color: '#666', marginTop: '2px' }}>
                    Optimal bid-ask quoting with inventory management
                  </div>
                </div>
              </div>
              <div style={{ fontSize: '12px', color: '#555', lineHeight: 1.5 }}>
                The strategy places limit orders on both sides of the order book, earning the bid-ask spread 
                while managing inventory risk. The Genetic Algorithm optimizes ~18 parameters per symbol 
                and capital allocation weights across your portfolio.
              </div>
            </div>

            {/* Active Exchanges (auto-populated from portfolio) */}
            <div style={{ 
              padding: '12px', 
              backgroundColor: '#f8f9fa', 
              borderRadius: '6px',
              border: '1px solid #e9ecef'
            }}>
              <div style={{ fontWeight: 500, marginBottom: '8px', fontSize: '13px' }}>
                📍 Active Exchanges:
              </div>
              {formData.portfolio_assets.length > 0 ? (
                <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px' }}>
                  {[...new Set(formData.portfolio_assets.map(a => a.exchange))].map(exchange => (
                    <span key={exchange} style={{
                      padding: '4px 10px',
                      backgroundColor: '#007bff',
                      color: 'white',
                      borderRadius: '12px',
                      fontSize: '12px',
                      fontWeight: 500
                    }}>
                      {exchange}
                    </span>
                  ))}
                </div>
              ) : (
                <div style={{ color: '#999', fontSize: '12px' }}>Add assets above to see active exchanges</div>
              )}
            </div>
          </div>
        </AccordionSection>

        {/* ==================== SECTION 3: CAPITAL & ALLOCATION ==================== */}
        <AccordionSection
          title="Capital & Allocation"
          icon="💰"
          isExpanded={expandedSections.capital ?? false}
          onToggle={() => toggleSection('capital')}
          badge={`$${formData.initial_capital.toLocaleString()}`}
          badgeColor="#28a745"
        >
          <div style={{ display: 'grid', gap: '16px' }}>
            {/* Initial Capital */}
            <div>
              <label style={{ display: 'block', marginBottom: '6px', fontWeight: 500 }}>Initial Capital ($)</label>
              <input
                type="number"
                name="initial_capital"
                value={formData.initial_capital}
                onChange={handleChange}
                step="1000"
                min="1000"
                style={{ width: '100%', padding: '10px', border: '1px solid #ccc', borderRadius: '6px', fontSize: '14px' }}
              />
            </div>

            {/* 30-Day Volume for Fee Tier */}
            <div>
              <label style={{ display: 'block', marginBottom: '6px', fontWeight: 500 }}>30-Day Trading Volume ($)</label>
              <input
                type="number"
                name="initial_30d_volume"
                value={formData.initial_30d_volume}
                onChange={handleChange}
                step="100000"
                min="0"
                style={{ width: '100%', padding: '10px', border: '1px solid #ccc', borderRadius: '6px' }}
              />
              <div style={{ fontSize: '11px', color: '#666', marginTop: '4px' }}>
                Your existing 30-day trading volume for fee tier calculation. 0 = default/highest fees.
              </div>
              
              {/* Fee Tier Presets */}
              <div style={{ marginTop: '10px' }}>
                <div style={{ fontSize: '12px', fontWeight: 500, marginBottom: '6px', color: '#495057' }}>Quick Select Fee Tier:</div>
                <div style={{ display: 'flex', gap: '6px', flexWrap: 'wrap' }}>
                  {[
                    { vol: 0, label: 'New', maker: '0.25%', color: '#dc3545' },
                    { vol: 100000, label: '$100K', maker: '0.12%', color: '#fd7e14' },
                    { vol: 1000000, label: '$1M', maker: '0.06%', color: '#ffc107' },
                    { vol: 5000000, label: '$5M', maker: '0.02%', color: '#28a745' },
                    { vol: 10000000, label: '$10M', maker: '0%', color: '#20c997' },
                  ].map(tier => (
                    <button
                      key={tier.vol}
                      type="button"
                      onClick={() => setFormData(prev => ({ ...prev, initial_30d_volume: tier.vol }))}
                      style={{
                        padding: '6px 10px',
                        border: formData.initial_30d_volume === tier.vol ? `2px solid ${tier.color}` : '1px solid #ddd',
                        borderRadius: '4px',
                        backgroundColor: formData.initial_30d_volume === tier.vol ? `${tier.color}15` : 'white',
                        cursor: 'pointer',
                        fontSize: '11px',
                        fontWeight: formData.initial_30d_volume === tier.vol ? 600 : 400,
                      }}
                    >
                      <div>{tier.label}</div>
                      <div style={{ color: tier.color, fontWeight: 600 }}>{tier.maker}</div>
                    </button>
                  ))}
                </div>
              </div>
              
              {/* Current Fee Display */}
              <div style={{ 
                marginTop: '10px', 
                padding: '10px', 
                backgroundColor: '#f8f9fa', 
                borderRadius: '6px',
                border: '1px solid #e9ecef'
              }}>
                <div style={{ fontSize: '12px', fontWeight: 500, marginBottom: '6px' }}>📊 Estimated Fees at Volume:</div>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '10px', fontSize: '12px' }}>
                  <div>
                    <span style={{ color: '#666' }}>Maker: </span>
                    <span style={{ fontWeight: 600, color: formData.initial_30d_volume >= 5000000 ? '#28a745' : formData.initial_30d_volume >= 100000 ? '#fd7e14' : '#dc3545' }}>
                      {formData.initial_30d_volume >= 10000000 ? '0.00%' :
                       formData.initial_30d_volume >= 5000000 ? '0.02%' :
                       formData.initial_30d_volume >= 2500000 ? '0.04%' :
                       formData.initial_30d_volume >= 1000000 ? '0.06%' :
                       formData.initial_30d_volume >= 500000 ? '0.08%' :
                       formData.initial_30d_volume >= 250000 ? '0.10%' :
                       formData.initial_30d_volume >= 100000 ? '0.12%' :
                       formData.initial_30d_volume >= 50000 ? '0.14%' :
                       formData.initial_30d_volume >= 10000 ? '0.20%' : '0.25%'}
                    </span>
                  </div>
                  <div>
                    <span style={{ color: '#666' }}>Taker: </span>
                    <span style={{ fontWeight: 600 }}>
                      {formData.initial_30d_volume >= 100000000 ? '0.08%' :
                       formData.initial_30d_volume >= 10000000 ? '0.10%' :
                       formData.initial_30d_volume >= 5000000 ? '0.12%' :
                       formData.initial_30d_volume >= 2500000 ? '0.14%' :
                       formData.initial_30d_volume >= 1000000 ? '0.16%' :
                       formData.initial_30d_volume >= 500000 ? '0.18%' :
                       formData.initial_30d_volume >= 250000 ? '0.20%' :
                       formData.initial_30d_volume >= 100000 ? '0.22%' :
                       formData.initial_30d_volume >= 50000 ? '0.24%' :
                       formData.initial_30d_volume >= 10000 ? '0.35%' : '0.40%'}
                    </span>
                  </div>
                </div>
                <div style={{ fontSize: '11px', color: '#666', marginTop: '6px' }}>
                  Round-trip cost: {
                    formData.initial_30d_volume >= 10000000 ? '10 bps' :
                    formData.initial_30d_volume >= 5000000 ? '14 bps' :
                    formData.initial_30d_volume >= 1000000 ? '22 bps' :
                    formData.initial_30d_volume >= 100000 ? '34 bps' :
                    formData.initial_30d_volume >= 0 ? '65 bps' : '65 bps'
                  } (maker + taker)
                </div>
              </div>
            </div>

            {/* Co-Located Infrastructure Toggle */}
            <div style={{ 
              padding: '16px', 
              border: formData.is_colocated ? '2px solid #28a745' : '1px solid #ddd',
              borderRadius: '8px',
              backgroundColor: formData.is_colocated ? '#f0fff4' : 'white'
            }}>
              <label style={{ display: 'flex', alignItems: 'center', cursor: 'pointer', gap: '12px' }}>
                <input
                  type="checkbox"
                  name="is_colocated"
                  checked={formData.is_colocated}
                  onChange={(e) => setFormData(prev => ({ ...prev, is_colocated: e.target.checked }))}
                  style={{ width: '18px', height: '18px' }}
                />
                <div>
                  <div style={{ fontWeight: 600, fontSize: '14px' }}>
                    🏢 Co-Located Infrastructure
                  </div>
                  <div style={{ fontSize: '12px', color: '#666', marginTop: '2px' }}>
                    Bot runs on Beeks Group or similar co-located hosting with sub-millisecond latency to exchange
                  </div>
                </div>
              </label>
              {formData.is_colocated && (
                <div style={{ 
                  marginTop: '12px', 
                  padding: '10px', 
                  backgroundColor: '#e8f5e9', 
                  borderRadius: '6px',
                  fontSize: '12px'
                }}>
                  <strong>Co-Located Advantages:</strong>
                  <ul style={{ margin: '6px 0 0 16px', padding: 0 }}>
                    <li>~0.5ms latency (vs 15ms for retail)</li>
                    <li>Front of queue position (15% vs 70% back)</li>
                    <li>85% base fill probability (vs 30%)</li>
                    <li>Minimal adverse selection (1.3x vs 2.5x)</li>
                  </ul>
                </div>
              )}
            </div>
          </div>
        </AccordionSection>

        {/* ==================== SECTION 4: OPTIMIZATION ==================== */}
        <AccordionSection
          title="Optimization Method"
          icon="🧬"
          isExpanded={expandedSections.optimization ?? false}
          onToggle={() => toggleSection('optimization')}
          badge={formData.optimization_method === 'genetic' ? 'Genetic Algorithm' : 'Manual'}
          badgeColor={formData.optimization_method === 'genetic' ? '#6f42c1' : '#6c757d'}
        >
          <div style={{ display: 'grid', gap: '16px' }}>
            {/* Method Selection */}
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '12px' }}>
              {[
                { value: 'genetic', label: 'Genetic Algorithm', desc: 'Auto-find optimal parameters', icon: '🧬', recommended: true },
                { value: 'manual', label: 'Manual Parameters', desc: 'Set exact values', icon: '⚙️', recommended: false },
              ].map(method => (
                <label
                  key={method.value}
                  style={{
                    padding: '16px',
                    border: formData.optimization_method === method.value ? '2px solid #6f42c1' : '1px solid #ddd',
                    borderRadius: '8px',
                    cursor: 'pointer',
                    backgroundColor: formData.optimization_method === method.value ? '#f8f4ff' : 'white',
                    position: 'relative'
                  }}
                >
                  <input
                    type="radio"
                    name="optimization_method"
                    value={method.value}
                    checked={formData.optimization_method === method.value}
                    onChange={handleChange}
                    style={{ display: 'none' }}
                  />
                  {method.recommended && (
                    <span style={{
                      position: 'absolute',
                      top: '-8px',
                      right: '10px',
                      backgroundColor: '#28a745',
                      color: 'white',
                      fontSize: '10px',
                      padding: '2px 6px',
                      borderRadius: '8px'
                    }}>Recommended</span>
                  )}
                  <div style={{ fontSize: '24px', marginBottom: '8px' }}>{method.icon}</div>
                  <div style={{ fontWeight: 600, marginBottom: '4px' }}>{method.label}</div>
                  <div style={{ fontSize: '12px', color: '#666' }}>{method.desc}</div>
                </label>
              ))}
            </div>

            {/* Genetic Algorithm Settings */}
            {formData.optimization_method === 'genetic' && (
              <div style={{ 
                padding: '16px', 
                backgroundColor: '#f8f4ff', 
                borderRadius: '8px',
                border: '1px solid #e0d4f0'
              }}>
                <div style={{ fontWeight: 600, marginBottom: '12px', color: '#6f42c1' }}>Genetic Algorithm Settings</div>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '12px' }}>
                  <div>
                    <label style={{ display: 'block', marginBottom: '4px', fontSize: '12px', fontWeight: 500 }}>Population Size</label>
                    <input
                      type="number"
                      name="population_size"
                      value={formData.population_size}
                      onChange={handleChange}
                      min="10"
                      max="200"
                      style={{ width: '100%', padding: '8px', border: '1px solid #ccc', borderRadius: '4px' }}
                    />
                    <div style={{ fontSize: '10px', color: '#666', marginTop: '2px' }}>Default: 50</div>
                  </div>
                  <div>
                    <label style={{ display: 'block', marginBottom: '4px', fontSize: '12px', fontWeight: 500 }}>Generations</label>
                    <input
                      type="number"
                      name="generations"
                      value={formData.generations}
                      onChange={handleChange}
                      min="5"
                      max="100"
                      style={{ width: '100%', padding: '8px', border: '1px solid #ccc', borderRadius: '4px' }}
                    />
                    <div style={{ fontSize: '10px', color: '#666', marginTop: '2px' }}>Default: 20</div>
                  </div>
                </div>
              </div>
            )}

            {/* Manual Parameters */}
            {formData.optimization_method === 'manual' && (
              <div style={{ 
                padding: '16px', 
                backgroundColor: '#f8f9fa', 
                borderRadius: '8px',
                border: '1px solid #e0e0e0'
              }}>
                <div style={{ fontWeight: 600, marginBottom: '12px' }}>Core Parameters</div>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: '10px', marginBottom: '16px' }}>
                  <div>
                    <label style={{ display: 'block', marginBottom: '4px', fontSize: '11px' }}>Risk Aversion (γ)</label>
                    <input type="number" name="risk_aversion" value={formData.risk_aversion} onChange={handleChange} step="0.1" min="0.1" max="10" style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '12px' }} />
                  </div>
                  <div>
                    <label style={{ display: 'block', marginBottom: '4px', fontSize: '11px' }}>Inventory Target (q*)</label>
                    <input type="number" name="inventory_target" value={formData.inventory_target} onChange={handleChange} step="0.1" min="-1" max="1" style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '12px' }} />
                  </div>
                  <div>
                    <label style={{ display: 'block', marginBottom: '4px', fontSize: '11px' }}>Order Size</label>
                    <input type="number" name="order_size" value={formData.order_size} onChange={handleChange} step="0.001" min="0.001" style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '12px' }} />
                  </div>
                </div>

                <div style={{ fontWeight: 600, marginBottom: '12px' }}>Timing & Risk</div>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: '10px', marginBottom: '16px' }}>
                  <div>
                    <label style={{ display: 'block', marginBottom: '4px', fontSize: '11px' }}>Window (ms)</label>
                    <input type="number" name="window_ms" value={formData.window_ms} onChange={handleChange} step="100" min="100" style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '12px' }} />
                  </div>
                  <div>
                    <label style={{ display: 'block', marginBottom: '4px', fontSize: '11px' }}>Min Quote Life (ms)</label>
                    <input type="number" name="min_quote_lifetime_ms" value={formData.min_quote_lifetime_ms} onChange={handleChange} step="10" min="10" style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '12px' }} />
                  </div>
                  <div>
                    <label style={{ display: 'block', marginBottom: '4px', fontSize: '11px' }}>Fee Rate</label>
                    <input type="number" name="fee_rate" value={formData.fee_rate} onChange={handleChange} step="0.0001" min="0" max="0.01" style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '12px' }} />
                  </div>
                  <div>
                    <label style={{ display: 'block', marginBottom: '4px', fontSize: '11px' }}>Volatility Cap</label>
                    <input type="number" name="volatility_cap" value={formData.volatility_cap} onChange={handleChange} step="0.01" min="0" style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '12px' }} />
                  </div>
                  <div>
                    <label style={{ display: 'block', marginBottom: '4px', fontSize: '11px' }}>Time Horizon (min)</label>
                    <input type="number" name="time_horizon_minutes" value={formData.time_horizon_minutes} onChange={handleChange} step="1" min="1" style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '12px' }} />
                  </div>
                  <div>
                    <label style={{ display: 'block', marginBottom: '4px', fontSize: '11px' }}>Market Depth (k)</label>
                    <input type="number" name="market_depth_k" value={formData.market_depth_k} onChange={handleChange} step="0.1" min="0.1" style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '12px' }} />
                  </div>
                </div>

                <div style={{ fontWeight: 600, marginBottom: '12px', color: '#666' }}>Phase 2 Adjustments</div>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '10px' }}>
                  <div>
                    <label style={{ display: 'block', marginBottom: '4px', fontSize: '11px' }}>Volatility Adjustment</label>
                    <input type="number" name="volatility_adjustment_factor" value={formData.volatility_adjustment_factor} onChange={handleChange} step="0.1" min="0" max="1" style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '12px' }} />
                  </div>
                  <div>
                    <label style={{ display: 'block', marginBottom: '4px', fontSize: '11px' }}>Inventory Penalty</label>
                    <input type="number" name="inventory_penalty_factor" value={formData.inventory_penalty_factor} onChange={handleChange} step="0.1" min="0" max="0.8" style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '12px' }} />
                  </div>
                  <div>
                    <label style={{ display: 'block', marginBottom: '4px', fontSize: '11px' }}>Trend Sensitivity</label>
                    <input type="number" name="trend_sensitivity" value={formData.trend_sensitivity} onChange={handleChange} step="0.1" min="-0.5" max="0.5" style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '12px' }} />
                  </div>
                  <div>
                    <label style={{ display: 'block', marginBottom: '4px', fontSize: '11px' }}>Flow Sensitivity</label>
                    <input type="number" name="flow_sensitivity" value={formData.flow_sensitivity} onChange={handleChange} step="0.1" min="-0.3" max="0.3" style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '12px' }} />
                  </div>
                </div>
              </div>
            )}
          </div>
        </AccordionSection>

        {/* ==================== SECTION 5: ANALYSIS & VALIDATION ==================== */}
        <AccordionSection
          title="Analysis & Validation"
          icon="🔬"
          isExpanded={expandedSections.analysis ?? false}
          onToggle={() => toggleSection('analysis')}
          badge={[
            formData.monte_carlo_enabled ? 'MC' : '',
            formData.walk_forward_enabled ? 'WF' : '',
            formData.auto_save_profitable ? 'Auto-Save' : ''
          ].filter(Boolean).join(' + ') || 'Configure'}
          badgeColor="#17a2b8"
        >
          <div style={{ display: 'grid', gap: '16px' }}>
            {/* Monte Carlo & Walk-Forward */}
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '12px' }}>
              <div style={{ 
                padding: '14px', 
                backgroundColor: formData.monte_carlo_enabled ? '#e3f2fd' : '#f8f9fa', 
                borderRadius: '8px',
                border: formData.monte_carlo_enabled ? '1px solid #90caf9' : '1px solid #e0e0e0'
              }}>
                <label style={{ display: 'flex', alignItems: 'center', cursor: 'pointer', marginBottom: '10px' }}>
                  <input
                    type="checkbox"
                    checked={formData.monte_carlo_enabled}
                    onChange={(e) => setFormData(prev => ({ ...prev, monte_carlo_enabled: e.target.checked }))}
                    style={{ marginRight: '8px' }}
                  />
                  <span style={{ fontWeight: 600 }}>🎲 Monte Carlo</span>
                </label>
                {formData.monte_carlo_enabled && (
                  <div>
                    <label style={{ fontSize: '11px', display: 'block', marginBottom: '4px' }}>Runs</label>
                    <input
                      type="number"
                      name="monte_carlo_runs"
                      value={formData.monte_carlo_runs}
                      onChange={handleChange}
                      min="100"
                      max="5000"
                      step="100"
                      style={{ width: '100%', padding: '6px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '12px' }}
                    />
                  </div>
                )}
              </div>
              <div style={{ 
                padding: '14px', 
                backgroundColor: formData.walk_forward_enabled ? '#e8f5e9' : '#f8f9fa', 
                borderRadius: '8px',
                border: formData.walk_forward_enabled ? '1px solid #a5d6a7' : '1px solid #e0e0e0'
              }}>
                <label style={{ display: 'flex', alignItems: 'center', cursor: 'pointer' }}>
                  <input
                    type="checkbox"
                    checked={formData.walk_forward_enabled}
                    onChange={(e) => setFormData(prev => ({ ...prev, walk_forward_enabled: e.target.checked }))}
                    style={{ marginRight: '8px' }}
                  />
                  <span style={{ fontWeight: 600 }}>🔄 Walk-Forward</span>
                </label>
                <div style={{ fontSize: '11px', color: '#666', marginTop: '6px' }}>
                  Tests out-of-sample performance
                </div>
              </div>
            </div>

            {/* Custom Fitness Weights (for Genetic) */}
            {formData.optimization_method === 'genetic' && (
              <div style={{ 
                padding: '14px', 
                backgroundColor: formData.custom_fitness_weights ? '#fff8e1' : '#f8f9fa', 
                borderRadius: '8px',
                border: formData.custom_fitness_weights ? '1px solid #ffcc80' : '1px solid #e0e0e0'
              }}>
                <label style={{ display: 'flex', alignItems: 'center', cursor: 'pointer', marginBottom: formData.custom_fitness_weights ? '12px' : '0' }}>
                  <input
                    type="checkbox"
                    checked={formData.custom_fitness_weights}
                    onChange={(e) => setFormData(prev => ({ ...prev, custom_fitness_weights: e.target.checked }))}
                    style={{ marginRight: '8px' }}
                  />
                  <span style={{ fontWeight: 600 }}>⚖️ Custom Fitness Weights</span>
                </label>
                {formData.custom_fitness_weights && (
                  <>
                    {/* Preset buttons */}
                    <div style={{ display: 'flex', gap: '6px', marginBottom: '12px', flexWrap: 'wrap' }}>
                      {Object.entries(FITNESS_PRESETS).map(([name, weights]) => (
                        <button
                          key={name}
                          type="button"
                          onClick={() => setFormData(prev => ({ ...prev, fitness_weights: { ...weights } }))}
                          style={{
                            padding: '4px 10px',
                            fontSize: '11px',
                            border: '1px solid #ddd',
                            borderRadius: '4px',
                            backgroundColor: '#fff',
                            cursor: 'pointer',
                            textTransform: 'capitalize',
                          }}
                        >
                          {name.replace(/([A-Z])/g, ' $1').trim()}
                        </button>
                      ))}
                    </div>
                    
                    {/* Pie chart and sliders layout */}
                    <div style={{ display: 'flex', gap: '16px', alignItems: 'flex-start' }}>
                      {/* Pie Chart */}
                      <div style={{ flexShrink: 0, display: 'flex', flexDirection: 'column', alignItems: 'center' }}>
                        <FitnessPieChart weights={formData.fitness_weights} size={130} />
                        {(() => {
                          const total = Object.values(formData.fitness_weights).reduce((a, b) => a + b, 0);
                          const isValid = total === 100;
                          return (
                            <div style={{ 
                              marginTop: '8px', 
                              fontSize: '11px', 
                              fontWeight: 600,
                              color: isValid ? '#4CAF50' : '#f44336',
                              textAlign: 'center'
                            }}>
                              {isValid ? '✓ Valid' : `${total}% ≠ 100%`}
                            </div>
                          );
                        })()}
                      </div>
                      
                      {/* Sliders with colored labels */}
                      <div style={{ flex: 1, display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '8px' }}>
                        {(['sharpe', 'sortino', 'profit_factor', 'win_rate', 'net_profit', 'calmar', 'num_trades'] as const).map(key => (
                          <div key={key} style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                            <div style={{ 
                              width: '10px', 
                              height: '10px', 
                              borderRadius: '2px', 
                              backgroundColor: WEIGHT_COLORS[key],
                              flexShrink: 0
                            }} />
                            <div style={{ flex: 1, minWidth: 0 }}>
                              <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: '10px', marginBottom: '2px' }}>
                                <span style={{ fontWeight: 500 }}>{WEIGHT_LABELS[key]}</span>
                                <span style={{ fontWeight: 600 }}>{formData.fitness_weights[key]}%</span>
                              </div>
                              <input
                                type="range"
                                min="0"
                                max="50"
                                value={formData.fitness_weights[key]}
                                onChange={(e) => setFormData(prev => ({
                                  ...prev,
                                  fitness_weights: { ...prev.fitness_weights, [key]: parseInt(e.target.value) }
                                }))}
                                style={{ 
                                  width: '100%', 
                                  height: '6px',
                                  accentColor: WEIGHT_COLORS[key]
                                }}
                              />
                            </div>
                          </div>
                        ))}
                      </div>
                    </div>
                  </>
                )}
              </div>
            )}

            {/* Auto-Save Profitable Strategies */}
            <div style={{ 
              padding: '14px', 
              backgroundColor: formData.auto_save_profitable ? '#e8f5e9' : '#f8f9fa', 
              borderRadius: '8px',
              border: formData.auto_save_profitable ? '1px solid #a5d6a7' : '1px solid #e0e0e0'
            }}>
              <label style={{ display: 'flex', alignItems: 'center', cursor: 'pointer', marginBottom: formData.auto_save_profitable ? '12px' : '0' }}>
                <input
                  type="checkbox"
                  checked={formData.auto_save_profitable}
                  onChange={(e) => setFormData(prev => ({ ...prev, auto_save_profitable: e.target.checked }))}
                  style={{ marginRight: '8px' }}
                />
                <span style={{ fontWeight: 600 }}>💾 Auto-Save Profitable Strategies</span>
              </label>
              {formData.auto_save_profitable && (
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: '10px' }}>
                  <div>
                    <label style={{ fontSize: '10px', display: 'block', marginBottom: '2px' }}>Min Sharpe</label>
                    <input type="number" name="min_sharpe_ratio" value={formData.min_sharpe_ratio} onChange={handleChange} step="0.1" min="0" style={{ width: '100%', padding: '4px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '11px' }} />
                  </div>
                  <div>
                    <label style={{ fontSize: '10px', display: 'block', marginBottom: '2px' }}>Min Profit Factor</label>
                    <input type="number" name="min_profit_factor" value={formData.min_profit_factor} onChange={handleChange} step="0.1" min="1" style={{ width: '100%', padding: '4px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '11px' }} />
                  </div>
                  <div>
                    <label style={{ fontSize: '10px', display: 'block', marginBottom: '2px' }}>Max Drawdown (%)</label>
                    <input type="number" value={formData.max_drawdown * 100} onChange={(e) => setFormData(prev => ({ ...prev, max_drawdown: parseFloat(e.target.value) / 100 }))} step="1" min="5" max="50" style={{ width: '100%', padding: '4px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '11px' }} />
                  </div>
                  <div>
                    <label style={{ fontSize: '10px', display: 'block', marginBottom: '2px' }}>Min Win Rate (%)</label>
                    <input type="number" value={formData.min_win_rate * 100} onChange={(e) => setFormData(prev => ({ ...prev, min_win_rate: parseFloat(e.target.value) / 100 }))} step="1" min="30" max="80" style={{ width: '100%', padding: '4px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '11px' }} />
                  </div>
                  <div>
                    <label style={{ fontSize: '10px', display: 'block', marginBottom: '2px' }}>Min Trades</label>
                    <input type="number" name="min_trades" value={formData.min_trades} onChange={handleChange} step="10" min="10" style={{ width: '100%', padding: '4px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '11px' }} />
                  </div>
                  <div>
                    <label style={{ fontSize: '10px', display: 'block', marginBottom: '2px' }}>Min PnL ($)</label>
                    <input type="number" value={formData.min_total_pnl ?? 0} onChange={(e) => setFormData(prev => ({ ...prev, min_total_pnl: parseFloat(e.target.value) || null }))} step="100" style={{ width: '100%', padding: '4px', border: '1px solid #ccc', borderRadius: '4px', fontSize: '11px' }} />
                  </div>
                </div>
              )}
            </div>
          </div>
        </AccordionSection>

        {/* ==================== SUBMIT SECTION ==================== */}
        <div style={{ 
          marginTop: '8px',
          padding: '20px',
          backgroundColor: '#f8f9fa',
          borderRadius: '8px',
          border: '1px solid #e0e0e0'
        }}>
          {/* Validation Check */}
          {formData.portfolio_assets.length === 0 && (
            <div style={{ 
              marginBottom: '16px',
              padding: '12px',
              backgroundColor: '#fff3cd',
              border: '1px solid #ffc107',
              borderRadius: '6px',
              color: '#856404',
              fontSize: '13px'
            }}>
              ⚠️ Add at least one asset to your portfolio before submitting.
            </div>
          )}

          {/* Fitness weights validation warning */}
          {formData.custom_fitness_weights && (() => {
            const total = Object.values(formData.fitness_weights).reduce((a, b) => a + b, 0);
            return total !== 100 ? (
              <div style={{
                padding: '10px 14px',
                marginBottom: '12px',
                backgroundColor: '#ffebee',
                border: '1px solid #f44336',
                borderRadius: '6px',
                color: '#c62828',
                fontSize: '13px'
              }}>
                ⚖️ Fitness weights must sum to 100% (currently {total}%)
              </div>
            ) : null;
          })()}

          <button
            type="submit"
            disabled={loading || formData.portfolio_assets.length === 0 || (formData.custom_fitness_weights && Object.values(formData.fitness_weights).reduce((a, b) => a + b, 0) !== 100)}
            style={{
              width: '100%',
              padding: '14px 24px',
              backgroundColor: (loading || formData.portfolio_assets.length === 0 || (formData.custom_fitness_weights && Object.values(formData.fitness_weights).reduce((a, b) => a + b, 0) !== 100)) ? '#ccc' : '#007bff',
              color: 'white',
              border: 'none',
              borderRadius: '8px',
              cursor: (loading || formData.portfolio_assets.length === 0 || (formData.custom_fitness_weights && Object.values(formData.fitness_weights).reduce((a, b) => a + b, 0) !== 100)) ? 'not-allowed' : 'pointer',
              fontSize: '16px',
              fontWeight: 600,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              gap: '8px'
            }}
          >
            {loading ? (
              <>⏳ Submitting...</>
            ) : (
              <>🚀 Run Backtest</>
            )}
          </button>

          {/* Summary */}
          <div style={{ 
            marginTop: '12px', 
            fontSize: '12px', 
            color: '#666',
            textAlign: 'center'
          }}>
            {formData.portfolio_assets.length} asset{formData.portfolio_assets.length !== 1 ? 's' : ''} • 
            ${formData.initial_capital.toLocaleString()} capital • 
            {formData.optimization_method === 'genetic' ? 'GA optimization' : 'Manual params'}
          </div>
        </div>

        {/* Error Message */}
        {error && (
          <div style={{
            marginTop: '12px',
            padding: '12px 16px',
            backgroundColor: '#f8d7da',
            border: '1px solid #f5c6cb',
            borderRadius: '8px',
            color: '#721c24',
          }}>
            <strong>Error:</strong> {error}
          </div>
        )}

        {/* Success Message */}
        {jobId && (
          <div style={{
            marginTop: '12px',
            padding: '12px 16px',
            backgroundColor: '#d4edda',
            border: '1px solid #c3e6cb',
            borderRadius: '8px',
            color: '#155724',
          }}>
            <strong>✓ Success!</strong> Backtest submitted with Job ID: <code style={{ backgroundColor: '#c3e6cb', padding: '2px 6px', borderRadius: '4px' }}>{jobId}</code>
          </div>
        )}
      </form>
    </div>
  );
};

export default BacktestForm;
