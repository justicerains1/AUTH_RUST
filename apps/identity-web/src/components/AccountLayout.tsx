import type { ReactNode } from 'react';
import { NavLink } from 'react-router';
import { PageTitle } from '../pages/PageTitle';

export function AccountLayout({ title, description, children }: { title: string; description: string; children: ReactNode }) {
  return <><PageTitle title={title} /><div className="account-title"><p className="eyebrow">YOUR ACCOUNT</p><h1>{title}</h1><p className="muted">{description}</p></div><div className="account-layout"><nav className="account-navigation" aria-label="账号设置"><NavLink to="/me" end>账号安全</NavLink><NavLink to="/me/sessions">设备会话</NavLink><NavLink to="/me/grants">授权应用</NavLink><NavLink to="/me/password/change">修改密码</NavLink><NavLink to="/me/mfa">双因素验证</NavLink><NavLink to="/me/passkeys">通行密钥</NavLink></nav><div className="account-content">{children}</div></div></>;
}
