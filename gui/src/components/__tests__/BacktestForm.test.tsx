import React from 'react';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom';
import { BacktestForm } from '../BacktestForm';

// Mock fetch globally
global.fetch = jest.fn();

// Helper to get the main symbol select (not portfolio header)
const getSymbolSelect = () => screen.getByRole('combobox', { name: /^symbol$/i }) as HTMLSelectElement;
const getExchangeSelect = () => screen.getByRole('combobox', { name: /^exchange$/i }) as HTMLSelectElement;

describe('BacktestForm', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    (global.fetch as jest.Mock).mockImplementation((url: string) => {
      if (url === '/api/symbols') {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ data: [{ symbol: 'BTC-USD' }, { symbol: 'ETH-USD' }] }),
        });
      }
      if (url === '/api/exchanges') {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ data: [{ exchange: 'coinbase' }, { exchange: 'binance' }] }),
        });
      }
      return Promise.resolve({ ok: false });
    });
  });

  test('renders form with all required fields', async () => {
    render(<BacktestForm />);
    
    await waitFor(() => {
      expect(getSymbolSelect()).toBeInTheDocument();
    });

    expect(getExchangeSelect()).toBeInTheDocument();
    expect(screen.getByLabelText(/strategy type/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/initial capital/i)).toBeInTheDocument();
    expect(screen.getByText(/optimization method/i)).toBeInTheDocument();
  });

  test('fetches and displays symbols and exchanges', async () => {
    render(<BacktestForm />);

    await waitFor(() => {
      const symbolSelect = getSymbolSelect();
      expect(symbolSelect.options).toHaveLength(2);
      expect(symbolSelect.options[0]!.value).toBe('BTC-USD');
      expect(symbolSelect.options[1]!.value).toBe('ETH-USD');
    });

    const exchangeSelect = getExchangeSelect();
    expect(exchangeSelect.options).toHaveLength(2);
    expect(exchangeSelect.options[0]!.value).toBe('coinbase');
    expect(exchangeSelect.options[1]!.value).toBe('binance');
  });

  test('shows genetic algorithm parameters by default', async () => {
    render(<BacktestForm />);

    await waitFor(() => {
      expect(screen.getByLabelText(/population size/i)).toBeInTheDocument();
    });

    expect(screen.getByLabelText(/generations/i)).toBeInTheDocument();
    expect(screen.queryByLabelText(/risk aversion/i)).not.toBeInTheDocument();
  });

  test('switches to manual parameters when manual radio is selected', async () => {
    render(<BacktestForm />);

    await waitFor(() => {
      expect(getSymbolSelect()).toBeInTheDocument();
    });

    const manualRadio = screen.getByRole('radio', { name: /manual parameters/i });
    fireEvent.click(manualRadio);

    await waitFor(() => {
      expect(screen.getByLabelText(/risk aversion/i)).toBeInTheDocument();
    });

    expect(screen.getByLabelText(/inventory target/i)).toBeInTheDocument();
    expect(screen.queryByLabelText(/population size/i)).not.toBeInTheDocument();
  });

  test('submits form with genetic algorithm parameters', async () => {
    const mockOnJobSubmit = jest.fn();
    (global.fetch as jest.Mock).mockImplementation((url: string, options?: any) => {
      if (url === '/api/symbols') {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ data: [{ symbol: 'BTC-USD' }] }),
        });
      }
      if (url === '/api/exchanges') {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ data: [{ exchange: 'coinbase' }] }),
        });
      }
      if (url === '/api/backtest' && options?.method === 'POST') {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ job_id: 'test-job-123' }),
        });
      }
      return Promise.resolve({ ok: false });
    });

    render(<BacktestForm onJobSubmit={mockOnJobSubmit} />);

    await waitFor(() => {
      expect(getSymbolSelect()).toBeInTheDocument();
    });

    const submitButton = screen.getByRole('button', { name: /submit backtest/i });
    fireEvent.click(submitButton);

    await waitFor(() => {
      expect(mockOnJobSubmit).toHaveBeenCalledWith('test-job-123');
    });
  });

  test('displays error message when submission fails', async () => {
    (global.fetch as jest.Mock).mockImplementation((url: string, options?: any) => {
      if (url === '/api/symbols') {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ data: [{ symbol: 'BTC-USD' }] }),
        });
      }
      if (url === '/api/exchanges') {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ data: [{ exchange: 'coinbase' }] }),
        });
      }
      if (url === '/api/backtest' && options?.method === 'POST') {
        return Promise.resolve({
          ok: false,
          json: () => Promise.resolve({ error: 'Submission failed' }),
        });
      }
      return Promise.resolve({ ok: false });
    });

    render(<BacktestForm />);

    await waitFor(() => {
      expect(getSymbolSelect()).toBeInTheDocument();
    });

    const submitButton = screen.getByRole('button', { name: /submit backtest/i });
    fireEvent.click(submitButton);

    await waitFor(() => {
      expect(screen.getByText(/submission failed/i)).toBeInTheDocument();
    });
  });

  test('disables submit button while loading', async () => {
    render(<BacktestForm />);

    await waitFor(() => {
      expect(getSymbolSelect()).toBeInTheDocument();
    });

    const submitButton = screen.getByRole('button', { name: /submit backtest/i });
    expect(submitButton).not.toBeDisabled();

    fireEvent.click(submitButton);

    // Button should be disabled during submission
    await waitFor(() => {
      expect(submitButton).toBeDisabled();
    });
  });

  test('updates form data when inputs change', async () => {
    render(<BacktestForm />);

    await waitFor(() => {
      expect(getSymbolSelect()).toBeInTheDocument();
    });

    const capitalInput = screen.getByLabelText(/initial capital/i) as HTMLInputElement;
    fireEvent.change(capitalInput, { target: { value: '50000' } });

    expect(capitalInput.value).toBe('50000');
  });

  test('displays loading state for dropdowns', () => {
    (global.fetch as jest.Mock).mockImplementation(() => new Promise(() => {})); // Never resolves

    render(<BacktestForm />);

    const symbolSelect = getSymbolSelect();
    expect(symbolSelect).toBeDisabled();
    expect(screen.getAllByText(/loading\.\.\./i).length).toBeGreaterThan(0);
  });

  test('handles metadata fetch failure gracefully', async () => {
    const consoleError = jest.spyOn(console, 'error').mockImplementation(() => {});
    (global.fetch as jest.Mock).mockRejectedValue(new Error('Network error'));

    render(<BacktestForm />);

    await waitFor(() => {
      const symbolSelect = getSymbolSelect();
      expect(symbolSelect).not.toBeDisabled();
    });

    consoleError.mockRestore();
  });

  test('genetic algorithm has default values', async () => {
    render(<BacktestForm />);

    await waitFor(() => {
      expect(screen.getByLabelText(/population size/i)).toBeInTheDocument();
    });

    const popSize = screen.getByLabelText(/population size/i) as HTMLInputElement;
    const generations = screen.getByLabelText(/generations/i) as HTMLInputElement;

    expect(popSize.value).toBe('50');
    expect(generations.value).toBe('20');
  });

  test('manual parameters are accessible when manual mode is selected', async () => {
    render(<BacktestForm />);

    await waitFor(() => {
      expect(getSymbolSelect()).toBeInTheDocument();
    });

    const manualRadio = screen.getByRole('radio', { name: /manual parameters/i });
    fireEvent.click(manualRadio);

    await waitFor(() => {
      expect(screen.getByLabelText(/risk aversion/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/inventory target/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/order size/i)).toBeInTheDocument();
    });
  });
});
