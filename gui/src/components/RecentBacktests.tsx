import React from 'react';
import {
  Card,
  CardContent,
  Typography,
  List,
  ListItem,
  ListItemText,
  Chip,
  Box,
  IconButton,
} from '@mui/material';
import {
  Launch as LaunchIcon,
  TrendingUp as TrendingUpIcon,
  TrendingDown as TrendingDownIcon,
} from '@mui/icons-material';
import { BacktestResult } from '../types';
import { format } from 'date-fns';

interface RecentBacktestsProps {
  backtests: BacktestResult[];
}

export const RecentBacktests: React.FC<RecentBacktestsProps> = ({ backtests }) => {
  const getReturnColor = (returnValue: number) => {
    if (returnValue > 0) return 'success';
    if (returnValue < 0) return 'error';
    return 'default';
  };

  const getReturnIcon = (returnValue: number) => {
    return returnValue >= 0 ? <TrendingUpIcon /> : <TrendingDownIcon />;
  };

  return (
    <Card sx={{ height: 400 }}>
      <CardContent>
        <Typography variant="h6" gutterBottom>
          Recent Backtests
        </Typography>
        {backtests.length === 0 ? (
          <Box
            sx={{
              height: 200,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
            }}
          >
            <Typography color="textSecondary">
              No recent backtests found
            </Typography>
          </Box>
        ) : (
          <List sx={{ maxHeight: 320, overflow: 'auto' }}>
            {backtests.map((backtest) => (
              <ListItem
                key={backtest.id}
                secondaryAction={
                  <IconButton
                    edge="end"
                    onClick={() => window.location.href = `/backtests/${backtest.id}`}
                  >
                    <LaunchIcon />
                  </IconButton>
                }
                sx={{
                  border: '1px solid',
                  borderColor: 'divider',
                  borderRadius: 1,
                  mb: 1,
                  '&:hover': {
                    backgroundColor: 'action.hover',
                  },
                }}
              >
                <ListItemText
                  primary={
                    <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
                      <Typography variant="subtitle2">
                        {backtest.strategy_name}
                      </Typography>
                      <Chip
                        label={backtest.symbol}
                        size="small"
                        variant="outlined"
                      />
                    </Box>
                  }
                  secondary={
                    <Box sx={{ mt: 1 }}>
                      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 0.5 }}>
                        {getReturnIcon(backtest.total_return_percent)}
                        <Chip
                          label={`${backtest.total_return_percent > 0 ? '+' : ''}${backtest.total_return_percent.toFixed(1)}%`}
                          size="small"
                          color={getReturnColor(backtest.total_return_percent)}
                          variant="filled"
                        />
                        <Typography variant="caption" color="textSecondary">
                          Sharpe: {backtest.sharpe_ratio.toFixed(2)}
                        </Typography>
                      </Box>
                      <Typography variant="caption" color="textSecondary">
                        {format(new Date(backtest.created_at), 'MMM dd, yyyy HH:mm')}
                      </Typography>
                    </Box>
                  }
                />
              </ListItem>
            ))}
          </List>
        )}
      </CardContent>
    </Card>
  );
};
