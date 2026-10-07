import { render, screen, cleanup, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { Button } from './Button';
import { Input } from './Input';
import { Password } from './Password';
import { ConfirmDialog } from './ConfirmDialog';
import { CursorPagination } from './CursorPagination';
import { Table } from './Table';

afterEach(cleanup);

describe('基础控件行为', () => {
  it('输入通过label定位，支持粘贴且错误有明确文本关联', async () => {
    const user = userEvent.setup();
    render(<Input label="验证码" autoComplete="one-time-code" error="请输入6位数字" />);
    const field = screen.getByLabelText<HTMLInputElement>('验证码');
    await user.click(field);
    await user.paste('123456');
    expect(field.value).toBe('123456');
    expect(field.getAttribute('aria-invalid')).toBe('true');
    const error = screen.getByRole('alert');
    expect(error.textContent).toContain('请输入6位数字');
    expect(field.getAttribute('aria-describedby')).toBe(error.id);
    expect(field.getAttribute('autocomplete')).toBe('one-time-code');
  });

  it('密码切换不会提交表单或丢失输入', async () => {
    const user = userEvent.setup();
    const submit = vi.fn((event: React.SyntheticEvent) => { event.preventDefault(); });
    render(<form onSubmit={submit}><Password label="密码" /></form>);
    const input = screen.getByLabelText<HTMLInputElement>('密码');
    await user.type(input, 'paste allowed password');
    await user.click(screen.getByRole('button', { name: '显示密码' }));
    expect(input.type).toBe('text');
    expect(input.value).toBe('paste allowed password');
    expect(screen.getByRole('button', { name: '隐藏密码' }).getAttribute('aria-pressed')).toBe('true');
    await user.click(screen.getByRole('button', { name: '隐藏密码' }));
    expect(input.type).toBe('password');
    expect(submit).not.toHaveBeenCalled();
  });

  it('加载按钮与禁用分页不能重复触发', async () => {
    const user = userEvent.setup();
    const next = vi.fn();
    render(<><Button loading onClick={next}>提交</Button><CursorPagination hasPrevious={false} hasNext loading onPrevious={next} onNext={next} /></>);
    await user.click(screen.getByRole('button', { name: '正在处理…' }));
    await user.click(screen.getByRole('button', { name: '下一页' }));
    expect(next).not.toHaveBeenCalled();
    expect(screen.getByRole('status').textContent).toBe('正在加载列表…');
  });

  it('对话框初始取消、焦点受限且Escape返回触发按钮', async () => {
    const user = userEvent.setup();
    const confirmed = vi.fn();
    render(<ConfirmDialog trigger={<Button>打开确认</Button>} title="确认示例？" description="仅测试控件" confirmLabel="确认" onConfirm={confirmed} />);
    const trigger = screen.getByRole('button', { name: '打开确认' });
    await user.click(trigger);
    const cancel = screen.getByRole('button', { name: '取消' });
    expect(document.activeElement).toBe(cancel);
    await user.tab({ shift: true });
    expect(document.activeElement).toBe(screen.getByRole('button', { name: '确认' }));
    await user.keyboard('{Escape}');
    await waitFor(() => { expect(screen.queryByRole('dialog')).toBeNull(); });
    expect(document.activeElement).toBe(trigger);
    expect(confirmed).not.toHaveBeenCalled();
  });

  it('对话框操作失败保留可读错误和重试机会', async () => {
    const user = userEvent.setup();
    render(<ConfirmDialog trigger={<Button>打开确认</Button>} title="确认示例？" description="仅测试控件" confirmLabel="确认" onConfirm={() => Promise.reject(new Error('internal details'))} />);
    await user.click(screen.getByRole('button', { name: '打开确认' }));
    await user.click(screen.getByRole('button', { name: '确认' }));
    expect((await screen.findByRole('alert')).textContent).toContain('操作暂未完成');
    expect(screen.getByRole('dialog').textContent).not.toContain('internal details');
  });

  it('表格明确区分加载、空与失败，不把失败显示为数据', () => {
    const columns = [{ key: 'name', title: '名称', render: (row: { id: string; name: string }) => row.name }];
    const { rerender } = render(<Table caption="记录" rows={[]} columns={columns} rowKey={(row) => row.id} loading />);
    expect(screen.getByRole('status').textContent).toContain('正在加载记录');
    rerender(<Table caption="记录" rows={[]} columns={columns} rowKey={(row) => row.id} />);
    expect(screen.getByText('暂无记录')).toBeTruthy();
    rerender(<Table caption="记录" rows={[]} columns={columns} rowKey={(row) => row.id} unavailable onRetry={() => undefined} />);
    expect(screen.getByRole('status').textContent).toContain('暂时无法加载记录');
    expect(screen.getByRole('button', { name: '重新加载' })).toBeTruthy();
    expect(screen.queryByRole('table')).toBeNull();
  });
});
