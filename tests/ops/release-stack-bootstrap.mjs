// Genuine local first-admin factor binding/client creation through TLS. No bypass fixture.
import assert from 'node:assert/strict';
import { chown, chmod, readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fixture, nextTotp, request } from './release-stack-helper.mjs';

export async function bootstrap(directory) {
  const data=fixture(directory);const headers={};let csrf;
  async function api(path,body){const result=await request(data,data.issuer,path,{...(body===undefined?{}:{method:'POST',body,headers:{...headers,origin:data.issuer,'content-type':'application/json','x-csrf-token':csrf}}),...(body===undefined?{headers}: {})});return result;}
  const preauth=await api('/api/v1/auth/csrf');assert.equal(preauth.status,200);headers.cookie=preauth.headers['set-cookie'].map((value)=>value.split(';')[0]).join('; ');csrf=preauth.json().csrf_token;
  const logged=await api('/api/v1/auth/login/password',{email:data.adminEmail,password:data.password});assert.equal(logged.status,200);headers.cookie=logged.headers['set-cookie'].filter((value)=>!value.includes('Max-Age=0')).map((value)=>value.split(';')[0]).join('; ');csrf=logged.json().csrf_token;
  assert.equal((await api('/api/v1/me/reauth/password',{password:data.password})).status,200);const enrollment=await api('/api/v1/me/mfa/totp/enrollment',{});assert.equal(enrollment.status,200);const setup=enrollment.json();const used=new Set();const code=await nextTotp(setup.secret,used);assert.equal((await api('/api/v1/me/mfa/totp/enrollment/confirm',{challenge_id:setup.challenge_id,code})).status,200);await writeFile(resolve(directory,'admin-factor'),setup.secret,{mode:0o600});await writeFile(resolve(directory,'admin-used-step'),String(Math.floor(Date.now()/30000)),{mode:0o600});
  for(const name of['a','b']){const origin=data[name==='a'?'appA':'appB'];const created=await api('/api/v1/admin/clients',{name:'Release stack '+name,allowed_scopes:['openid','profile','email'],redirect_uris:[origin+'/bff/callback'],post_logout_redirect_uris:[origin+'/']});assert.equal(created.status,201);const value=created.json();const secret=resolve(directory,'secret/client-'+name);await writeFile(secret,value.client_secret,{mode:0o600});await chown(secret,10001,10001);await chmod(secret,0o600);const env=resolve(directory,'demo-'+name+'.env');const text=await readFile(env,'utf8');await writeFile(env,text.replace('BFF_CLIENT_ID=stack-'+name,'BFF_CLIENT_ID='+value.client.client_id),{mode:0o600});}
  assert.equal((await api('/api/v1/auth/logout',{})).status,204);
}
