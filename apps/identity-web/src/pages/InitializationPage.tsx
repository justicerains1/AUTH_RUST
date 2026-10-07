import { Link } from 'react-router';
import { PageTitle } from './PageTitle';

export default function InitializationPage() {
  return <><PageTitle title="T01 初始化" /><section className="foundation-hero"><div><p className="eyebrow">IDENTITY / FOUNDATION</p><h1>统一身份中心<br /><span>T01 初始化</span></h1><p className="lead">前端工程已初始化。</p><p className="muted">认证功能按后续任务实现。账号、登录和管理路由已建立基础框架。</p><Link className="button button--primary" to="/login">查看登录入口</Link></div><aside className="foundation-note"><p className="eyebrow">清楚的访问，可靠的归属</p><h2>从同一个入口开始</h2><p>身份、会话与应用授权由统一的安全边界管理。</p></aside></section></>;
}
