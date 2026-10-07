import { Link } from 'react-router';
import { PageTitle } from './PageTitle';

export default function NotFoundPage() {
  return <><PageTitle title="页面不存在" /><h1>页面不存在</h1><p className="muted">你访问的地址暂时没有对应页面。</p><Link to="/">返回首页</Link></>;
}
