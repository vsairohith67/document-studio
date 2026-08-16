import React from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import { applyDesignTokens } from './designTokens';
import './styles.css';

applyDesignTokens();

createRoot(document.getElementById('root')!).render(
  <React.StrictMode><App /></React.StrictMode>
);
