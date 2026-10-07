import { lazy, Suspense } from 'react';
import { Route, Routes } from 'react-router';
import { Layout } from './components/Layout';
import { AuthGuard } from './components/AuthGuard';
import { Status } from './components/Status';

const InitializationPage = lazy(() => import('./pages/InitializationPage'));
const LoginPage = lazy(() => import('./pages/LoginPage'));
const RegisterPage = lazy(() => import('./pages/RegisterPage'));
const EmailVerificationPage = lazy(() => import('./pages/EmailVerificationPage'));
const AccountPage = lazy(() => import('./pages/AccountPage'));
const SessionsPage = lazy(() => import('./pages/SessionsPage'));
const PasswordResetPage = lazy(() => import('./pages/PasswordResetPage'));
const PasswordChangePage = lazy(() => import('./pages/PasswordChangePage'));
const MfaPage = lazy(() => import('./pages/MfaPage'));
const PasskeysPage = lazy(() => import('./pages/PasskeysPage'));
const ConsentPage = lazy(() => import('./pages/ConsentPage'));
const AdminPage = lazy(() => import('./pages/AdminPage'));
const NotFoundPage = lazy(() => import('./pages/NotFoundPage'));
const ComponentsPage = import.meta.env.DEV ? lazy(() => import('./pages/ComponentsPage')) : null;

export default function App() {
  return <Suspense fallback={<Status kind="loading" title="正在加载页面…" />}><Routes><Route element={<Layout />}><Route path="/" element={<InitializationPage />} /><Route path="/login" element={<LoginPage />} /><Route path="/register" element={<RegisterPage />} /><Route path="/email-verification" element={<EmailVerificationPage />} /><Route path="/password-reset" element={<PasswordResetPage />} /><Route path="/oauth/consent/:id" element={<ConsentPage />} /><Route element={<AuthGuard />}><Route path="/me" element={<AccountPage />} /><Route path="/me/sessions" element={<SessionsPage />} /><Route path="/me/password/change" element={<PasswordChangePage />} /><Route path="/me/mfa" element={<MfaPage />} /><Route path="/me/passkeys" element={<PasskeysPage />} /></Route><Route element={<AuthGuard admin />}><Route path="/admin" element={<AdminPage />} /></Route>{ComponentsPage && <Route path="/dev/components" element={<ComponentsPage />} />}<Route path="*" element={<NotFoundPage />} /></Route></Routes></Suspense>;
}
