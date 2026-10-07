# T11 JOSE 依赖核查

2026-10-08实际访问crates.io jsonwebtoken/OpenSSL元数据：当前稳定jsonwebtoken11.1.0（MIT，MSRV1.88，notyanked）、openssl0.10.81（Apache-2.0，MSRV1.80，notyanked）。二者已在原锁文件，新增OpenSSL直接依赖仅用于导出公开JWK，签名由既有jsonwebtoken/aws-lc完成。

下载源码Cargo.toml与元数据一致；jsonwebtoken manifest SHA256 `9dc09ddaf9c1ba49e1686f68317a75fb1f316d5954e80f476a492607de7304a0`。仓库 https://github.com/Keats/jsonwebtoken 实际未归档/停用，pushed_at2026-10-04T13:33:29Z；OpenSSL manifest SHA256 `f78aa73885313fec152f91a20a2c96c6380d61f1bb81ff316c406b5e2edfbf47`，https://github.com/rust-openssl/rust-openssl 实际未归档/停用、pushed_at2026-09-22T11:01:14Z。最终锁文件RustSec由根证据填写。

核心JOSE单元测试2项真实退出0；core/store Clippy lib/bins最终退出0。测试RSA私钥仅临时内存生成，无token/私钥写证据。此记录不宣称完成OpenID conformance、生产密钥轮换或未执行的完整依赖审计。
