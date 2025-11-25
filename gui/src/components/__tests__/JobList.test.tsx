import React from 'react';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom';
import { JobList } from '../JobList';

// Mock fetch globally
global.fetch = jest.fn();

const mockJobs = [
  {
    job_id: 'job-1',
    symbol: 'BTC-USD',
    exchange: 'coinbase',
    status: 'completed',
    progress: 100,
    created_at: '2024-01-01T00:00:00Z',
    completed_at: '2024-01-01T01:00:00Z',
    sharpe_ratio: 1.5,
    max_drawdown: -0.15,
    total_pnl: 5000,
    win_rate: 0.65,
  },
  {
    job_id: 'job-2',
    symbol: 'ETH-USD',
    exchange: 'binance',
    status: 'queued',
    progress: 0,
    created_at: '2024-01-01T02:00:00Z',
  },
];

describe('JobList', () => {
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
      if (url.startsWith('/api/backtests')) {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ backtests: mockJobs, total: mockJobs.length }),
        });
      }
      return Promise.resolve({ ok: false });
    });
  });

  test('renders job list with table headers', async () => {
    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText('Backtest Jobs')).toBeInTheDocument();
    });

    expect(screen.getByText('Job ID')).toBeInTheDocument();
    expect(screen.getByText('Symbol')).toBeInTheDocument();
    expect(screen.getByText('Exchange')).toBeInTheDocument();
    expect(screen.getByText('Status')).toBeInTheDocument();
  });

  test('fetches and displays jobs', async () => {
    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText(/job-1/)).toBeInTheDocument();
    });

    expect(screen.getByText(/job-2/)).toBeInTheDocument();
    
    // Query within table to avoid dropdown options
    const table = screen.getByRole('table');
    expect(table).toHaveTextContent('BTC-USD');
    expect(table).toHaveTextContent('ETH-USD');
  });

  test('displays job status badges', async () => {
    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText('COMPLETED')).toBeInTheDocument();
    });

    expect(screen.getByText('QUEUED')).toBeInTheDocument();
  });

  test('filters jobs by status', async () => {
    (global.fetch as jest.Mock).mockImplementation((url: string) => {
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
      if (url.startsWith('/api/backtests')) {
        // Return only completed jobs when filtered
        const filteredJobs = url.includes('status=completed') 
          ? [mockJobs[0]] 
          : mockJobs;
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ backtests: filteredJobs, total: filteredJobs.length }),
        });
      }
      return Promise.resolve({ ok: false });
    });

    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText(/job-1/)).toBeInTheDocument();
    });

    const statusFilter = screen.getByLabelText('Status:');
    fireEvent.change(statusFilter, { target: { value: 'completed' } });

    await waitFor(() => {
      expect(screen.getByText(/job-1/)).toBeInTheDocument();
      expect(screen.queryByText(/job-2/)).not.toBeInTheDocument();
    });
  });

  test('filters jobs by symbol', async () => {
    (global.fetch as jest.Mock).mockImplementation((url: string) => {
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
      if (url.startsWith('/api/backtests')) {
        const filteredJobs = url.includes('symbol=BTC-USD') 
          ? [mockJobs[0]] 
          : mockJobs;
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ backtests: filteredJobs, total: filteredJobs.length }),
        });
      }
      return Promise.resolve({ ok: false });
    });

    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText(/job-1/)).toBeInTheDocument();
    });

    const symbolFilter = screen.getByLabelText('Symbol:');
    fireEvent.change(symbolFilter, { target: { value: 'BTC-USD' } });

    await waitFor(() => {
      expect(screen.getByText(/job-1/)).toBeInTheDocument();
      expect(screen.queryByText('ETH-USD')).not.toBeInTheDocument();
    });
  });

  test('filters jobs by exchange', async () => {
    (global.fetch as jest.Mock).mockImplementation((url: string) => {
      if (url === '/api/symbols') {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ data: [{ symbol: 'ETH-USD' }] }),
        });
      }
      if (url === '/api/exchanges') {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ data: [{ exchange: 'binance' }] }),
        });
      }
      if (url.startsWith('/api/backtests')) {
        const filteredJobs = url.includes('exchange=binance') 
          ? [mockJobs[1]] 
          : mockJobs;
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ backtests: filteredJobs, total: filteredJobs.length }),
        });
      }
      return Promise.resolve({ ok: false });
    });

    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText(/job-2/)).toBeInTheDocument();
    });

    const exchangeFilter = screen.getByLabelText('Exchange:');
    fireEvent.change(exchangeFilter, { target: { value: 'binance' } });

    await waitFor(() => {
      expect(screen.getByText(/job-2/)).toBeInTheDocument();
      expect(screen.queryByText('BTC-USD')).not.toBeInTheDocument();
    });
  });

  test('refreshes job list when refresh button is clicked', async () => {
    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText(/job-1/)).toBeInTheDocument();
    });

    const initialFetchCount = (global.fetch as jest.Mock).mock.calls.filter(
      call => call[0].startsWith('/api/backtests')
    ).length;
    
    const refreshButton = screen.getByRole('button', { name: /refresh/i });
    fireEvent.click(refreshButton);

    await waitFor(() => {
      const newFetchCount = (global.fetch as jest.Mock).mock.calls.filter(
        call => call[0].startsWith('/api/backtests')
      ).length;
      expect(newFetchCount).toBeGreaterThan(initialFetchCount);
    });
  });

  test('displays error message when fetch fails', async () => {
    (global.fetch as jest.Mock).mockImplementation((url: string) => {
      if (url === '/api/symbols') {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ data: [] }),
        });
      }
      if (url === '/api/exchanges') {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ data: [] }),
        });
      }
      if (url.startsWith('/api/backtests')) {
        return Promise.resolve({ ok: false });
      }
      return Promise.resolve({ ok: false });
    });

    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText(/failed to fetch backtests/i)).toBeInTheDocument();
    });
  });

  test('shows loading indicator while fetching', () => {
    (global.fetch as jest.Mock).mockImplementation(() => new Promise(() => {})); // Never resolves

    render(<JobList />);

    expect(screen.getByText(/loading\.\.\./i)).toBeInTheDocument();
  });

  test('shows delete confirmation dialog', async () => {
    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText(/job-1/)).toBeInTheDocument();
    });

    const deleteButtons = screen.getAllByRole('button', { name: /delete/i });
    fireEvent.click(deleteButtons[0]);

    await waitFor(() => {
      expect(screen.getByText(/are you sure/i)).toBeInTheDocument();
    });

    const allDeleteButtons = screen.getAllByRole('button', { name: /delete/i });
    expect(allDeleteButtons.length).toBeGreaterThan(1); // Should have both table Delete buttons and modal Delete button
    expect(screen.getByRole('button', { name: /cancel/i })).toBeInTheDocument();
  });

  test('deletes job when confirmed', async () => {
    (global.fetch as jest.Mock).mockImplementation((url: string, options?: any) => {
      if (url === '/api/symbols') {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ data: [] }),
        });
      }
      if (url === '/api/exchanges') {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ data: [] }),
        });
      }
      if (url.startsWith('/api/backtest/') && options?.method === 'DELETE') {
        return Promise.resolve({ ok: true });
      }
      if (url.startsWith('/api/backtests')) {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ backtests: mockJobs, total: mockJobs.length }),
        });
      }
      return Promise.resolve({ ok: false });
    });

    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText(/job-1/)).toBeInTheDocument();
    });

    const deleteButtons = screen.getAllByRole('button', { name: /delete/i });
    fireEvent.click(deleteButtons[0]);

    await waitFor(() => {
      expect(screen.getByText(/are you sure/i)).toBeInTheDocument();
    });

    // After dialog opens, get all Delete buttons - last one is the confirm button in modal
    const allDeleteButtons = screen.getAllByRole('button', { name: /delete/i });
    const confirmButton = allDeleteButtons[allDeleteButtons.length - 1];
    fireEvent.click(confirmButton);

    await waitFor(() => {
      expect(global.fetch).toHaveBeenCalledWith(
        expect.stringContaining('/api/backtest/job-1'),
        expect.objectContaining({ method: 'DELETE' }),
      );
    });
  });

  test('cancels delete when cancel is clicked', async () => {
    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText(/job-1/)).toBeInTheDocument();
    });

    const deleteButtons = screen.getAllByRole('button', { name: /delete/i });
    fireEvent.click(deleteButtons[0]);

    await waitFor(() => {
      expect(screen.getByText(/are you sure/i)).toBeInTheDocument();
    });

    const cancelButton = screen.getByRole('button', { name: /cancel/i });
    fireEvent.click(cancelButton);

    await waitFor(() => {
      expect(screen.queryByText(/are you sure/i)).not.toBeInTheDocument();
    });
  });

  test('displays metrics for completed jobs', async () => {
    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText(/job-1/)).toBeInTheDocument();
    });

    const table = screen.getByRole('table');
    expect(table).toBeInTheDocument();
    expect(screen.getByText('COMPLETED')).toBeInTheDocument();
    expect(screen.getByText('100%')).toBeInTheDocument();
  });
});
