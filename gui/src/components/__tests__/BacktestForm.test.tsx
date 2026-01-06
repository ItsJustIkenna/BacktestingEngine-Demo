import React from 'react';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom';
import { BacktestForm } from '../BacktestForm';

// Mock fetch globally
global.fetch = jest.fn();

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

  test('renders form with accordion sections', async () => {
    render(<BacktestForm />);
    
    // Wait for metadata to load
    await waitFor(() => {
      expect(screen.getByText(/portfolio assets/i)).toBeInTheDocument();
    });

    // Check accordion sections are present
    expect(screen.getByText(/^strategy$/i)).toBeInTheDocument();
    expect(screen.getByText(/capital & allocation/i)).toBeInTheDocument();
    expect(screen.getByText(/optimization method/i)).toBeInTheDocument();
    expect(screen.getByText(/analysis & validation/i)).toBeInTheDocument();
  });

  test('fetches and displays symbols and exchanges in portfolio section', async () => {
    render(<BacktestForm />);

    await waitFor(() => {
      // The portfolio assets section should have symbol and exchange dropdowns
      const selects = screen.getAllByRole('combobox');
      expect(selects.length).toBeGreaterThanOrEqual(2);
    });
  });

  test('can add assets to portfolio', async () => {
    render(<BacktestForm />);

    await waitFor(() => {
      expect(screen.getByText(/portfolio assets/i)).toBeInTheDocument();
    });

    // Click the Add button to add an asset
    const addButton = screen.getByRole('button', { name: /\+ add/i });
    fireEvent.click(addButton);

    // Should show the asset count update in the header
    await waitFor(() => {
      expect(screen.getByText(/1.*asset.*configured/i)).toBeInTheDocument();
    });
  });

  test('can remove assets from portfolio', async () => {
    render(<BacktestForm />);

    await waitFor(() => {
      expect(screen.getByText(/portfolio assets/i)).toBeInTheDocument();
    });

    // Add an asset
    const addButton = screen.getByRole('button', { name: /\+ add/i });
    fireEvent.click(addButton);

    await waitFor(() => {
      expect(screen.getByText(/1.*asset.*configured/i)).toBeInTheDocument();
    });

    // Remove the asset using title attribute
    const removeButton = screen.getByTitle('Remove');
    fireEvent.click(removeButton);

    // Should show empty state
    await waitFor(() => {
      expect(screen.getByText(/no assets added yet/i)).toBeInTheDocument();
    });
  });

  test('shows genetic algorithm in optimization section badge', async () => {
    render(<BacktestForm />);

    await waitFor(() => {
      expect(screen.getByText(/optimization method/i)).toBeInTheDocument();
    });

    // Genetic Algorithm is mentioned in the Strategy section and shown as a badge on optimization
    const gaElements = screen.getAllByText(/genetic algorithm/i);
    expect(gaElements.length).toBeGreaterThan(0);
  });

  test('can expand optimization section to see settings', async () => {
    render(<BacktestForm />);

    await waitFor(() => {
      expect(screen.getByText(/optimization method/i)).toBeInTheDocument();
    });

    // Click to expand the optimization section
    const optimizationHeader = screen.getByText(/optimization method/i);
    fireEvent.click(optimizationHeader);

    // After expanding, population size and generations should be visible
    await waitFor(() => {
      expect(screen.getByText(/population size/i)).toBeInTheDocument();
      expect(screen.getByText(/generations/i)).toBeInTheDocument();
    });
  });

  test('submits form with portfolio assets', async () => {
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
      expect(screen.getByText(/portfolio assets/i)).toBeInTheDocument();
    });

    // Add an asset first
    const addButton = screen.getByRole('button', { name: /\+ add/i });
    fireEvent.click(addButton);

    await waitFor(() => {
      expect(screen.getByText(/1.*asset.*configured/i)).toBeInTheDocument();
    });

    const submitButton = screen.getByRole('button', { name: /run backtest/i });
    fireEvent.click(submitButton);

    await waitFor(() => {
      expect(mockOnJobSubmit).toHaveBeenCalledWith('test-job-123');
    });
  });

  test('shows validation warning when no assets configured', async () => {
    render(<BacktestForm />);

    await waitFor(() => {
      expect(screen.getByText(/portfolio assets/i)).toBeInTheDocument();
    });

    // Without any assets added, there should be a warning message
    expect(screen.getByText(/add at least one asset/i)).toBeInTheDocument();
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
      expect(screen.getByText(/portfolio assets/i)).toBeInTheDocument();
    });

    // Add an asset first
    const addButton = screen.getByRole('button', { name: /\+ add/i });
    fireEvent.click(addButton);

    await waitFor(() => {
      expect(screen.getByText(/1.*asset.*configured/i)).toBeInTheDocument();
    });

    const submitButton = screen.getByRole('button', { name: /run backtest/i });
    fireEvent.click(submitButton);

    await waitFor(() => {
      expect(screen.getByText(/submission failed/i)).toBeInTheDocument();
    });
  });

  test('disables submit button while loading', async () => {
    render(<BacktestForm />);

    await waitFor(() => {
      expect(screen.getByText(/portfolio assets/i)).toBeInTheDocument();
    });

    // Add an asset
    const addButton = screen.getByRole('button', { name: /\+ add/i });
    fireEvent.click(addButton);

    await waitFor(() => {
      expect(screen.getByText(/1.*asset.*configured/i)).toBeInTheDocument();
    });

    const submitButton = screen.getByRole('button', { name: /run backtest/i });
    expect(submitButton).not.toBeDisabled();

    fireEvent.click(submitButton);

    // Button should be disabled during submission
    await waitFor(() => {
      expect(submitButton).toBeDisabled();
    });
  });

  test('displays loading state for dropdowns', async () => {
    (global.fetch as jest.Mock).mockImplementation(() => new Promise(() => {})); // Never resolves

    render(<BacktestForm />);

    // Should show loading states
    await waitFor(() => {
      expect(screen.getAllByText(/loading\.\.\./i).length).toBeGreaterThan(0);
    });
  });

  test('handles metadata fetch failure gracefully', async () => {
    const consoleError = jest.spyOn(console, 'error').mockImplementation(() => {});
    (global.fetch as jest.Mock).mockRejectedValue(new Error('Network error'));

    render(<BacktestForm />);

    await waitFor(() => {
      expect(screen.getByText(/portfolio assets/i)).toBeInTheDocument();
    });

    consoleError.mockRestore();
  });

  test('genetic algorithm section shows population and generation inputs when expanded', async () => {
    render(<BacktestForm />);

    await waitFor(() => {
      expect(screen.getByText(/optimization method/i)).toBeInTheDocument();
    });

    // Expand the optimization section first
    fireEvent.click(screen.getByText(/optimization method/i));

    await waitFor(() => {
      expect(screen.getByText(/population size/i)).toBeInTheDocument();
    });

    expect(screen.getByText(/generations/i)).toBeInTheDocument();
  });

  test('can switch to manual parameters mode', async () => {
    render(<BacktestForm />);

    await waitFor(() => {
      expect(screen.getByText(/optimization method/i)).toBeInTheDocument();
    });

    // Click to expand the optimization section
    const optimizationHeader = screen.getByText(/optimization method/i);
    fireEvent.click(optimizationHeader);

    await waitFor(() => {
      expect(screen.getByText(/manual parameters/i)).toBeInTheDocument();
    });

    // Click on Manual Parameters label
    const manualLabel = screen.getByText(/manual parameters/i).closest('label');
    if (manualLabel) {
      fireEvent.click(manualLabel);
    }

    // After selecting manual, risk aversion and other fields should appear
    await waitFor(() => {
      expect(screen.getByText(/risk aversion/i)).toBeInTheDocument();
      expect(screen.getByText(/inventory target/i)).toBeInTheDocument();
      expect(screen.getByText(/order size/i)).toBeInTheDocument();
    });
  });

  test('strategy section is present with market making info', async () => {
    render(<BacktestForm />);

    await waitFor(() => {
      expect(screen.getByText(/portfolio assets/i)).toBeInTheDocument();
    });

    // Check that strategy section exists
    expect(screen.getByText(/^strategy$/i)).toBeInTheDocument();
    // Check market making strategy info is shown (may appear in dropdown and description)
    expect(screen.getAllByText(/avellaneda-stoikov/i).length).toBeGreaterThan(0);
  });

  test('adding assets updates active exchanges display', async () => {
    render(<BacktestForm />);

    await waitFor(() => {
      expect(screen.getByText(/portfolio assets/i)).toBeInTheDocument();
    });

    // Add an asset with coinbase
    const addButton = screen.getByRole('button', { name: /\+ add/i });
    fireEvent.click(addButton);

    await waitFor(() => {
      expect(screen.getByText(/1.*asset.*configured/i)).toBeInTheDocument();
    });

    // The active exchanges display should show the exchange
    expect(screen.getByText(/active exchanges/i)).toBeInTheDocument();
  });
});
