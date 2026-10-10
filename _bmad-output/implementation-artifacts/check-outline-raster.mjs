// SPDX-License-Identifier: AGPL-3.0-or-later
// Local oracle only: Sharp is supplied by the agent runtime, never by the product.
import { createRequire } from 'node:module';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
const require = createRequire(import.meta.url);
const sharp = require(process.argv[2]);
const cases = JSON.parse(await readFile('target/prototype-oracle/font-visual-input.json', 'utf8'));
await mkdir('target/prototype-oracle/rasters', { recursive: true });
let maximum = 0;
for (let index=0; index<cases.length; index++) {
  const item = cases[index];
  const numbers = item.expected.match(/[-+]?\d+(?:\.\d*)?(?:[eE][-+]?\d+)?/g)?.map(Number) ?? [];
  const xs=numbers.filter((_,i)=>i%2===0), ys=numbers.filter((_,i)=>i%2===1);
  const left=xs.length?Math.min(...xs)-20:0, bottom=ys.length?Math.min(...ys)-20:0;
  const width=xs.length?Math.max(...xs)-left+20:100, height=ys.length?Math.max(...ys)-bottom+20:100;
  const render = commands => sharp(Buffer.from(`<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="${left} ${bottom} ${width} ${height}"><path fill="black" fill-rule="nonzero" d="${commands}"/></svg>`));
  const expected = await render(item.expected).ensureAlpha().raw().toBuffer();
  const actual = await render(item.actual).ensureAlpha().raw().toBuffer();
  if (actual.length!==expected.length) throw Error('Raster dimensions differ');
  for(let i=0;i<actual.length;i++) maximum=Math.max(maximum,Math.abs(actual[i]-expected[i]));
  if(maximum>1) throw Error(`Raster differs for ${item.font} glyph ${item.glyph}: ${maximum}`);
  if(index%15===6) await render(item.actual).png().toFile(resolve(`target/prototype-oracle/rasters/${index}.png`));
}
const evidence={font_faces:20,raster_cases:cases.length,reference:'FontTools paths rasterized independently by librsvg through Sharp',image_size:128,maximum_channel_difference:maximum,allowed_channel_difference:1,all_equal_within_tolerance:true};
await writeFile('_bmad-output/implementation-artifacts/prototype-outline-raster.json',JSON.stringify(evidence,null,2)+'\n');
console.log(JSON.stringify(evidence));
