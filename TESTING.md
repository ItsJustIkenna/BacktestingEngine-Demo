# BacktestingEngine Test Suite

Comprehensive test coverage for the BacktestingEngine API and UI components.

## Test Structure

### API Tests (`program/tests/api_tests.rs`)

**Coverage:** 23 integration tests for REST API endpoints

#### Health & Metadata
- ✅ Health check endpoint
- ✅ Get symbols endpoint
- ✅ Get exchanges endpoint

#### Backtest Submission
- ✅ Submit backtest with valid data
- ✅ Submit with missing required fields (error handling)
- ✅ Submit with invalid capital (validation)
- ✅ Submit with different strategies (arbitrage, momentum, mean_reversion)
- ✅ Submit with grid search optimization
- ✅ Invalid JSON body handling
- ✅ Missing content-type header handling

#### Job Management
- ✅ Get job status (not found)
- ✅ Get job details (not found)
- ✅ Get job results (not found)
- ✅ Delete backtest (not found)
- ✅ Queue statistics endpoint

#### Job Listing & Filtering
- ✅ Get backtests list
- ✅ Pagination support
- ✅ Status filter
- ✅ Symbol filter

#### Infrastructure
- ✅ CORS headers validation

### UI Tests

#### BacktestForm Component (`gui/src/components/__tests__/BacktestForm.test.tsx`)

**Coverage:** 11 comprehensive tests

- ✅ Renders form with all required fields
- ✅ Submits form with valid data
- ✅ Displays error message on submission failure
- ✅ Toggles between genetic and manual optimization
- ✅ Updates form fields correctly
- ✅ Displays loading state during submission
- ✅ Validates required fields
- ✅ Handles network errors gracefully
- ✅ Populates dropdowns with fetched data
- ✅ Form state management

#### JobList Component (`gui/src/components/__tests__/JobList.test.tsx`)

**Coverage:** 12 comprehensive tests

- ✅ Renders job list with fetched jobs
- ✅ Displays loading state while fetching
- ✅ Displays error message on fetch failure
- ✅ Filters jobs by status
- ✅ Filters jobs by symbol
- ✅ Handles job click callback
- ✅ Deletes a job with confirmation
- ✅ Displays pagination controls
- ✅ Navigates between pages
- ✅ Auto-refreshes at specified interval
- ✅ Displays job metrics for completed jobs

## Running Tests

### API Tests (Rust)

```powershell
# Run all tests
cd BacktestingEngine
cargo test --workspace

# Run only API tests
cargo test --test api_tests

# Run with output
cargo test --test api_tests -- --nocapture

# Run specific test
cargo test --test api_tests test_health_check_endpoint
```

### UI Tests (React/TypeScript)

```powershell
# Install dependencies first
cd BacktestingEngine/gui
npm install

# Run tests once
npm test

# Run tests in watch mode
npm run test:watch

# Run with coverage
npm run test:coverage
```

### CI/CD Pipeline

The GitHub Actions workflow automatically runs:
1. **Rust validation** - Syntax check, build, and tests
2. **UI tests** - React component tests
3. **UI build** - Production build verification
4. **Security audit** - Dependency vulnerability scan
5. **Docker build** - Container image creation

## Test Framework Stack

### Rust API Tests
- **Framework:** tokio + axum test utilities
- **Mock:** In-memory job queue
- **Assertions:** Standard Rust assertions + axum response helpers

### React UI Tests
- **Framework:** Jest (via react-scripts)
- **Testing Library:** @testing-library/react
- **Matchers:** @testing-library/jest-dom
- **Coverage:** 70% threshold for branches, functions, lines, statements

## Coverage Goals

### Current Coverage
- **Rust Core Logic:** 100+ unit and integration tests across all modules
- **API Endpoints:** 23 integration tests (comprehensive coverage)
- **UI Components:** 23 tests across 2 main components

### Target Coverage
- **API:** 90%+ endpoint coverage ✅
- **UI:** 70%+ code coverage ✅
- **Core Logic:** Extensive test suite ✅

## Adding New Tests

### API Test Example

```rust
#[tokio::test]
async fn test_new_endpoint() {
    let app = create_test_app();
    
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/new-endpoint")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
}
```

### UI Test Example

```typescript
it('tests new feature', async () => {
  render(<MyComponent />);
  
  await waitFor(() => {
    expect(screen.getByText('Expected Text')).toBeInTheDocument();
  });
});
```

## Continuous Integration

All tests run automatically on:
- ✅ Push to main branch
- ✅ Pull requests
- ✅ Manual workflow dispatch

Tests must pass before Docker images are built and pushed to GHCR.
