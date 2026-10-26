const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('crates/devoption-ui/assets/project-secrets.js', 'utf8');
function fixture(action='save', revision='0') {
  const events={},handlers={},calls=[],queue=[],navigation=[],links=[];
  const name={value:'SERVICE_KEY'},value=action==='save'?{value:'disposable secret'}:null,fieldset={disabled:true};
  const status={textContent:'',appendChild:link=>links.push(link)};
  const form={dataset:{secretForm:action,revision},elements:{namedItem:key=>key==='name'?name:value},querySelector:()=>fieldset,reportValidity:()=>true,addEventListener:(name,fn)=>{handlers[name]=fn;}};
  if(action==='remove') form.dataset.name='SERVICE_KEY';
  const root={dataset:{base:'/projects/'+'01'.repeat(16)+'/environments/production'},querySelectorAll:selector=>selector==='[data-secret-form]'?[form]:selector.includes('password')?(value?[value]:[]):[]};
  const document={getElementById:id=>id==='environment-secrets'?root:status,addEventListener:(_,fn)=>fn(),createElement:()=>({addEventListener:(name,fn)=>{handlers['link-'+name]=fn;}})};
  const window={location:{reload:()=>navigation.push('reload')},addEventListener:(name,fn)=>{events[name]=fn;}};
  const fetch=async(url,options)=>{calls.push({url,...options});const r=await queue.shift();if(r instanceof Error)throw r;return r;};
  vm.runInNewContext(source,{document,window,fetch,TextEncoder});
  return {events,handlers,calls,queue,navigation,links,fieldset,value,name,status,submit:()=>handlers.submit({preventDefault(){}})};
}
const response=(status,csrf='ab'.repeat(32))=>({status,headers:{get:()=>csrf}});
test('saving authenticates immediately before mutation and clears inputs',async()=>{
 const p=fixture();p.queue.push(response(204),response(204));await p.submit();
 assert.equal(p.calls[0].url,'/auth/session');assert.equal(p.calls[1].url,'/projects/'+'01'.repeat(16)+'/environments/production/secrets/SERVICE_KEY');
 assert.deepEqual(JSON.parse(p.calls[1].body),{value:'disposable secret',revision:0});
 assert.equal(p.calls[1].headers['x-csrf-token'],'ab'.repeat(32));assert.equal(p.calls[1].redirect,'error');assert.equal(p.calls[1].cache,'no-store');
 assert.equal(p.value.value,'');assert.deepEqual(p.navigation,['reload']);
});
test('lost responses and rejections keep writes disabled until explicit reload',async()=>{
 for(const result of [response(400),response(409),response(413),response(503),new Error('connection lost after possible commit')]){
  const p=fixture();p.queue.push(response(204),result);await p.submit();await p.submit();
  assert.equal(p.calls.length,2);assert.equal(p.value.value,'');assert.equal(p.fieldset.disabled,true);assert.deepEqual(p.navigation,[]);
  assert.match(p.status.textContent,/Reload/);p.handlers['link-click']({preventDefault(){}});assert.deepEqual(p.navigation,['reload']);
 }
});
test('an expired or invalid session prevents sending any secret body',async()=>{
 for(const result of [response(401),response(204,'invalid'),new Error('session unavailable')]){
  const p=fixture();p.queue.push(result);await p.submit();assert.equal(p.calls.length,1);assert.equal(p.value.value,'');assert.equal(p.fieldset.disabled,true);
 }
});
test('pending duplicate submission and late responses after navigation cannot resubmit',async()=>{
 const p=fixture();let finish;p.queue.push(response(204),new Promise(resolve=>{finish=resolve;}));const pending=p.submit();await new Promise(resolve=>setImmediate(resolve));await p.submit();assert.equal(p.calls.length,2);
 p.events.pagehide();finish(response(204));await pending;assert.equal(p.value.value,'');assert.equal(p.fieldset.disabled,true);assert.deepEqual(p.navigation,[]);
 p.events.pageshow({persisted:true});assert.deepEqual(p.navigation,['reload']);
});
test('removal sends only the displayed revision and never a replacement value',async()=>{
 const p=fixture('remove','7');p.queue.push(response(204),response(204));await p.submit();assert.ok(p.calls[1].url.endsWith('/SERVICE_KEY/remove'));assert.equal(p.calls[1].body,'7');
});
test('UTF-8 byte limit is checked before sending a request',async()=>{
 const p=fixture();p.value.value='é'.repeat(4097);await p.submit();assert.equal(p.calls.length,0);assert.match(p.status.textContent,/8,192 UTF-8 bytes/);
});
