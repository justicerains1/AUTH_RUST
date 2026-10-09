import { Link } from 'react-router';
import { PageTitle } from './PageTitle';

const sections = [
  ['information', '我们处理的信息'],
  ['purpose', '信息的用途'],
  ['cookies', 'Cookie 与浏览器存储'],
  ['sharing', '应用授权与服务提供方'],
  ['retention', '安全与保存'],
  ['choices', '你的选择与权利'],
  ['contact', '联系与政策更新'],
] as const;

export default function PrivacyPage() {
  return <><PageTitle title="隐私政策" /><article className="privacy-page" aria-labelledby="privacy-title">
    <header className="privacy-header"><p className="eyebrow">CDNGOD / PRIVACY</p><h1 id="privacy-title">隐私政策</h1><p className="lead muted">了解我们如何处理账号信息，以及你如何管理自己的访问与授权。</p><p className="muted">运营主体：CDNGOD · 更新日期：<time dateTime="2026-10-09">2026年10月9日</time></p><p>本政策适用于 CDNGOD 提供的统一身份中心，包括账号注册、登录、身份验证和应用授权。接入应用自身收集或使用的信息，应同时查看该应用的隐私说明。</p></header>
    <nav className="privacy-contents panel" aria-label="隐私政策目录"><h2>阅读目录</h2><ol>{sections.map(([id, title]) => <li key={id}><a href={`#privacy-${id}`}>{title}</a></li>)}</ol></nav>
    <section id="privacy-information" className="privacy-section" aria-labelledby="privacy-information-title"><h2 id="privacy-information-title">1. 我们处理的信息</h2><ul>
      <li><strong>账号资料：</strong>你提供的邮箱地址、显示名称（如设置），以及邮箱验证状态、账号创建时间和账号状态。</li>
      <li><strong>身份验证资料：</strong>密码的不可逆哈希；启用 TOTP 时的加密验证器密钥；恢复码的摘要；注册 Passkey 时的公钥、凭证标识和名称。身份中心不接收设备的指纹或面容数据，也不保存 Passkey 私钥。</li>
      <li><strong>会话与授权记录：</strong>登录方式、登录及到期时间、浏览器提供的设备信息、活跃会话，以及你授权的应用和访问范围。</li>
      <li><strong>安全与运行记录：</strong>认证、撤销、管理操作和邮件投递的结果及时间、请求标识等。服务接收请求来源 IP，并将其用于访问控制、限流和安全分析；应用审计使用来源摘要，部署中的代理或基础设施也可能记录访问日志。</li>
      <li><strong>邮件处理资料：</strong>收件邮箱、验证或重置链接、安全通知及投递状态，用于完成你请求的账号操作。</li>
    </ul></section>
    <section id="privacy-purpose" className="privacy-section" aria-labelledby="privacy-purpose-title"><h2 id="privacy-purpose-title">2. 信息的用途</h2><p>我们使用上述信息创建和维护账号、验证身份、发送必要的验证与安全邮件、管理会话、执行你明确授权的应用连接，以及防止滥用、调查安全事件和保障服务运行。</p><p>授权应用可访问哪些账号信息，以授权页面展示的范围和你的选择为准。身份验证需要的资料不会作为应用资料交给接入应用。</p></section>
    <section id="privacy-cookies" className="privacy-section" aria-labelledby="privacy-cookies-title"><h2 id="privacy-cookies-title">3. Cookie 与浏览器存储</h2><p>身份中心和接入应用使用会话 Cookie 维持登录，使用临时预认证和防跨站请求伪造机制保护账号操作。不同应用的登录会话分别管理。禁用必要 Cookie 可能导致注册、登录或安全操作无法完成。</p><p>密码、一次性验证码和邮件链接凭据仅用于当前流程；恢复码和客户端秘密在生成成功后仅显示一次，请保存在可信位置。身份中心不会将这些秘密写入 localStorage 或 sessionStorage。复制到剪贴板或下载的文件由你管理，在共享设备上请及时清理。</p></section>
    <section id="privacy-sharing" className="privacy-section" aria-labelledby="privacy-sharing-title"><h2 id="privacy-sharing-title">4. 应用授权与服务提供方</h2><p>连接应用时，身份中心按你同意的范围提供账号标识及相应资料，例如显示名称或邮箱。你可以拒绝授权，也可以在账号中撤销已有授权；撤销不自动删除接入应用此前已合法接收的信息。</p><p>邮件、托管、存储和备份等服务提供方可能为完成对应服务而处理必要信息。具体提供方、存储地域及适用的跨境处理安排将在正式服务信息中说明。当前尚未公布这些部署信息。</p><p>如适用法律要求披露信息，CDNGOD 将按适用要求处理，并限制披露的范围。</p></section>
    <section id="privacy-retention" className="privacy-section" aria-labelledby="privacy-retention-title"><h2 id="privacy-retention-title">5. 安全与保存</h2><p>我们采用密码哈希、敏感资料加密、访问权限控制及安全审计等措施保护账号资料。邮件任务完成投递后会清除包含秘密的模板参数；过期、已消费或已撤销的凭据不能继续用于认证。</p><p>信息的保存取决于提供服务、安全审计、备份恢复及适用法律的需要。凭据到期或撤销不等同于所有相关记录立即删除，备份中的信息也可能按备份周期保留。具体生产日志、账号记录和备份保留期限尚未公布；公布后将更新本政策。</p></section>
    <section id="privacy-choices" className="privacy-section" aria-labelledby="privacy-choices-title"><h2 id="privacy-choices-title">6. 你的选择与权利</h2><p>你可以在<Link to="/me">账号安全中心</Link>查看账号资料、管理验证方式，查看并撤销<Link to="/me/sessions">设备会话</Link>，以及撤销<Link to="/me/grants">应用授权</Link>。移除验证方式时，需要完成必要的身份验证，并保留符合安全要求的账号保护方式。</p><p>如需更正资料、获取个人信息副本、注销账号或提出其他隐私请求，请通过 CDNGOD 公布的隐私联系渠道申请。当前页面尚未提供自助账号注销或完整资料导出功能；我们可能需要核验你的身份后处理请求。</p></section>
    <section id="privacy-contact" className="privacy-section" aria-labelledby="privacy-contact-title"><h2 id="privacy-contact-title">7. 联系与政策更新</h2><p>如有隐私问题或需要提交个人信息相关请求，请联系 CDNGOD：<a href="mailto:support@cdngod.com">support@cdngod.com</a>。请在邮件中说明请求内容；不要发送密码、一次性验证码、恢复码或私钥。</p><p>正式服务开放前，我们将补充必要的服务提供方、存储地域和保留期限说明。</p><p>政策更新后会在本页展示更新日期；涉及信息处理方式的重要变化时，我们会通过适当方式提示。你可以随时返回本页查阅最新说明。</p></section>
    <div className="actions privacy-actions"><Link className="button" to="/">返回首页</Link><Link className="button button--primary" to="/me">管理账号安全</Link></div>
  </article></>;
}
