import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {test} from 'node:test';
import vm from 'node:vm';

// Pure load-generator logic only; these checks never claim backend identity or performance.
test('continuous scenario retains exact measurement bounds, rate and valid status metric dispatch',async()=>{
 let script=await readFile(new URL('./scenarios.js',import.meta.url),'utf8');script=script.replace(/^import .*;\n/gmu,'').replaceAll('export ','');
 const samples={}; class Metric {constructor(name){this.name=name;samples[name]=[]}add(value){samples[this.name].push(value)}}
 let now=0; const credentials={base:'http://127.0.0.1:5310',control:'http://127.0.0.1:5311',clients:Array.from({length:20},()=>({id:'fixture',secret:'fixture'})),sessions:Array.from({length:2048},()=>({cookie:'fixture'})),emails:['fixture@example.test']};
 const context=vm.createContext({__ENV:{APP_ENV:'test',T21_SCENARIO:'introspection',T21_CREDENTIALS:'fixture'},open:()=>JSON.stringify(credentials),Counter:Metric,Rate:Metric,Trend:Metric,execution:{scenario:{startTime:0},vu:{idInTest:1}},Date:{now:()=>now},http:{},JSON,Number});
 vm.runInContext(script,context);
 for(const[t,expected]of[[119999,false],[120000,true],[1019999,true],[1020000,false]]){now=t;assert.equal(vm.runInContext('measured()',context),expected);}
 vm.runInContext("record('introspection',{status:200},2,true,true)",context);assert.deepEqual(samples.introspection_duration,[2]);assert.equal(samples.measurement_requests.length,1);
 assert.equal(vm.runInContext('scenarios.introspection.duration',context),'17m');assert.equal(vm.runInContext('scenarios.introspection.rate',context),300);
 assert.equal(vm.runInContext("thresholds['measurement_requests{endpoint:introspection}'][0]",context),'count>=270000');
});
