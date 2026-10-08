import { act } from 'react';
import { createRoot } from 'react-dom/client';
import type { Root } from 'react-dom/client';
import { MemoryRouter } from 'react-router';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';

// UI-only response fixtures. Real identity and SSO acceptance use the T13 browser harness.
describe('演示应用 B BFF 页面', () => {
  let container: HTMLDivElement;
  let root: Root;
  const fetchMock = vi.fn<typeof fetch>();
  const authenticated = () => Response.json({ user: { sub: 'ui-fixture-user' }, authenticated: true });

  beforeEach(() => {
    vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
    vi.stubGlobal('fetch', fetchMock);
    fetchMock.mockReset();
    container = document.createElement('div');
    document.body.append(container);
    root = createRoot(container);
  });
  afterEach(() => {
    act(() => { root.unmount(); });
    container.remove();
    vi.unstubAllGlobals();
  });
  async function navigate(path: string) {
    await act(async () => {
      root.render(<MemoryRouter initialEntries={[path]}><App /></MemoryRouter>);
      await Promise.resolve();
    });
    await vi.waitFor(() => { expect(container.querySelector('main h1')).not.toBeNull(); });
  }
  async function click(label: string) {
    const button = Array.from(container.querySelectorAll('button')).find((element) => element.textContent === label);
    expect(button).toBeDefined();
    await act(async () => { button?.click(); await Promise.resolve(); });
  }

  it('匿名首页保持唯一 main/h1，并通过真实 BFF 登录入口跳转', async () => {
    fetchMock.mockResolvedValueOnce(Response.json({}, { status: 401 }));
    await navigate('/');
    expect(container.querySelectorAll('main')).toHaveLength(1);
    expect(container.querySelectorAll('h1')).toHaveLength(1);
    expect(container.querySelector('h1')?.textContent).toBe('演示应用 B');
    expect(container.querySelector('a')?.getAttribute('href')).toBe('/bff/login');
    expect(container.textContent).not.toContain('已登录本应用');
    expect(fetchMock).toHaveBeenCalledWith('/bff/session', { credentials: 'same-origin', cache: 'no-store' });
  });
  it('身份状态 503 时不展示已登录内容，并允许重新检查', async () => {
    fetchMock.mockResolvedValueOnce(Response.json({}, { status: 503 }));
    await navigate('/');
    expect(container.textContent).toContain('身份状态暂不可用');
    expect(container.querySelector('a')).toBeNull();
    fetchMock.mockResolvedValueOnce(authenticated());
    await click('重新检查');
    expect(container.textContent).toContain('已登录本应用');
    expect(container.textContent).toContain('ui-fixture-user');
  });
  it('登录后明确区分两个退出动作，本应用退出先获取 CSRF 再 POST', async () => {
    fetchMock.mockResolvedValueOnce(authenticated());
    await navigate('/');
    expect(Array.from(container.querySelectorAll('button'), (button) => button.textContent)).toEqual(['退出本应用', '退出身份平台']);
    fetchMock.mockResolvedValueOnce(Response.json({ csrf_token: 'ui-only-csrf' }));
    fetchMock.mockResolvedValueOnce(Response.json({ status: 'logged_out', redirect_to: null }));
    await click('退出本应用');
    expect(fetchMock).toHaveBeenLastCalledWith('/bff/logout', {
      method: 'POST', credentials: 'same-origin', cache: 'no-store',
      headers: { 'X-CSRF-Token': 'ui-only-csrf' },
    });
    expect(container.textContent).toContain('已退出本应用；身份平台及其他应用的会话不受影响。');
    expect(container.textContent).not.toContain('ui-fixture-user');
    expect(container.querySelector('a')?.getAttribute('href')).toBe('/bff/login');
  });
  it('授权撤销失败仍确认本应用已退出，并显示准确结果', async () => {
    fetchMock.mockResolvedValueOnce(authenticated());
    await navigate('/');
    fetchMock.mockResolvedValueOnce(Response.json({ csrf_token: 'ui-only-csrf' }));
    fetchMock.mockResolvedValueOnce(Response.json({ status: 'logged_out', revocation_status: 'failed', redirect_to: null }));
    await click('退出本应用');
    expect(container.textContent).toContain('本应用已退出，平台授权撤销暂未完成。');
    expect(container.textContent).not.toContain('ui-fixture-user');
    expect(container.querySelector('a')?.getAttribute('href')).toBe('/bff/login');
  });
  it('平台退出失败明确提示，允许用户重试', async () => {
    fetchMock.mockResolvedValueOnce(authenticated());
    await navigate('/');
    fetchMock.mockResolvedValueOnce(Response.json({ csrf_token: 'ui-only-csrf' }));
    fetchMock.mockResolvedValueOnce(Response.json({}, { status: 503 }));
    await click('退出身份平台');
    expect(fetchMock.mock.calls.at(-1)?.[0]).toBe('/bff/identity-logout');
    expect(container.textContent).toContain('退出暂未完成，请稍后重试。');
    expect(container.querySelectorAll('button:disabled')).toHaveLength(0);
  });
  it('未知路径提供可访问的首页链接且不请求身份', async () => {
    await navigate('/missing/path');
    expect(container.querySelector('main h1')?.textContent).toBe('页面不存在');
    expect(container.querySelector('main a')?.getAttribute('href')).toBe('/');
    expect(container.querySelector('main a')?.textContent).toBe('返回首页');
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
