const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const { test } = require("node:test");
const { runInNewContext } = require("node:vm");
const script = readFileSync(join(__dirname,"../crates/devoption-ui/assets/repository-create.js"),"utf8");
const project = "01".repeat(16), repository = "02".repeat(16), destination = `/projects/${project}/code#repository-${repository}`;
const response = (status, location, token = "ab".repeat(32)) => ({status, headers: new Map([["location",location],["x-csrf-token",token]])});
const tick = () => new Promise(resolve=>setImmediate(resolve));
function page(id = project) {
  const nodes=new Map(), events=new Map(), windows=new Map(), queue=[], calls=[], navigation=[];
  for(const key of ["repository-create-form","repository-create-name","repository-create-submit","repository-create-status","repository-create-reload","repository-create-signin"]){const listeners=new Map(),attributes=new Map();nodes.set(key,{value:"",textContent:"",disabled:true,hidden:true,dataset:{project:id},addEventListener:(key,fn)=>listeners.set(key,fn),emit:(key,event={})=>listeners.get(key)?.(event),setAttribute:(key,value)=>attributes.set(key,value),getAttribute:key=>attributes.get(key),focus(){this.focused=true}})}
  runInNewContext(script,{document:{getElementById:id=>nodes.get(id),addEventListener:(key,fn)=>events.set(key,fn)},window:{addEventListener:(key,fn)=>windows.set(key,fn),history:{replaceState:(_state,_title,url)=>navigation.push(url)},location:{reload:()=>navigation.push("reload")}},fetch:async(url,options)=>{calls.push({url,...options});assert.equal(options.method,"POST");assert.equal(options.credentials,"same-origin");assert.equal(options.cache,"no-store");assert.equal(options.redirect,"error");assert.equal(options.referrerPolicy,"no-referrer");assert.ok(queue.length);const result=await queue.shift();if(result instanceof Error)throw result;return result;}});
  events.get("DOMContentLoaded")();
  return{nodes,queue,calls,navigation,session:detail=>events.get("devoption-session")?.({detail}),hide:()=>windows.get("pagehide")(),submit:value=>{nodes.get("repository-create-name").value=value;return nodes.get("repository-create-form").emit("submit",{preventDefault(){}})},status:()=>nodes.get("repository-create-status").textContent};
}
test("creation requires verified session, fresh CSRF and exact project location",async()=>{
 const p=page();await p.submit("sample");assert.equal(p.calls.length,0);p.session("signed-in");p.queue.push(response(204),response(204,destination));await p.submit("sample");assert.deepEqual(p.navigation,[destination,"reload"]);assert.equal(p.calls[1].url,`/projects/${project}/repositories/create`);assert.equal(p.calls[1].body,"sample");assert.equal(p.calls[1].headers["x-csrf-token"],"ab".repeat(32));
});
test("invalid portable names never reach the server",async()=>{
 const p=page();p.session("signed-in");for(const name of ["","../outside","/absolute","two/parts","two\\parts",".git"," name","é","x".repeat(65),"-x"]){await p.submit(name);assert.match(p.status(),/1–64/)}assert.equal(p.calls.length,0);
});
test("duplicate pending submissions are ignored and existing paths allow correction",async()=>{
 const p=page();p.session("signed-in");let finish;p.queue.push(response(204),new Promise(resolve=>finish=resolve));const pending=p.submit("existing");await tick();await p.submit("ignored");assert.equal(p.calls.length,2);assert.equal(p.nodes.get("repository-create-submit").disabled,true);finish(response(409));await pending;assert.match(p.status(),/already exists/);assert.equal(p.nodes.get("repository-create-reload").hidden,false);assert.equal(p.nodes.get("repository-create-submit").disabled,false);p.queue.push(response(204),response(204,destination));await p.submit("different");assert.deepEqual(p.navigation,[destination,"reload"]);
});
test("uncertain saves and untrusted success destinations block retries until a reload",async()=>{
 for(const result of [response(503),new Error("network"),response(204,"https://attacker.invalid"),response(204,`/projects/${"ff".repeat(16)}/code#repository-${repository}`),response(204)]){
  const p=page();p.session("signed-in");p.queue.push(response(204),result);await p.submit("attempt");assert.equal(p.navigation.length,0);assert.match(p.status(),/could not be confirmed/);assert.equal(p.nodes.get("repository-create-reload").hidden,false);assert.equal(p.nodes.get("repository-create-submit").disabled,true);const count=p.calls.length;await p.submit("again");assert.equal(p.calls.length,count);
 }
});
test("invalid or expired sessions never expose a working mutation form",async()=>{
 for(const session of [response(401),response(204,undefined,"invalid"),response(503)]){const p=page();p.session("signed-in");p.queue.push(session);await p.submit("sample");assert.equal(p.calls.length,1);assert.equal(p.nodes.get("repository-create-signin").hidden,false);assert.equal(p.nodes.get("repository-create-submit").disabled,true)}
 const p=page();p.session("signed-in");p.queue.push(response(204),response(401));await p.submit("sample");assert.match(p.status(),/session ended/);
});
test("pagehide and changed session discard late success and retain uncertainty on return",async()=>{
 for(const change of [p=>p.hide(),p=>p.session("signed-out")]){const p=page();p.session("signed-in");let finish;p.queue.push(response(204),new Promise(resolve=>finish=resolve));const pending=p.submit("sample");await tick();change(p);finish(response(204,destination));await pending;assert.equal(p.navigation.length,0);p.session("signed-in");assert.equal(p.nodes.get("repository-create-submit").disabled,true);assert.match(p.status(),/inspect saved state/)}
});
test("server validation permits editing, while malformed project data stays disabled",async()=>{
 const p=page();p.session("signed-in");p.queue.push(response(204),response(400));await p.submit("rejected");assert.match(p.status(),/name was rejected/);assert.equal(p.nodes.get("repository-create-submit").disabled,false);assert.equal(p.nodes.get("repository-create-name").value,"rejected");
 const malformed=page("../escape");malformed.session("signed-in");await malformed.submit("sample");assert.equal(malformed.calls.length,0);assert.equal(malformed.nodes.get("repository-create-submit").disabled,true);
});
