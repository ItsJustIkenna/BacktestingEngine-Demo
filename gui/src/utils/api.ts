/**
 * Centralized API client with authentication, error handling, and request cancellation
 */

const API_BASE_URL = process.env.REACT_APP_API_URL || '/api';
const API_TIMEOUT = 30000; // 30 seconds

export interface ApiError {
  message: string;
  status?: number;
  code?: string;
}

class ApiClient {
  private baseUrl: string;
  private authToken: string | null = null;

  constructor(baseUrl: string) {
    this.baseUrl = baseUrl;
    this.loadAuthToken();
  }

  private loadAuthToken(): void {
    this.authToken = localStorage.getItem('authToken');
  }

  setAuthToken(token: string | null): void {
    this.authToken = token;
    if (token) {
      localStorage.setItem('authToken', token);
    } else {
      localStorage.removeItem('authToken');
    }
  }

  getAuthToken(): string | null {
    return this.authToken;
  }

  private async fetchWithTimeout(
    url: string,
    options: RequestInit = {},
    timeout: number = API_TIMEOUT,
    signal?: AbortSignal
  ): Promise<Response> {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), timeout);

    // Combine signals if an external one is provided
    const combinedSignal = signal
      ? this.combineSignals([signal, controller.signal])
      : controller.signal;

    try {
      const response = await fetch(url, {
        ...options,
        signal: combinedSignal,
      });
      clearTimeout(timeoutId);
      return response;
    } catch (error) {
      clearTimeout(timeoutId);
      throw error;
    }
  }

  private combineSignals(signals: AbortSignal[]): AbortSignal {
    const controller = new AbortController();
    signals.forEach((signal) => {
      if (signal.aborted) {
        controller.abort();
      } else {
        signal.addEventListener('abort', () => controller.abort(), { once: true });
      }
    });
    return controller.signal;
  }

  private getHeaders(): HeadersInit {
    const headers: HeadersInit = {
      'Content-Type': 'application/json',
    };

    if (this.authToken) {
      headers['Authorization'] = `Bearer ${this.authToken}`;
    }

    return headers;
  }

  private async handleResponse<T>(response: Response): Promise<T> {
    if (!response.ok) {
      const errorData = await response.json().catch(() => ({
        message: response.statusText || 'Unknown error',
      }));

      const error: ApiError = {
        message: errorData.error || errorData.message || 'Request failed',
        status: response.status,
        code: errorData.code,
      };

      // Handle specific status codes
      if (response.status === 401) {
        // Unauthorized - clear token
        this.setAuthToken(null);
        error.message = 'Unauthorized. Please log in again.';
      } else if (response.status === 403) {
        error.message = 'Access denied.';
      } else if (response.status === 404) {
        error.message = 'Resource not found.';
      } else if (response.status >= 500) {
        error.message = 'Server error. Please try again later.';
      }

      throw error;
    }

    const contentType = response.headers.get('content-type');
    if (contentType && contentType.includes('application/json')) {
      return response.json();
    }

    return response.text() as any;
  }

  async get<T>(endpoint: string, signal?: AbortSignal): Promise<T> {
    const url = `${this.baseUrl}${endpoint}`;
    const response = await this.fetchWithTimeout(
      url,
      {
        method: 'GET',
        headers: this.getHeaders(),
      },
      API_TIMEOUT,
      signal
    );
    return this.handleResponse<T>(response);
  }

  async post<T>(endpoint: string, data?: any, signal?: AbortSignal): Promise<T> {
    const url = `${this.baseUrl}${endpoint}`;
    const response = await this.fetchWithTimeout(
      url,
      {
        method: 'POST',
        headers: this.getHeaders(),
        body: data ? JSON.stringify(data) : undefined,
      },
      API_TIMEOUT,
      signal
    );
    return this.handleResponse<T>(response);
  }

  async put<T>(endpoint: string, data?: any, signal?: AbortSignal): Promise<T> {
    const url = `${this.baseUrl}${endpoint}`;
    const response = await this.fetchWithTimeout(
      url,
      {
        method: 'PUT',
        headers: this.getHeaders(),
        body: data ? JSON.stringify(data) : undefined,
      },
      API_TIMEOUT,
      signal
    );
    return this.handleResponse<T>(response);
  }

  async delete<T>(endpoint: string, signal?: AbortSignal): Promise<T> {
    const url = `${this.baseUrl}${endpoint}`;
    const response = await this.fetchWithTimeout(
      url,
      {
        method: 'DELETE',
        headers: this.getHeaders(),
      },
      API_TIMEOUT,
      signal
    );
    return this.handleResponse<T>(response);
  }
}

// Export singleton instance
export const apiClient = new ApiClient(API_BASE_URL);

// Export types
export type { ApiClient };
