import React, { useState, useEffect } from 'react';
import {
  Box,
  Grid,
  Card,
  CardContent,
  Typography,
  Button,
  LinearProgress,
  Alert,
  useTheme,
} from '@mui/material';
import {
  TrendingUp as TrendingUpIcon,
  Assessment as AssessmentIcon,
  Speed as SpeedIcon,
  Timeline as TimelineIcon,
} from '@mui/icons-material';
import { BacktestResult, BacktestStatus } from '../types';
import { MetricCard } from './MetricCard';
import { PerformanceChart } from './PerformanceChart';
import { RecentBacktests } from './RecentBacktests';

export const Dashboard: React.FC = () => {
  const theme = useTheme();
  const [recentBacktests, setRecentBacktests] = useState<BacktestResult[]>([]);
  const [runningBacktests, setRunningBacktests] = useState<BacktestStatus[]>([]);
  const [dashboardMetrics, setDashboardMetrics] = useState({
    totalBacktests: 0,
    avgReturn: 0,
    avgSharpe: 0,
    bestStrategy: '',
    totalStrategies: 0,
    avgExecutionTime: 0,
  });
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    loadDashboardData();
  }, []);

  const loadDashboardData = async () => {
    try {
      setLoading(true);
      
      // Simulate API calls - replace with actual API endpoints
      await Promise.all([
        fetch('/api/backtests?limit=5&sort=created_at:desc'),
        fetch('/api/backtests/running'),
        fetch('/api/backtests/metrics'),
      ]);
      
      // Mock data for development
      const mockRecentBacktests: BacktestResult[] = [
        {
          id: '1',
          strategy_name: 'Mean Reversion',
          symbol: 'SPY',
          timeframe: '1d',
          start_date: '2024-01-01',
          end_date: '2024-08-28',
          initial_capital: 100000,
          final_capital: 125000,
          total_return_percent: 25.0,
          annualized_return_percent: 31.25,
          sharpe_ratio: 1.8,
          sortino_ratio: 2.1,
          calmar_ratio: 1.5,
          max_drawdown_percent: -8.5,
          max_drawdown_duration_days: 45,
          total_trades: 156,
          winning_trades: 98,
          losing_trades: 58,
          win_rate: 0.628,
          profit_factor: 1.75,
          avg_trade_return: 0.0045,
          best_trade: 2500,
          worst_trade: -1200,
          volatility: 0.18,
          value_at_risk_95: -2800,
          expected_shortfall_95: -3500,
          beta: 0.85,
          total_orders: 312,
          filled_orders: 308,
          cancelled_orders: 4,
          avg_slippage: 0.001,
          total_commission_paid: 780,
          equity_curve: [],
          trades: [],
          drawdown_curve: [],
          created_at: '2024-08-28T10:30:00Z',
          backtest_duration_seconds: 45.2,
        },
        // Add more mock data as needed
      ];

      const mockRunningBacktests: BacktestStatus[] = [
        {
          status: 'running',
          progress: 65,
          message: 'Processing momentum strategy on TSLA...',
          start_time: '2024-08-28T11:00:00Z',
          estimated_completion: '2024-08-28T11:15:00Z',
        },
      ];

      const mockMetrics = {
        totalBacktests: 1247,
        avgReturn: 18.5,
        avgSharpe: 1.42,
        bestStrategy: 'Trend Following',
        totalStrategies: 23,
        avgExecutionTime: 38.7,
      };

      setRecentBacktests(mockRecentBacktests);
      setRunningBacktests(mockRunningBacktests);
      setDashboardMetrics(mockMetrics);
    } catch (error) {
      console.error('Error loading dashboard data:', error);
    } finally {
      setLoading(false);
    }
  };

  if (loading) {
    return (
      <Box sx={{ width: '100%', mt: 4 }}>
        <LinearProgress />
        <Typography sx={{ mt: 2, textAlign: 'center' }}>
          Loading dashboard data...
        </Typography>
      </Box>
    );
  }

  return (
    <Box sx={{ flexGrow: 1 }}>
      {/* Header */}
      <Box sx={{ mb: 4 }}>
        <Typography variant="h4" component="h1" gutterBottom sx={{ fontWeight: 300 }}>
          BacktestingEngine Dashboard
        </Typography>
        <Typography variant="body1" color="textSecondary">
          Monitor your trading strategies and backtest performance
        </Typography>
      </Box>

      {/* Running Backtests Alert */}
      {runningBacktests.length > 0 && (
        <Box sx={{ mb: 3 }}>
          <Alert severity="info" icon={<SpeedIcon />}>
            <Box>
              <Typography variant="subtitle2" sx={{ mb: 1 }}>
                Active Backtests ({runningBacktests.length})
              </Typography>
              {runningBacktests.map((backtest, index) => (
                <Box key={index} sx={{ mb: 1 }}>
                  <Typography variant="body2" sx={{ mb: 0.5 }}>
                    {backtest.message}
                  </Typography>
                  <LinearProgress
                    variant="determinate"
                    value={backtest.progress}
                    sx={{ height: 6, borderRadius: 3 }}
                  />
                  <Typography variant="caption" color="textSecondary">
                    {backtest.progress}% complete
                  </Typography>
                </Box>
              ))}
            </Box>
          </Alert>
        </Box>
      )}

      {/* Key Metrics */}
      <Grid container spacing={3} sx={{ mb: 4 }}>
        <Grid item xs={12} sm={6} md={2}>
          <MetricCard
            title="Total Backtests"
            value={dashboardMetrics.totalBacktests.toLocaleString()}
            icon={<AssessmentIcon />}
            color={theme.palette.primary.main}
          />
        </Grid>
        <Grid item xs={12} sm={6} md={2}>
          <MetricCard
            title="Avg Return"
            value={`${dashboardMetrics.avgReturn}%`}
            icon={<TrendingUpIcon />}
            color={theme.palette.success.main}
          />
        </Grid>
        <Grid item xs={12} sm={6} md={2}>
          <MetricCard
            title="Avg Sharpe"
            value={dashboardMetrics.avgSharpe.toFixed(2)}
            icon={<TimelineIcon />}
            color={theme.palette.info.main}
          />
        </Grid>
        <Grid item xs={12} sm={6} md={2}>
          <MetricCard
            title="Strategies"
            value={dashboardMetrics.totalStrategies.toString()}
            icon={<SpeedIcon />}
            color={theme.palette.warning.main}
          />
        </Grid>
        <Grid item xs={12} sm={6} md={2}>
          <MetricCard
            title="Best Strategy"
            value={dashboardMetrics.bestStrategy}
            icon={<TrendingUpIcon />}
            color={theme.palette.success.main}
          />
        </Grid>
        <Grid item xs={12} sm={6} md={2}>
          <MetricCard
            title="Avg Exec Time"
            value={`${dashboardMetrics.avgExecutionTime}s`}
            icon={<SpeedIcon />}
            color={theme.palette.secondary.main}
          />
        </Grid>
      </Grid>

      {/* Charts and Recent Backtests */}
      <Grid container spacing={3}>
        <Grid item xs={12} lg={8}>
          <Card sx={{ height: 400 }}>
            <CardContent>
              <Typography variant="h6" gutterBottom>
                Performance Overview
              </Typography>
              <PerformanceChart backtests={recentBacktests} />
            </CardContent>
          </Card>
        </Grid>
        <Grid item xs={12} lg={4}>
          <RecentBacktests backtests={recentBacktests} />
        </Grid>
      </Grid>

      {/* Quick Actions */}
      <Box sx={{ mt: 4, display: 'flex', gap: 2, flexWrap: 'wrap' }}>
        <Button
          variant="contained"
          size="large"
          onClick={() => window.location.href = '/new-backtest'}
          sx={{
            background: `linear-gradient(45deg, ${theme.palette.secondary.main}, ${theme.palette.secondary.dark})`,
            boxShadow: `0 3px 5px 2px ${theme.palette.secondary.main}30`,
          }}
        >
          View Strategies
        </Button>
        <Button
          variant="outlined"
          size="large"
          onClick={() => window.location.href = '/backtests'}
        >
          View All Results
        </Button>
        <Button
          variant="outlined"
          size="large"
          onClick={() => window.location.href = '/compare'}
        >
          Compare Strategies
        </Button>
      </Box>
    </Box>
  );
};
