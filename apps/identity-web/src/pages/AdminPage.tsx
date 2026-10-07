import { Empty } from '../components/Empty';
import { PageTitle } from './PageTitle';

export default function AdminPage() {
  return <><PageTitle title="管理后台" /><p className="eyebrow">PLATFORM / ADMINISTRATION</p><h1>管理后台</h1><section className="panel"><Empty title="后台基础路由已就绪" description="用户、客户端及管理员管理将在对应任务完成后提供。" /></section></>;
}
