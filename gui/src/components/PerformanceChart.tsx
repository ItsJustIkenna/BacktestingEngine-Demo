import React from 'react';
import { Box, Typography } from '@mui/material';
import { ScatterChart, Scatter, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from 'recharts';
import { BacktestResult } from '../types';

interface PerformanceChartProps {
  backtests: BacktestResult[];
}

export const PerformanceChart: React.FC<PerformanceChartProps> = ({ backtests }) => {
  if (!backtests || backtests.length === 0) {
    return (
      <Box
        sx={{
          height: 300,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
        }}
      >
        <Typography color="textSecondary">
          No backtest data available
        </Typography>
      </Box>
    );
  }

  // Prepare data for the scatter plot
  const scatterData = backtests.map(backtest => ({
    x: backtest.total_return_percent,
    y: backtest.sharpe_ratio,
    name: `${backtest.strategy_name} (${backtest.symbol})`,
    return: backtest.total_return_percent,
    sharpe: backtest.sharpe_ratio,
  }));

  const CustomTooltip = ({ active, payload }: any) => {
    if (active && payload && payload.length) {
      const data = payload[0].payload;
      return (
        <div style={{ 
          backgroundColor: 'rgba(0, 0, 0, 0.8)', 
          padding: '10px', 
          border: '1px solid #333',
          borderRadius: '4px',
          color: 'white'
        }}>
          <p>{data.name}</p>
          <p>{`Return: ${data.return.toFixed(1)}%`}</p>
          <p>{`Sharpe: ${data.sharpe.toFixed(2)}`}</p>
        </div>
      );
    }
    return null;
  };

  return (
    <Box sx={{ height: 350, width: '100%' }}>
      <ResponsiveContainer width="100%" height="100%">
        <ScatterChart
          margin={{
            top: 20,
            right: 20,
            bottom: 20,
            left: 20,
          }}
        >
          <CartesianGrid strokeDasharray="3 3" stroke="#333" />
          <XAxis
            type="number"
            dataKey="x"
            name="Return"
            unit="%"
            stroke="white"
            label={{ value: 'Total Return (%)', position: 'bottom', style: { fill: 'white' } }}
          />
          <YAxis
            type="number"
            dataKey="y"
            name="Sharpe"
            stroke="white"
            label={{ value: 'Sharpe Ratio', angle: -90, position: 'insideLeft', style: { fill: 'white' } }}
          />
          <Tooltip content={<CustomTooltip />} />
          <Scatter
            name="Strategies"
            data={scatterData}
            fill="#1976d2"
          />
        </ScatterChart>
      </ResponsiveContainer>
    </Box>
  );
};
