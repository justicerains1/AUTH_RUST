import { Link, NavLink, Outlet, useLocation } from 'react-router';
import { useEffect, useRef } from 'react';
import { zhCN } from '../lib/i18n';

export function Layout() {
  const location = useLocation();
  const main = useRef<HTMLElement>(null);
  useEffect(() => {
    main.current?.focus();
  }, [location.pathname]);
  return <>
    <a className="skip-link" href="#main-content">{zhCN.skipNavigation}</a>
    <header className="site-header"><div className="container header-inner"><Link className="brand" to="/"><span aria-hidden="true">◈</span>{zhCN.brand}</Link><nav aria-label="主导航" className="main-nav"><NavLink className="nav-product" to="/" end>产品</NavLink><NavLink to="/me">{zhCN.account}</NavLink><NavLink className="nav-login" to="/login">登录</NavLink></nav></div></header>
    <main ref={main} id="main-content" className="container page-main" tabIndex={-1}><Outlet /></main>
    <footer className="container site-footer"><span>{zhCN.brand}</span><span>让每次访问，都有清晰的归属。</span><nav aria-label="法律信息"><Link to="/privacy">隐私政策</Link></nav></footer>
  </>;
}
