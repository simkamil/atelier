// SPDX-License-Identifier: AGPL-3.0-or-later
import { CanvasRenderer } from "../web/canvas-renderer.mjs";

function fixture(width,height,commands,minX=0,minY=0){
  const b=[];const u8=v=>b.push(v&255),u32=v=>{for(let i=0;i<4;i++)u8(v>>>8*i);},u64=v=>{let n=BigInt(v);for(let i=0;i<8;i++){u8(Number(n&255n));n>>=8n;}},f64=v=>{const a=new ArrayBuffer(8);new DataView(a).setFloat64(0,v,true);b.push(...new Uint8Array(a));},f32=v=>{const a=new ArrayBuffer(4);new DataView(a).setFloat32(0,v,true);b.push(...new Uint8Array(a));};
  const rect=(x,y,w,h)=>{f64(x);f64(y);f64(x+w);f64(y+h);},color=(r,g,bl,a=1)=>{f32(r);f32(g);f32(bl);f32(a);};
  b.push(65,84,68,76);u8(1);u8(0);u8(0);u8(0);rect(minX,minY,width,height);u32(commands.length);
  for(const c of commands){
    if(c.type==="fill"){u8(1);u8(c.rule||0);color(...c.color);u32(c.points.length+1);u8(0);f64(c.points[0][0]);f64(c.points[0][1]);for(const p of c.points.slice(1)){u8(1);f64(p[0]);f64(p[1]);}u8(4);}
    else if(c.type==="stroke"){u8(2);color(...c.color);f64(c.width);u8(c.cap||0);u8(c.join||0);f64(c.miter||1);u32(c.points.length+1);u8(0);f64(c.points[0][0]);f64(c.points[0][1]);for(const p of c.points.slice(1)){u8(1);f64(p[0]);f64(p[1]);}u8(4);}
    else if(c.type==="clip"){u8(5);rect(...c.rect);}
    else if(c.type==="popClip")u8(6);
    else if(c.type==="transform"){u8(7);for(const v of c.matrix)f64(v);}
    else if(c.type==="popTransform")u8(8);
    else if(c.type==="image"){u8(4);u64(c.id);rect(...c.rect);}
    else if(c.type==="glyph"){u8(3);u64(c.font);color(...c.color);u32(1);u32(c.gid);f64(c.x);f64(c.y);f64(0);f64(0);}
  }
  return Uint8Array.from(b);
}
const check=(ok,name)=>{if(!ok)throw new Error(name);return name;};
const pixel=(ctx,x,y)=>Array.from(ctx.getImageData(x,y,1,1).data);

export async function runCanvasChecks(){
  const checks=[];
  const canvas=document.createElement("canvas");canvas.style.width="120px";canvas.style.height="100px";document.body.append(canvas);
  for (const pageGap of [Number.NaN, Number.POSITIVE_INFINITY, -1]) {
    let rejected = false;
    try { new CanvasRenderer(canvas, { pageGap }); } catch (error) { rejected = error instanceof RangeError; }
    checks.push(check(rejected, `invalid pageGap ${String(pageGap)} is rejected`));
  }
  for (const options of [
    { resolveGlyph: null },
    { resolveImage: false },
    { onDiagnostic: 1 },
  ]) {
    let rejected = false;
    try { new CanvasRenderer(canvas, options); } catch (error) { rejected = error instanceof TypeError; }
    checks.push(check(rejected, "invalid renderer callback is rejected"));
  }
  const glyphPath=new Path2D();glyphPath.rect(-3,-8,12,10);
  let glyphLoads=0;
  const image=document.createElement("canvas");image.width=8;image.height=8;const ic=image.getContext("2d");ic.fillStyle="#00ff00";ic.fillRect(0,0,8,8);
  const renderer=new CanvasRenderer(canvas,{resolveGlyph:(font,gid)=>{glyphLoads++;return font===7n&&gid===12?{path:glyphPath,bounds:{x:-3,y:-8,width:12,height:10}}:null;},resolveImage:id=>id===9n?image:null,maxGlyphCache:4,pageGap:4});
  const filled={type:"fill",color:[1,0,0,1],points:[[5,5],[35,5],[35,35],[5,35]]};
  const lineElement={type:"lineTo",point:{x:1,y:1}};
  const millionSegmentPath=new Array(1_000_000);
  millionSegmentPath[0]={type:"moveTo",point:{x:0,y:0}};
  millionSegmentPath.fill(lineElement,1);
  const pathBounds=renderer._bounds({type:"fillPath",path:millionSegmentPath},{a:1,b:0,c:0,d:1,e:0,f:0});
  checks.push(check(pathBounds.x===0&&pathBounds.y===0&&pathBounds.width===1&&pathBounds.height===1,
    "bounds handle one million valid path segments"));
  renderer.setViewport({width:120,height:100,zoom:1,dpr:1});renderer.setPages([fixture(120,100,[filled])]);
  checks.push(check(pixel(renderer.ctx,15,15)[0]>240,"Path2D filled into real canvas"));
  renderer.setPages([fixture(120,100,[{type:"stroke",color:[0,0,0,1],width:4,join:0,miter:8,points:[[15,45],[35,45],[15,65]]}])]);
  checks.push(check(pixel(renderer.ctx,20,45)[3]>0,"Path2D stroke with miter join"));
  const nonCommutative=fixture(120,100,[{type:"transform",matrix:[2,0,0,2,0,0]},{type:"transform",matrix:[1,0,0,1,10,0]},filled,{type:"popTransform"},{type:"popTransform"}]);
  renderer.setPages([nonCommutative]);
  checks.push(check(pixel(renderer.ctx,25,15)[0]>240&&pixel(renderer.ctx,15,15)[3]===0,"non-commutative current then new transform composition"));
  const clipped=fixture(120,100,[{type:"clip",rect:[20,20,20,20]},{type:"transform",matrix:[1,0,0,1,10,0]},filled,{type:"popTransform"},{type:"popClip"}]);
  renderer.setPages([clipped]);
  checks.push(check(pixel(renderer.ctx,35,30)[0]>240&&pixel(renderer.ctx,55,30)[3]===0,"page-coordinate clip and current*new affine transform"));
  renderer.setPages([fixture(120,100,[{type:"image",id:9n,rect:[60,10,8,8]},{type:"glyph",font:7n,gid:12,color:[0,0,1,1],x:85,y:30}])]);
  checks.push(check(pixel(renderer.ctx,63,13)[1]>240&&pixel(renderer.ctx,84,24)[2]>240,"CanvasImageSource and injected glyph Path2D"));
  renderer.setPages([fixture(120,100,[{...filled,color:[1,0,0,1]}])]);
  const moved={type:"fill",color:[0,0,1,1],points:[[70,5],[100,5],[100,35],[70,35]]};
  renderer.pages=[(await import("../web/display-list.mjs")).decodeDisplayList(fixture(120,100,[moved]))];
  renderer.render({x:0,y:0,width:45,height:45});
  checks.push(check(pixel(renderer.ctx,15,15)[3]===0&&pixel(renderer.ctx,80,15)[3]===0,"dirty clear removes old pixels and limits repaint to dirty bounds"));
  renderer.setPages([fixture(120,100,[{type:"glyph",font:7n,gid:12,color:[0,0,1,1],x:85,y:30},{type:"glyph",font:7n,gid:12,color:[0,0,1,1],x:100,y:30}])]);
  checks.push(check(renderer.stats.glyphCacheMisses===0&&renderer.stats.glyphCacheHits===2,"bounded glyph Path2D cache reuses outlines"));
  const beforeInvalidation=glyphLoads;renderer.invalidateResources();
  checks.push(check(glyphLoads===beforeInvalidation+1,"resource invalidation reloads glyph paths"));
  renderer.setViewport({width:120,height:100,zoom:1,dpr:2});
  renderer.setPages([fixture(120,100,[{type:"glyph",font:7n,gid:12,color:[0,0,1,1],x:25,y:40}],10,20)]);
  checks.push(check(pixel(renderer.ctx,40,40)[2]>200,"glyph origin, page offset, and DPR transform order"));
  renderer.setOverlay([{x:5,y:50,width:40,height:2}]);checks.push(check(pixel(renderer.ctx,20,104)[2]>100,"viewport-local underline overlay"));
  renderer.clearOverlay();checks.push(check(pixel(renderer.ctx,20,104)[3]===0,"overlay can be erased"));
  renderer.setViewport({width:120,height:100,zoom:1,dpr:1.5});checks.push(check(canvas.width===180&&canvas.height===150,"DPR 1.5 backing dimensions"));
  for(const dpr of [1,2]){renderer.setViewport({width:120,height:100,zoom:1,dpr});checks.push(check(canvas.width===120*dpr&&canvas.height===100*dpr,`DPR ${dpr} backing dimensions`));}
  const pages=Array.from({length:300},()=>fixture(120,50,[filled]));
  renderer.setViewport({width:120,height:80,zoom:1,scrollY:0,dpr:1});renderer.setPages(pages);
  const durations=[];
  const measuredPageCounts=[];
  let visibleOnEveryFrame=true;
  let pagesDrawnAtEnd=0;
  for(let frame=0;frame<130;frame++){
    const started=performance.now();
    renderer.setViewport({scrollY:(frame%300)*54});
    const duration=performance.now()-started;
    pagesDrawnAtEnd=renderer.stats.pagesDrawn;
    visibleOnEveryFrame&&=pagesDrawnAtEnd>0&&pagesDrawnAtEnd<=2;
    if(frame>=10){durations.push(duration);measuredPageCounts.push(pagesDrawnAtEnd);}
  }
  const sorted=[...durations].sort((a,b)=>a-b);
  const percentile=p=>sorted[Math.min(sorted.length-1,Math.ceil(p*sorted.length)-1)];
  const timing={medianMs:percentile(.5),p95Ms:percentile(.95),maxMs:Math.max(...durations)};
  checks.push(check(visibleOnEveryFrame,"300-page scroll replays visible pages only"));
  renderer.setViewport({width:0,height:0,zoom:1,dpr:1});checks.push(check(renderer.stats.pagesDrawn===0,"empty viewport draws no pages"));
  checks.push({name:"benchmark 300-page scroll",passed:true,informational:true,pages:300,frames:durations.length,...timing,
    pagesDrawnPerFrame:measuredPageCounts,environment:"headless software Canvas",userAgent:navigator.userAgent});
  renderer.destroy();canvas.remove();return checks.map(value=>typeof value==="string"?{name:value,passed:true}:value);
}
