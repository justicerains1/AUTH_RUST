import assert from 'node:assert/strict';
import { createHash, createHmac, randomBytes } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import https from 'node:https';
import { resolve } from 'node:path';

export const root = resolve(import.meta.dirname, '../..');
export const services = ['api','worker','demo-a-bff','demo-b-bff','edge'];
export const run = (program, args, options = {}) => execFileSync(program, args, { encoding: 'utf8', stdio: ['ignore','pipe','pipe'], ...options });
export const docker = (...args) => run('docker', args);
export const hash = (value) => createHash('sha256').update(value).digest('hex');
export function fixture(directory) { return JSON.parse(run(process.execPath, ['-e', 'process.stdout.write(require("node:fs").readFileSync(process.argv[1],"utf8"))', resolve(directory,'fixture.json')])); }
export function compose(fixture, args) { return ['compose','--project-name',fixture.project,'--env-file',resolve(fixture.directory,'release.env'),'-f',resolve(fixture.directory,'compose.yaml'),...args]; }
export async function request(fixture, origin, path, { method = 'GET', body, headers = {} } = {}) {
  const url = new URL(path, origin); const ca = await readFile(resolve(fixture.directory,'ca.pem'));
  return new Promise((done,reject) => { const request = https.request({ hostname:'127.0.0.1', port:fixture.port, servername:url.hostname, path:url.pathname+url.search, method, ca, rejectUnauthorized:true, headers:{host:url.host,...headers} }, (response) => { let text='';response.setEncoding('utf8');response.on('data',(value)=>{text+=value;});response.on('end',()=>done({status:response.statusCode,headers:response.headers,text,json:()=>JSON.parse(text)})); }); request.on('error',reject);if(body!==undefined)request.write(typeof body==='string'?body:JSON.stringify(body));request.end(); });
}
export function totp(secret, seconds) { let buffer=0,bits=0;const bytes=[];for(const ch of secret){const value='ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'.indexOf(ch);assert.ok(value>=0);buffer=(buffer<<5)|value;bits+=5;if(bits>=8){bits-=8;bytes.push((buffer>>bits)&255);buffer&=(1<<bits)-1;}}const counter=Buffer.alloc(8);counter.writeBigUInt64BE(BigInt(Math.floor(seconds/30)));const digest=createHmac('sha1',Buffer.from(bytes)).update(counter).digest();return String((digest.readUInt32BE(digest[19]&15)&0x7fffffff)%1000000).padStart(6,'0'); }
export async function nextTotp(secret, usedSteps) { while(usedSteps.has(Math.floor(Date.now()/30000)))await new Promise((done)=>setTimeout(done,100));const step=Math.floor(Date.now()/30000);usedSteps.add(step);return totp(secret, step*30); }
export async function mailToken(fixture,email) { for(let attempt=0;attempt<100;attempt++){const list=await fetch('http://127.0.0.1:'+fixture.mailPort+'/api/v1/messages');assert.equal(list.status,200);const data=await list.json();for(const mail of data.messages){if(!mail.To.some((value)=>value.Address===email))continue;const detail=await(await fetch('http://127.0.0.1:'+fixture.mailPort+'/api/v1/message/'+mail.ID)).json();const token=/#token=([A-Za-z0-9_-]{43})/u.exec(detail.Text)?.[1];if(token)return token;}await new Promise((done)=>setTimeout(done,100));}throw new Error('Actual owned SMTP verification mail not delivered.'); }
export function privatePhrase() { return 'Release stack '+randomBytes(32).toString('base64url'); }
