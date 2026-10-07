import { Link } from 'react-router';
import { Empty } from '../components/Empty';
import { PageTitle } from './PageTitle';

export default function LoginPage() {
  return <><PageTitle title="登录" /><div className="foundation-grid"><div><p className="eyebrow">ACCOUNT ACCESS</p><h1>登录</h1><p className="lead">从统一入口，进入你的账号。</p></div><section className="panel form-panel"><h2>登录入口</h2><Empty title="认证页面待接入" description="邮箱、密码及 Passkey 登录将在认证接口完成后提供。" /><Link to="/">返回首页</Link></section></div></>;
}
