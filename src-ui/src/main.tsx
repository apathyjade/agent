import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import { ErrorBoundary } from './components/ErrorBoundary';

const logError = (msg: string) => {
  try {
    localStorage.setItem('agent_error', new Date().toISOString() + ': ' + msg);
  } catch {}
};

window.onerror = (msg, url, line, col, err) => {
  logError(`GLOBAL: ${msg} at ${url}:${line}:${col} ${err?.stack}`);
  return false;
};

window.onunhandledrejection = (e) => {
  logError(`PROMISE: ${e.reason?.stack || e.reason}`);
};

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>
);
