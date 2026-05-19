import { Component } from 'react';

interface Props {
  children: React.ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error('React error boundary caught:', error, info);
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex items-center justify-center h-screen bg-white p-8">
          <div className="max-w-lg w-full">
            <h1 className="text-xl font-bold text-red-600 mb-4">应用出现错误</h1>
            <pre className="bg-red-50 border border-red-200 rounded-lg p-4 text-sm text-red-700 whitespace-pre-wrap overflow-auto max-h-96">
              {this.state.error?.name}: {this.state.error?.message}
              {'\n\n'}
              {this.state.error?.stack}
            </pre>
            <button
              onClick={() => { this.setState({ hasError: false, error: null }); window.location.reload(); }}
              className="mt-4 bg-purple-600 hover:bg-purple-700 text-white px-6 py-2 rounded-lg text-sm"
            >
              重新加载
            </button>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
