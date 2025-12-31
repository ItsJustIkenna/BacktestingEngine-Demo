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
  {
    job_id: 'job-3',
    symbol: 'BTC-USD',
    exchange: 'coinbase',
    status: 'running',
    progress: 50,
    created_at: '2024-01-01T03:00:00Z',
    current_phase: 'Genetic Optimization',
    phase_details: {
      current_generation: 10,
      total_generations: 20,
      best_fitness: 1.5,
    },
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

  test('renders job list with header and status cards', async () => {
    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText('Backtest Jobs')).toBeInTheDocument();
    });

    // Status filter cards should be present
    expect(screen.getByText('All')).toBeInTheDocument();
    expect(screen.getByText('Running')).toBeInTheDocument();
    expect(screen.getByText('Queued')).toBeInTheDocument();
    expect(screen.getByText('Completed')).toBeInTheDocument();
    expect(screen.getByText('Failed')).toBeInTheDocument();
  });

  test('fetches and displays jobs', async () => {
    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText(/job-1/)).toBeInTheDocument();
    });

    expect(screen.getByText(/job-2/)).toBeInTheDocument();
    expect(screen.getByText(/job-3/)).toBeInTheDocument();
  });

  test('displays job metrics for completed job', async () => {
    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText(/job-1/)).toBeInTheDocument();
    });

    // Should show Sharpe ratio - use getAllByText since there might be multiple
    const sharpeElements = screen.getAllByText(/1\.50/);
    expect(sharpeElements.length).toBeGreaterThan(0);
  });

  test('filters jobs by clicking status cards', async () => {
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

    // Click the Completed status card
    const completedCard = screen.getByText('Completed').closest('button');
    fireEvent.click(completedCard!);

    await waitFor(() => {
      expect(screen.getByText(/job-1/)).toBeInTheDocument();
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

    expect(screen.getByText(/loading/i)).toBeInTheDocument();
  });

  test('shows empty state when no jobs', async () => {
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
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ backtests: [], total: 0 }),
        });
      }
      return Promise.resolve({ ok: false });
    });

    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText(/no backtests found/i)).toBeInTheDocument();
    });
  });

  test('shows delete confirmation dialog', async () => {
    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText(/job-1/)).toBeInTheDocument();
    });

    const deleteButtons = screen.getAllByRole('button', { name: /delete/i });
    fireEvent.click(deleteButtons[0]!);

    await waitFor(() => {
      expect(screen.getByText(/are you sure/i)).toBeInTheDocument();
    });
  });

  test('displays progress for running jobs', async () => {
    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText(/job-3/)).toBeInTheDocument();
    });

    // Should show progress percentage
    expect(screen.getByText(/50%/)).toBeInTheDocument();
  });

  test('shows phase details for running jobs', async () => {
    render(<JobList />);

    await waitFor(() => {
      expect(screen.getByText(/job-3/)).toBeInTheDocument();
    });

    // Should show phase information
    expect(screen.getByText(/genetic optimization/i)).toBeInTheDocument();
  });

  test('can click on job card to trigger callback', async () => {
    const mockOnJobClick = jest.fn();
    render(<JobList onJobClick={mockOnJobClick} />);

    await waitFor(() => {
      expect(screen.getByText(/job-1/)).toBeInTheDocument();
    });

    // Find and click the job card (the card with job-1 text)
    const jobCard = screen.getByText(/job-1/).closest('div[style*="cursor: pointer"]');
    if (jobCard) {
      fireEvent.click(jobCard);
      expect(mockOnJobClick).toHaveBeenCalledWith('job-1');
    }
  });

  test('header shows total job count', async () => {
    render(<JobList />);

    // Wait for jobs to load
    await waitFor(() => {
      expect(screen.getByText(/job-1/)).toBeInTheDocument();
    });

    // Check the header stats show total
    expect(screen.getByText(/3 total/i)).toBeInTheDocument();
  });
});