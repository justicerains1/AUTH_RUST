// Reproduce with PLAYWRIGHT_MODULE pointing to a separately installed playwright/index.mjs.
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE);
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
const root=resolve(import.meta.dirname,'../../..');
const cases=JSON.parse(await readFile(resolve(root,'docs/evidence/T15/reference-cases.json'),'utf8'));
const browser=await chromium.launch({headless:true});
const report={date:new Date().toISOString(),browser:await browser.version(),cases:[],mockups:[]};
console.log('Browser',report.browser);
await Promise.all(cases.map(async(site)=>{
 const entry={...site,visits:[]};
 for(const [kind,url] of [['official',site.official_url],['website',site.site_url]]){
  const context=await browser.newContext({viewport:{width:1440,height:1000},deviceScaleFactor:1});
  const page=await context.newPage();
  const visit={kind,url,visitedAt:new Date().toISOString()};
  const requests=[];page.on('requestfinished',request=>requests.push(request.url()));
  try{
   const started=Date.now(); const response=await page.goto(url,{waitUntil:'domcontentloaded',timeout:35000});
   await page.waitForTimeout(3000);
   visit.status=response?.status();visit.finalUrl=page.url();visit.title=await page.title();visit.domContentAndWaitMs=Date.now()-started;
   visit.text=(await page.locator('body').innerText()).slice(0,9000);
   visit.images=await page.locator('img').count();visit.canvas=await page.locator('canvas').count();visit.video=await page.locator('video').count();
   visit.fontSamples=await page.locator('h1,h2,p').evaluateAll(nodes=>nodes.slice(0,8).map(node=>({tag:node.tagName,text:node.innerText.slice(0,100),font:globalThis.getComputedStyle(node).fontFamily,size:globalThis.getComputedStyle(node).fontSize,color:globalThis.getComputedStyle(node).color})));
   visit.finishedRequestCount=requests.length;
   for(const [device,width,height] of [['desktop',1440,1000],['mobile',390,844]]){
    await page.setViewportSize({width,height});await page.waitForTimeout(700);
    const output=`docs/design/reference-screenshots/${site.slug}-${kind}-${device}.png`;
    await page.screenshot({path:resolve(root,output),fullPage:false});
    visit[`${device}Screenshot`]=output;
   }
  }catch(error){visit.error=error.message.split('\n')[0];}
  entry.visits.push(visit);await context.close();console.log(site.slug,kind,visit.status??visit.error);
 }
 report.cases.push(entry);
}));
for(const name of ['home','login','account','states']){
 const context=await browser.newContext({viewport:{width:1440,height:1000},deviceScaleFactor:1});const page=await context.newPage();
 await page.goto(pathToFileURL(resolve(root,'docs/design/mockups',`${name}.html`)).href);await page.evaluate(()=>globalThis.document.fonts.ready);
 for(const [device,width,height] of [['desktop',1440,1000],['mobile',390,844]]){
  await page.setViewportSize({width,height});const path=`docs/design/mockups/${name}-${device}.png`;await page.screenshot({path:resolve(root,path),fullPage:true});
  report.mockups.push({name,device,width,height,path,overflow:await page.evaluate(()=>globalThis.document.documentElement.scrollWidth>globalThis.innerWidth)});
 }
 for(const width of [360,768]){await page.setViewportSize({width,height:1000});report.mockups.push({name,device:'extra-layout',width,overflow:await page.evaluate(()=>globalThis.document.documentElement.scrollWidth>globalThis.innerWidth)});}
 await context.close();
}
await writeFile(resolve(root,'docs/evidence/T15/browser-capture.json'),JSON.stringify(report,null,2)+'\n');
await browser.close();console.log('Captured',report.cases.length,'references and',report.mockups.length,'mockup checks');
