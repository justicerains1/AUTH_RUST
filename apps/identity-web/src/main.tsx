import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClientProvider } from '@tanstack/react-query';
import { BrowserRouter } from 'react-router';
import { queryClient } from './lib/query';
import App from './App';
import './styles.css';

const root = document.getElementById('root');
if (root === null) throw new Error('找不到前端挂载节点。');
createRoot(root).render(<StrictMode><QueryClientProvider client={queryClient}><BrowserRouter><App /></BrowserRouter></QueryClientProvider></StrictMode>);
