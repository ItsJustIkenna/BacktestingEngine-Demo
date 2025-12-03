import React from 'react';

interface EquityCurveData {
  timestamp: string;
  equity: number;
}

interface EquityCurveChartProps {
  data: EquityCurveData[];
}

export const EquityCurveChart: React.FC<EquityCurveChartProps> = ({ data }) => {
  // Basic SVG chart implementation
  // For production, use a library like recharts or Chart.js
  
  if (!data || data.length === 0) {
    return <div style={{ padding: '20px', textAlign: 'center' }}>No data available</div>;
  }

  const width = 800;
  const height = 400;
  const padding = 40;

  const maxEquity = Math.max(...data.map(d => d.equity));
  const minEquity = Math.min(...data.map(d => d.equity));
  const equityRange = maxEquity - minEquity;

  const xScale = (index: number) => padding + (index / (data.length - 1)) * (width - 2 * padding);
  const yScale = (equity: number) => height - padding - ((equity - minEquity) / equityRange) * (height - 2 * padding);

  const pathData = data.map((d, i) => {
    const x = xScale(i);
    const y = yScale(d.equity);
    return i === 0 ? `M ${x} ${y}` : `L ${x} ${y}`;
  }).join(' ');

  return (
    <div style={{ width: '100%', overflowX: 'auto' }}>
      <svg width={width} height={height} style={{ border: '1px solid #ddd' }}>
        {/* Background */}
        <rect width={width} height={height} fill="#f9f9f9" />
        
        {/* Grid lines */}
        {[0, 0.25, 0.5, 0.75, 1].map((percent, i) => {
          const y = padding + percent * (height - 2 * padding);
          return (
            <g key={i}>
              <line
                x1={padding}
                y1={y}
                x2={width - padding}
                y2={y}
                stroke="#e0e0e0"
                strokeWidth={1}
              />
              <text x={padding - 10} y={y + 5} fontSize="12" textAnchor="end" fill="#666">
                {(maxEquity - percent * equityRange).toFixed(0)}
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
          Time
        </text>
        <text x={10} y={height / 2} fontSize="14" textAnchor="middle" transform={`rotate(-90, 10, ${height / 2})`} fill="#333">
          Equity
        </text>
      </svg>
    </div>
  );
};

export default EquityCurveChart;
