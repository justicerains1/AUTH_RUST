import { useState } from 'react';
import { Button } from './Button';
import { Status } from './Status';

export function RecoveryCodes({ codes, onClose }: { codes: readonly string[]; onClose: () => void }) {
  const [copied, setCopied] = useState(false);
  const [downloaded, setDownloaded] = useState(false);
  const [failed, setFailed] = useState(false);
  async function copy() { try { await navigator.clipboard.writeText(codes.join('\n')); setCopied(true); setFailed(false); } catch { setFailed(true); } }
  function download() {
    const blob = new Blob(['统一身份中心一次性恢复码\n请保存在可信位置，每个恢复码只能使用一次。\n\n', codes.join('\n'), '\n'], { type: 'text/plain;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a'); link.href = url; link.download = 'identity-recovery-codes.txt'; link.click(); URL.revokeObjectURL(url); setDownloaded(true);
  }
  return <section className="panel" aria-label="一次性恢复码"><h2>保存你的恢复码</h2><p>这十个恢复码仅显示一次，每个只能使用一次。重新生成会使旧集合全部失效。请保存在可信密码管理器中；下载文件会明文保存到你的设备。</p><ol className="recovery-codes">{codes.map((code) => <li key={code}><code>{code}</code></li>)}</ol><div className="actions"><Button onClick={() => { void copy(); }}>复制恢复码</Button><Button onClick={download}>下载恢复码</Button><Button onClick={onClose}>已保存，关闭恢复码</Button></div>{copied && <Status kind="success" title="已复制到剪贴板" description="请在可信位置保存，并清理共享设备的剪贴板。" />}{downloaded && <Status kind="success" title="已请求下载恢复码" description="请确认文件已保存，并移出共享或不可信位置。" />}{failed && <Status kind="error" title="暂时无法复制" description="请手动选择或下载保存恢复码。" />}</section>;
}
