import { act } from 'react';
import { createRoot } from 'react-dom/client';
import type { Root } from 'react-dom/client';
import { MemoryRouter } from 'react-router';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';

describe('T01 初始化路由', () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
    container = document.createElement('div');
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    container.remove();
    vi.unstubAllGlobals();
  });

  async function navigate(path: string) {
    await act(() => {
      root.render(
        <MemoryRouter initialEntries={[path]}>
          <App />
        </MemoryRouter>,
      );
      return Promise.resolve();
    });
    await vi.waitFor(() => {
      expect(container.querySelector('main h1')).not.toBeNull();
    });
  }

  it('首页通过 main 和一级标题说明初始化状态', async () => {
    await navigate('/');

    expect(container.querySelectorAll('main')).toHaveLength(1);
    expect(container.querySelectorAll('h1')).toHaveLength(1);
    expect(container.querySelector('h1')?.textContent).toBe('演示应用 B · T01 初始化');
    expect(container.textContent).toContain('认证功能按后续任务实现');
    expect(container.querySelector('form')).toBeNull();
  });

  it('未知路径显示未找到页面并提供可访问的首页链接', async () => {
    await navigate('/missing/path');

    expect(container.querySelector('main h1')?.textContent).toBe('页面不存在');
    const homeLink = container.querySelector('main a');
    expect(homeLink?.getAttribute('href')).toBe('/');
    expect(homeLink?.textContent).toBe('返回初始化页');
  });
});
