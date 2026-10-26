const {execFileSync}=require('child_process');
const g=(...a)=>execFileSync('git',a,{encoding:'utf8',cwd:process.argv[2]});
const log=g('log','--date-order','--branches','--remotes','--tags','HEAD','--format=%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%s%x1e','--');
const commits=log.split('\x1e').map(r=>r.replace(/^\s+/,'')).filter(Boolean).map(r=>{const f=r.split('\x1f');return {hash:f[0],parents:f[1]?f[1].split(' '):[],author:f[2],email:f[3],time:+f[4],subject:f[5]};});
const refsOut=g('for-each-ref','--format=%(refname)%00%(objectname)%00%(*objectname)%00%(upstream:short)%00%(HEAD)','refs/heads','refs/remotes','refs/tags');
const local=[],remote=[],tags=[];
for(const l of refsOut.split('\n').filter(Boolean)){const[ref,oid,peeled,up,head]=l.split('\0');
 if(ref.startsWith('refs/heads/'))local.push({name:ref.slice(11),hash:oid,upstream:up||null,current:head==='*',ahead:0,behind:0});
 else if(ref.startsWith('refs/remotes/')){const rest=ref.slice(13);if(rest.endsWith('/HEAD'))continue;const i=rest.indexOf('/');remote.push({remote:rest.slice(0,i),branch:rest.slice(i+1),name:rest,hash:oid});}
 else tags.push({name:ref.slice(10),hash:peeled||oid});}
const files={};
for(const c of commits){const base=c.parents[0]||'4b825dc642cb6eb9a060e54bf8d69288fbee4904';
 const ns=g('diff','--name-status',base,c.hash,'--').trim().split('\n').filter(Boolean).map(l=>{const[s,...p]=l.split('\t');return{status:s[0],path:p[p.length-1],orig:p.length>1?p[0]:null};});
 files[c.hash]=ns.map(f=>({...f,diff:g('diff',base,c.hash,'--',f.path)}));}
const head=g('rev-parse','HEAD').trim();
const data={name:'demo',path:'C:\\Users\\Ana\\proyectos\\demo',commits,refs:{remotes:['origin'],local,remote,tags,stashes:[],head,currentBranch:'develop'},files};
process.stdout.write('// Generado a partir del repositorio de demostración (scripts: ver README).\nwindow.MOCK_REPO = '+JSON.stringify(data,null,1)+';\n');
