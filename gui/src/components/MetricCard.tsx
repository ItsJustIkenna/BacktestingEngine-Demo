import React from 'react';
import { Card, CardContent, Typography, Box, Avatar, useTheme } from '@mui/material';

interface MetricCardProps {
  title: string;
  value: string | number;
  icon: React.ReactNode;
  color: string;
  subtitle?: string;
  trend?: 'up' | 'down' | 'neutral';
}

export const MetricCard: React.FC<MetricCardProps> = ({
  title,
  value,
  icon,
  color,
  subtitle,
  trend,
}) => {
  const theme = useTheme();

  const getTrendColor = () => {
    switch (trend) {
      case 'up':
        return theme.palette.success.main;
      case 'down':
        return theme.palette.error.main;
      default:
        return theme.palette.text.secondary;
    }
  };

  return (
    <Card
      sx={{
        height: '100%',
        position: 'relative',
        overflow: 'hidden',
        '&:hover': {
          transform: 'translateY(-2px)',
          boxShadow: theme.shadows[8],
        },
        transition: 'all 0.3s ease-in-out',
      }}
    >
      <CardContent>
        <Box
          sx={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            mb: 2,
          }}
        >
          <Avatar
            sx={{
              backgroundColor: `${color}20`,
              color: color,
              width: 48,
              height: 48,
            }}
          >
            {icon}
          </Avatar>
        </Box>
        
        <Typography
          variant="h4"
          component="div"
          sx={{
            fontWeight: 600,
            mb: 0.5,
            color: theme.palette.text.primary,
          }}
        >
          {value}
        </Typography>
        
        <Typography
          variant="body2"
          color="textSecondary"
          sx={{
            fontWeight: 500,
            mb: subtitle ? 0.5 : 0,
          }}
        >
          {title}
        </Typography>
        
        {subtitle && (
          <Typography
            variant="caption"
            sx={{
              color: getTrendColor(),
              fontWeight: 500,
            }}
          >
            {subtitle}
          </Typography>
        )}
      </CardContent>
      
      {/* Decorative background gradient */}
      <Box
        sx={{
          position: 'absolute',
          top: 0,
          right: 0,
          width: '60px',
          height: '60px',
          background: `linear-gradient(135deg, ${color}20, transparent)`,
          borderRadius: '0 8px 0 100%',
        }}
      />
    </Card>
  );
};
