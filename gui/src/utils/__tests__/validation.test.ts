import {
  sanitizeString,
  sanitizeNumber,
  validateSymbol,
  validateExchange,
  validateUUID,
  validateEmail,
  validateBacktestParams,
  sanitizeObject,
} from '../validation';

describe('Validation Utilities', () => {
  describe('sanitizeString', () => {
    test('escapes HTML special characters', () => {
      const input = '<script>alert("XSS")</script>';
      const expected = '&lt;script&gt;alert(&quot;XSS&quot;)&lt;&#x2F;script&gt;';
      expect(sanitizeString(input)).toBe(expected);
    });

    test('handles empty string', () => {
      expect(sanitizeString('')).toBe('');
    });

    test('escapes ampersands', () => {
      expect(sanitizeString('A & B')).toBe('A &amp; B');
    });
  });

  describe('sanitizeNumber', () => {
    test('parses valid number', () => {
      expect(sanitizeNumber('123')).toBe(123);
      expect(sanitizeNumber(456.789)).toBe(456.789);
    });

    test('throws error for invalid number', () => {
      expect(() => sanitizeNumber('abc')).toThrow('Invalid number');
    });

    test('enforces minimum value', () => {
      expect(() => sanitizeNumber(5, 10)).toThrow('Number must be at least 10');
    });

    test('enforces maximum value', () => {
      expect(() => sanitizeNumber(100, 0, 50)).toThrow('Number must be at most 50');
    });

    test('allows number within range', () => {
      expect(sanitizeNumber(25, 0, 100)).toBe(25);
    });
  });

  describe('validateSymbol', () => {
    test('accepts valid symbols', () => {
      expect(validateSymbol('BTC/USD')).toBe(true);
      expect(validateSymbol('ETH-USDT')).toBe(true);
      expect(validateSymbol('AAPL')).toBe(true);
    });

    test('rejects invalid symbols', () => {
      expect(validateSymbol('')).toBe(false);
      expect(validateSymbol('BTC USD')).toBe(false); // space
      expect(validateSymbol('BTC<script>')).toBe(false); // special char
    });

    test('rejects too long symbols', () => {
      expect(validateSymbol('A'.repeat(21))).toBe(false);
    });
  });

  describe('validateExchange', () => {
    test('accepts valid exchanges', () => {
      expect(validateExchange('BINANCE')).toBe(true);
      expect(validateExchange('COINBASE_PRO')).toBe(true);
    });

    test('rejects invalid exchanges', () => {
      expect(validateExchange('')).toBe(false);
      expect(validateExchange('BINANCE SPOT')).toBe(false); // space
      expect(validateExchange('COIN-BASE')).toBe(false); // dash
    });
  });

  describe('validateUUID', () => {
    test('accepts valid UUIDs', () => {
      expect(validateUUID('550e8400-e29b-41d4-a716-446655440000')).toBe(true);
    });

    test('rejects invalid UUIDs', () => {
      expect(validateUUID('')).toBe(false);
      expect(validateUUID('invalid-uuid')).toBe(false);
      expect(validateUUID('550e8400-e29b-41d4-a716')).toBe(false); // incomplete
    });
  });

  describe('validateEmail', () => {
    test('accepts valid emails', () => {
      expect(validateEmail('test@example.com')).toBe(true);
      expect(validateEmail('user+tag@domain.co.uk')).toBe(true);
    });

    test('rejects invalid emails', () => {
      expect(validateEmail('')).toBe(false);
      expect(validateEmail('notanemail')).toBe(false);
      expect(validateEmail('@domain.com')).toBe(false);
      expect(validateEmail('user@')).toBe(false);
    });
  });

  describe('sanitizeObject', () => {
    test('sanitizes string values in object', () => {
      const input = {
        name: '<script>alert("XSS")</script>',
        value: 123,
      };
      const result = sanitizeObject(input);
      expect(result.name).toContain('&lt;script&gt;');
      expect(result.value).toBe(123);
    });

    test('handles nested objects', () => {
      const input = {
        user: {
          name: '<b>Test</b>',
        },
      };
      const result = sanitizeObject(input);
      expect(result.user.name).toContain('&lt;b&gt;');
    });

    test('handles arrays', () => {
      const input = {
        items: ['<script>', 'safe', { value: '<div>' }],
      };
      const result = sanitizeObject(input);
      expect(result.items[0]).toContain('&lt;script&gt;');
      expect(result.items[1]).toBe('safe');
    });
  });

  describe('validateBacktestParams', () => {
    const validGeneticParams = {
      symbol: 'BTC/USD',
      exchange: 'BINANCE',
      strategy_type: 'market_making',
      initial_capital: 100000,
      optimization_method: 'genetic',
      population_size: 50,
      generations: 20,
    };

    test('accepts valid genetic parameters', () => {
      const result = validateBacktestParams(validGeneticParams);
      expect(result.valid).toBe(true);
      expect(result.errors).toHaveLength(0);
    });

    test('rejects invalid symbol', () => {
      const params = { ...validGeneticParams, symbol: 'INVALID SYMBOL' };
      const result = validateBacktestParams(params);
      expect(result.valid).toBe(false);
      expect(result.errors).toContain('Invalid symbol format');
    });

    test('rejects invalid initial capital', () => {
      const params = { ...validGeneticParams, initial_capital: 100 };
      const result = validateBacktestParams(params);
      expect(result.valid).toBe(false);
      expect(result.errors.some(e => e.includes('Initial capital'))).toBe(true);
    });

    test('rejects invalid population size', () => {
      const params = { ...validGeneticParams, population_size: 200 };
      const result = validateBacktestParams(params);
      expect(result.valid).toBe(false);
      expect(result.errors.some(e => e.includes('Population size'))).toBe(true);
    });

    test('validates manual parameters', () => {
      const manualParams = {
        symbol: 'ETH/USD',
        exchange: 'COINBASE',
        strategy_type: 'market_making',
        initial_capital: 50000,
        optimization_method: 'manual',
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
      };

      const result = validateBacktestParams(manualParams);
      expect(result.valid).toBe(true);
      expect(result.errors).toHaveLength(0);
    });

    test('rejects out-of-range manual parameters', () => {
      const manualParams = {
        symbol: 'ETH/USD',
        exchange: 'COINBASE',
        strategy_type: 'market_making',
        initial_capital: 50000,
        optimization_method: 'manual',
        risk_aversion: 50, // Too high
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
      };

      const result = validateBacktestParams(manualParams);
      expect(result.valid).toBe(false);
      expect(result.errors.some(e => e.includes('risk_aversion'))).toBe(true);
    });
  });
});
