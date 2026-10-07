import { useEffect, useState } from 'react';

export default function TotpQrCode({ uri }: { uri: string }) {
  const [matrix, setMatrix] = useState<{ size: number; path: string } | null>(null);
  useEffect(() => {
    let disposed = false;
    void import('qrcode-generator').then(({ default: createQr }) => {
      const qr = createQr(0, 'M'); qr.addData(uri); qr.make();
      const size = qr.getModuleCount(); let path = '';
      for (let row = 0; row < size; row += 1) for (let column = 0; column < size; column += 1) if (qr.isDark(row, column)) path += `M${String(column + 4)},${String(row + 4)}h1v1h-1z`;
      if (!disposed) setMatrix({ size: size + 8, path });
    }).catch(() => { if (!disposed) setMatrix(null); });
    return () => { disposed = true; };
  }, [uri]);
  return matrix ? <svg role="img" aria-label="TOTP设置二维码" viewBox={`0 0 ${String(matrix.size)} ${String(matrix.size)}`} width="220" height="220"><rect width="100%" height="100%" fill="white" /><path d={matrix.path} fill="black" /></svg> : <p className="muted">二维码加载中；你也可以手动输入下方密钥。</p>;
}
