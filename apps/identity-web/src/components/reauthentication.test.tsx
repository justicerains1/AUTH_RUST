import { useState } from 'react';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { ReauthenticationDialog } from './ReauthenticationDialog';
import { api } from '../lib/api';

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
function Harness() { const [open, setOpen] = useState(false); return <><button onClick={() => { setOpen(true); }}>确认安全操作</button><ReauthenticationDialog open={open} onOpenChange={setOpen} requiredStrength="password" methods={['password']} onConfirmed={() => undefined} /></>; }
it('identity dialog initially focuses cancel and restores the original trigger without a POST', async () => {
  const request = vi.spyOn(api, 'request'); render(<Harness />); const user = userEvent.setup(); const trigger = screen.getByRole('button', { name: '确认安全操作' });
  await user.click(trigger); const cancel = await screen.findByRole('button', { name: '取消身份确认' }); expect(document.activeElement).toBe(cancel);
  await user.click(cancel); await waitFor(() => { expect(document.activeElement).toBe(trigger); }); expect(request).not.toHaveBeenCalled(); expect(screen.queryByRole('dialog')).toBeNull();
});
