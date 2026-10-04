import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './styles.css';
import './styles/story.css';
import './styles/metrics.css';
import './index.css';
import { applyTheme, readStoredTheme } from './theme';

applyTheme(readStoredTheme());

const rootElement = document.getElementById('root');
if (!rootElement) {
  throw new Error('Root element not found. Ensure index.html has a <div id="root"></div>.');
}

ReactDOM.createRoot(rootElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
