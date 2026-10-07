import { spawn } from 'node:child_process';

const INSTALL_HINTS = {
  cargo: '安装 rustup 和 rust-toolchain.toml 指定的 Rust，并确保 cargo 在 PATH 中。',
  docker: '安装并启动 Docker Desktop 或 Docker Engine（含 Compose v2），再重试。',
  openssl: '安装 OpenSSL 3，并确保 openssl 在 PATH 中。',
};

export class CommandError extends Error {
  constructor(message, exitCode = 1) {
    super(message);
    this.name = 'CommandError';
    this.exitCode = exitCode || 1;
  }
}

/** Arguments are always passed as an array; no shell or command concatenation. */
export function runProcess(program, args, { cwd = process.cwd(), env = process.env, capture = false, quiet = false } = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(program, args, { cwd, env, shell: false, windowsHide: true, stdio: capture || quiet ? ['ignore', 'pipe', 'pipe'] : 'inherit' });
    let stdout = '';
    let stderr = '';
    if (capture || quiet) {
      child.stdout.setEncoding('utf8');
      child.stderr.setEncoding('utf8');
      child.stdout.on('data', (data) => { stdout += data; });
      child.stderr.on('data', (data) => { stderr += data; });
    }
    child.on('error', (error) => {
      if (error.code === 'ENOENT') {
        reject(new CommandError(`缺少 ${program}。${INSTALL_HINTS[program] ?? '确认该工具已安装且路径正确。'}`));
      } else reject(new CommandError(`无法启动 ${program}（${error.code ?? 'unknown'}）。`));
    });
    child.on('close', (code, signal) => {
      if (code !== 0) {
        if (capture && !quiet) {
          if (stdout) process.stdout.write(stdout);
          if (stderr) process.stderr.write(stderr);
        }
        reject(new CommandError(`${program} ${signal ? `被信号 ${signal} 终止` : `退出码 ${code ?? 1}`}。`, code));
      } else resolve({ stdout, stderr });
    });
  });
}
