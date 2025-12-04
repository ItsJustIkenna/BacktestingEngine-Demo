/**
 * Input validation and sanitization utilities
 */

/**
 * Sanitize string input to prevent XSS attacks
 */
export function sanitizeString(input: string): string {
  if (!input) return '';
  
  // Replace potentially dangerous characters
  return input
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#x27;')
    .replace(/\//g, '&#x2F;');
}

/**
 * Validate and sanitize numeric input
 */
export function sanitizeNumber(input: any, min?: number, max?: number): number {
  const num = parseFloat(input);
  
  if (isNaN(num)) {
    throw new Error('Invalid number');
  }
  
  if (min !== undefined && num < min) {
    throw new Error(`Number must be at least ${min}`);
  }
  
  if (max !== undefined && num > max) {
    throw new Error(`Number must be at most ${max}`);
  }
  
  return num;
}

/**
 * Validate symbol format (alphanumeric and limited special chars)
 */
export function validateSymbol(symbol: string): boolean {
  if (!symbol || typeof symbol !== 'string') return false;
  // Allow alphanumeric, dash, underscore, and forward slash
  return /^[a-zA-Z0-9_\-\/]+$/.test(symbol) && symbol.length <= 20;
}

/**
 * Validate exchange name
 */
export function validateExchange(exchange: string): boolean {
  if (!exchange || typeof exchange !== 'string') return false;
  // Allow alphanumeric and underscore only
  return /^[a-zA-Z0-9_]+$/.test(exchange) && exchange.length <= 50;
}

/**
 * Validate UUID format
 */
export function validateUUID(uuid: string): boolean {
  if (!uuid || typeof uuid !== 'string') return false;
  const uuidRegex = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
  return uuidRegex.test(uuid);
}

/**
 * Validate email format
 */
export function validateEmail(email: string): boolean {
  if (!email || typeof email !== 'string') return false;
  const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
  return emailRegex.test(email) && email.length <= 255;
}

/**
 * Sanitize object for safe JSON serialization
 */
export function sanitizeObject<T extends Record<string, any>>(obj: T): T {
  const sanitized = {} as T;
  
  for (const key in obj) {
    if (obj.hasOwnProperty(key)) {
      const value = obj[key];
      
      if (typeof value === 'string') {
        sanitized[key] = sanitizeString(value) as any;
      } else if (typeof value === 'number') {
        sanitized[key] = value;
      } else if (typeof value === 'boolean') {
        sanitized[key] = value;
      } else if (value === null) {
        sanitized[key] = null as any;
      } else if (Array.isArray(value)) {
        sanitized[key] = value.map((item: any) =>
          typeof item === 'object' && item !== null
            ? sanitizeObject(item)
            : sanitizeString(String(item))
        ) as any;
      } else if (typeof value === 'object') {
        sanitized[key] = sanitizeObject(value) as any;
      }
    }
  }
  
  return sanitized;
}

/**
 * Validate backtest form parameters
 */
export interface BacktestFormValidation {
  symbol: boolean;
  exchange: boolean;
  initial_capital: boolean;
  population_size: boolean;
  generations: boolean;
}

export function validateBacktestParams(params: any): {
  valid: boolean;
  errors: string[];
} {
  const errors: string[] = [];
  
  // Required fields
  if (!params.symbol || !validateSymbol(params.symbol)) {
    errors.push('Invalid symbol format');
  }
  
  if (!params.exchange || !validateExchange(params.exchange)) {
    errors.push('Invalid exchange format');
  }
  
  if (!params.strategy_type || typeof params.strategy_type !== 'string') {
    errors.push('Strategy type is required');
  }
  
  // Numeric validations
  try {
    sanitizeNumber(params.initial_capital, 1000, 100000000);
  } catch (e) {
    errors.push('Initial capital must be between 1,000 and 100,000,000');
  }
  
  if (params.optimization_method === 'genetic') {
    try {
      sanitizeNumber(params.population_size, 5, 100);
    } catch (e) {
      errors.push('Population size must be between 5 and 100');
    }
    
    try {
      sanitizeNumber(params.generations, 3, 50);
    } catch (e) {
      errors.push('Generations must be between 3 and 50');
    }
  } else if (params.optimization_method === 'manual') {
    // Validate manual parameters
    const manualParams = [
      { name: 'risk_aversion', min: 0.01, max: 10 },
      { name: 'inventory_target', min: 0, max: 1 },
      { name: 'order_size', min: 0.001, max: 100 },
      { name: 'window_ms', min: 100, max: 60000 },
      { name: 'min_quote_lifetime_ms', min: 10, max: 10000 },
      { name: 'fee_rate', min: 0, max: 0.1 },
      { name: 'latency_buffer', min: 0, max: 1000 },
      { name: 'volatility_cap', min: 0.001, max: 1 },
      { name: 'time_horizon_minutes', min: 1, max: 1440 },
      { name: 'market_depth_k', min: 0.1, max: 10 },
      { name: 'volatility_adjustment_factor', min: 0, max: 2 },
      { name: 'inventory_penalty_factor', min: 0, max: 2 },
      { name: 'trend_sensitivity', min: -1, max: 1 },
      { name: 'flow_sensitivity', min: -1, max: 1 },
    ];
    
    for (const param of manualParams) {
      try {
        sanitizeNumber(params[param.name], param.min, param.max);
      } catch (e) {
        errors.push(`${param.name} must be between ${param.min} and ${param.max}`);
      }
    }
  }
  
  return {
    valid: errors.length === 0,
    errors,
  };
}

/**
 * Escape HTML to prevent XSS when rendering user content
 */
export function escapeHtml(unsafe: string): string {
  return sanitizeString(unsafe);
}

/**
 * Safe JSON parse with error handling
 */
export function safeJsonParse<T>(json: string, fallback: T): T {
  try {
    return JSON.parse(json);
  } catch {
    return fallback;
  }
}
