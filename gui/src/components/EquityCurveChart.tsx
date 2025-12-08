import React from 'react';

// Support multiple data formats:
// 1. Array of numbers (from backend): [10000, 10050, 10100, ...]
// 2. Array of objects with value: [{ value: 10000 }, ...]
// 3. Array of objects with timestamp and value: [{ timestamp: "2025-01-01", value: 10000 }, ...]
type EquityCurveDataItem = number | { timestamp?: string; equity?: number; value?: number };

interface EquityCurveChartProps {
  data?: EquityCurveDataItem[];
  startDate?: string;
  endDate?: string;
}

export const EquityCurveChart: React.FC<EquityCurveChartProps> = ({ data, startDate, endDate }) => {
  // Basic SVG chart implementation
  // For production, use a library like recharts or Chart.js
  
  if (!data || data.length === 0) {
    return <div style={{ padding: '20px', textAlign: 'center' }}>No data available</div>;
  }

  // Helper to get the equity value (supports numbers and objects with 'equity' or 'value' properties)
  const getEquity = (d: EquityCurveDataItem): number => {
    if (typeof d === 'number') {
      return d;
    }
    return d.equity ?? d.value ?? 0;
  };

  // Helper to calculate date at a given ratio between start and end
  const getDateAtRatio = (ratio: number): string => {
    if (!startDate || !endDate) {
      return `${Math.round(ratio * 100)}%`;
    }
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

  const width = 800;
  const height = 400;
  const padding = 60;  // Increased padding for Y-axis labels

  // Filter out invalid values and get clean equity array
  const equities = data.map(getEquity).filter(v => !isNaN(v) && isFinite(v));
  
  if (equities.length === 0) {
    return <div style={{ padding: '20px', textAlign: 'center' }}>No valid data points available</div>;
  }

  const maxEquity = Math.max(...equities);
  const minEquity = Math.min(...equities);
  const equityRange = maxEquity - minEquity || 1; // Prevent division by zero

  const xScale = (index: number) => padding + (index / (equities.length - 1 || 1)) * (width - 2 * padding);
  const yScale = (equity: number) => height - padding - ((equity - minEquity) / equityRange) * (height - 2 * padding);

  // Build path data from valid equities only
  const pathData = equities.map((equity, i) => {
    const x = xScale(i);
    const y = yScale(equity);
    return i === 0 ? `M ${x} ${y}` : `L ${x} ${y}`;
  }).join(' ');

  // Format currency for Y-axis labels
  const formatCurrency = (value: number): string => {
    if (Math.abs(value) >= 1000000) {
      return `$${(value / 1000000).toFixed(1)}M`;
    } else if (Math.abs(value) >= 1000) {
      return `$${(value / 1000).toFixed(1)}K`;
    }
    return `$${value.toFixed(0)}`;
  };

  return (
    <div style={{ width: '100%', overflowX: 'auto' }}>
      <svg width={width} height={height} style={{ border: '1px solid #ddd' }}>
        {/* Background */}
        <rect width={width} height={height} fill="#f9f9f9" />
        
        {/* Horizontal Grid lines and Y-axis labels */}
        {[0, 0.25, 0.5, 0.75, 1].map((percent, i) => {
          const y = padding + percent * (height - 2 * padding);
          const value = maxEquity - percent * equityRange;
          return (
            <g key={`h-${i}`}>
              <line
                x1={padding}
                y1={y}
                x2={width - padding}
                y2={y}
                stroke="#e0e0e0"
                strokeWidth={1}
              />
              <text x={padding - 5} y={y + 4} fontSize="11" textAnchor="end" fill="#666">
                {formatCurrency(value)}
              </text>
            </g>
          );
        })}

        {/* Vertical Grid lines and X-axis labels */}
        {[0, 0.25, 0.5, 0.75, 1].map((percent, i) => {
          const x = padding + percent * (width - 2 * padding);
          return (
            <g key={`v-${i}`}>
              <line
                x1={x}
                y1={padding}
                x2={x}
                y2={height - padding}
                stroke="#e0e0e0"
                strokeWidth={1}
              />
              <text x={x} y={height - padding + 15} fontSize="11" textAnchor="middle" fill="#666">
                {getDateAtRatio(percent)}
              </text>
            </g>
          );
        })}

        {/* Equity curve */}
        <path d={pathData} stroke="#2563eb" strokeWidth={2} fill="none" />

        {/* Axes */}
        <line x1={padding} y1={padding} x2={padding} y2={height - padding} stroke="#333" strokeWidth={2} />
        <line x1={padding} y1={height - padding} x2={width - padding} y2={height - padding} stroke="#333" strokeWidth={2} />

        {/* Labels */}
        <text x={width / 2} y={height - 5} fontSize="14" textAnchor="middle" fill="#333">
          {startDate && endDate ? 'Date' : 'Time'}
        </text>
        <text x={10} y={height / 2} fontSize="14" textAnchor="middle" transform={`rotate(-90, 10, ${height / 2})`} fill="#333">
          Equity
        </text>
      </svg>
    </div>
  );
};

export default EquityCurveChart;
