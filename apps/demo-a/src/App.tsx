import { lazy, Suspense } from 'react';
import { Route, Routes } from 'react-router';

const InitializationPage = lazy(() => import('./pages/InitializationPage'));
const NotFoundPage = lazy(() => import('./pages/NotFoundPage'));

export default function App() {
  return (
    <Suspense fallback={<main aria-busy="true">正在加载页面…</main>}>
      <Routes>
        <Route path="/" element={<InitializationPage />} />
        <Route path="*" element={<NotFoundPage />} />
      </Routes>
    </Suspense>
  );
}
