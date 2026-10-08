import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, cleanup } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { afterEach, describe, expect, it, vi } from 'vitest';
import App from './App';
import { api, ApiError } from './lib/api';
import type { Me } from './lib/api';

afterEach(() => { cleanup(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });
function route(path: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  return render(<QueryClientProvider client={client}><MemoryRouter initialEntries={[path]}><App /></MemoryRouter></QueryClientProvider>);
}

describe('可访问基础路由', () => {
  it('首页展示已确认品牌与真实账号入口', async () => {
    route('/');
    const title = await screen.findByRole('heading', { level: 1 });
    expect(title.textContent).toContain('一个身份');
    expect(screen.getByRole('link', { name: '登录账号' }).getAttribute('href')).toBe('/login');
    expect(screen.getByRole('main')).toBe(document.activeElement);
    expect(document.title).toBe('一个身份，安心连接 · 统一身份中心');
  });

  it('未知路径显示未找到边界且可返回首页', async () => {
    route('/unknown/path');
    expect((await screen.findByRole('heading', { level: 1 })).textContent).toBe('页面不存在');
    expect(screen.getByRole('link', { name: '返回首页' }).getAttribute('href')).toBe('/');
  });

  it('401转登录而不是呈现保护页面', async () => {
    vi.spyOn(api, 'request').mockRejectedValue(new ApiError(401, 'AUTH_SESSION_REQUIRED', '请登录'));
    route('/me');
    expect((await screen.findByRole('heading', { level: 1, name: '登录' })).textContent).toBe('登录');
    expect(screen.queryByRole('heading', { name: '账号安全' })).toBeNull();
  });

  it('503保持可恢复失败，不显示已认证页面', async () => {
    vi.spyOn(api, 'request').mockRejectedValue(new ApiError(503, 'DEPENDENCY_UNAVAILABLE', '暂不可用'));
    route('/admin');
    expect(await screen.findByText('暂时无法检查账号状态')).toBeTruthy();
    expect(screen.getByRole('button', { name: '重新加载' })).toBeTruthy();
    expect(screen.queryByRole('heading', { name: '管理后台' })).toBeNull();
  });
  it('普通账号无法因已登录而进入管理员页面', async () => {
    const now = new Date().toISOString();
    const me: Me = {
      user: { id: '65d69320-97e8-4de0-a062-4c0f948a1b80', sub: '65d69320-97e8-4de0-a062-4c0f948a1b80', email: 'fixture@example.test', email_verified: true, status: 'active', created_at: now },
      session: { id: '65d69320-97e8-4de0-a062-4c0f948a1b80', amr: ['pwd'], auth_time: now, strong_at: null, expires_at: new Date(Date.now() + 60_000).toISOString(), created_at: now, current: true },
      security: { totp_enabled: false, passkey_count: 0, recovery_codes_remaining: 0, is_admin: false, admin_binding_only: false },
    };
    vi.spyOn(api, 'request').mockResolvedValue(me);
    route('/admin');
    expect(await screen.findByText('无法访问管理后台')).toBeTruthy();
    expect(screen.queryByRole('heading', { name: '管理后台' })).toBeNull();
  });

});
