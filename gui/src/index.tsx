import React, { useState } from 'react';
import ReactDOM from 'react-dom/client';
import './index.css';
import BacktestForm from './components/BacktestForm';
import JobList from './components/JobList';
import ResultsVisualization from './components/ResultsVisualization';
import TardisUpload from './components/TardisUpload';
import ErrorBoundary from './components/ErrorBoundary';

const App: React.FC = () => {
  const [currentView, setCurrentView] = useState<'form' | 'list' | 'results' | 'tardis'>('list');
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null);

  const handleJobSubmit = (jobId: string) => {
    setSelectedJobId(jobId);
    setCurrentView('list');
  };

  const handleJobClick = (jobId: string) => {
    setSelectedJobId(jobId);
    setCurrentView('results');
  };

  const handleBackToList = () => {
    setCurrentView('list');
    setSelectedJobId(null);
  };

  const handleTardisBacktestSubmitted = (jobId: string) => {
    setSelectedJobId(jobId);
    setCurrentView('list');
  };

  return (
    <div className="app">
      <header style={{
        padding: '20px',
        backgroundColor: '#282c34',
        color: 'white',
        marginBottom: '20px',
      }}>
        <h1 style={{ margin: 0 }}>BacktestingEngine</h1>
        <nav style={{ marginTop: '10px', display: 'flex', gap: '15px' }}>
          <button
            onClick={() => setCurrentView('form')}
            style={{
              padding: '8px 16px',
              backgroundColor: currentView === 'form' ? '#61dafb' : '#444',
              color: 'white',
              border: 'none',
              borderRadius: '4px',
              cursor: 'pointer',
            }}
          >
            New Backtest
          </button>
          <button
            onClick={() => setCurrentView('tardis')}
            style={{
              padding: '8px 16px',
              backgroundColor: currentView === 'tardis' ? '#61dafb' : '#444',
              color: 'white',
              border: 'none',
              borderRadius: '4px',
              cursor: 'pointer',
            }}
          >
            📁 Tardis Data
          </button>
          <button
            onClick={() => setCurrentView('list')}
            style={{
              padding: '8px 16px',
              backgroundColor: currentView === 'list' ? '#61dafb' : '#444',
              color: 'white',
              border: 'none',
              borderRadius: '4px',
              cursor: 'pointer',
            }}
          >
            Job List
          </button>
        </nav>
      </header>

      <main>
        {currentView === 'form' && (
          <BacktestForm onJobSubmit={handleJobSubmit} />
        )}
        {currentView === 'tardis' && (
          <TardisUpload onBacktestSubmitted={handleTardisBacktestSubmitted} />
        )}
        {currentView === 'list' && (
          <JobList onJobClick={handleJobClick} />
        )}
        {currentView === 'results' && selectedJobId && (
          <ResultsVisualization jobId={selectedJobId} onBack={handleBackToList} />
        )}
      </main>
    </div>
  );
};

const root = ReactDOM.createRoot(
  document.getElementById('root') as HTMLElement
);
root.render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>
);
