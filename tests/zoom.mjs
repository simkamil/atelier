// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { clampZoom, fitZoom, isAltGraphEvent, wheelZoom, zoomScrollPosition } from "../web/zoom.mjs";

assert.equal(clampZoom(0.1), 0.1);
assert.equal(clampZoom(5), 5);
for (const value of [NaN, Infinity, 0.09, 5.01]) assert.throws(() => clampZoom(value), RangeError);
assert.equal(fitZoom("width", 600, 800, 300, 900), 0.5);
assert.equal(fitZoom("page", 600, 800, 900, 400), 0.5);
assert.equal(fitZoom("pages", 600, 800, 900, 1000, 16), 0.615);
assert.equal(fitZoom("width", 600, 800, 6000, 9000), 5, "fit modes respect the maximum zoom");

const anchor = zoomScrollPosition({ oldZoom: 1, newZoom: 2, scrollLeft: 100, scrollTop: 200,
  viewportLeft: 10, viewportTop: 20, clientX: 210, clientY: 320 });
assert.deepEqual(anchor, { left: 400, top: 700 }, "the point under the pointer remains fixed through zoom");
assert.equal(zoomScrollPosition({ oldZoom: 1, newZoom: 2, scrollLeft: 0, scrollTop: 126,
  viewportLeft: 0, viewportTop: 0, clientX: 0, clientY: 0, pageHeights: [100, 100], pageGap: 16 }).top,
236, "page-stack anchoring keeps the fixed gap while preserving the same point on page two");
assert.equal(isAltGraphEvent({ ctrlKey: true, altKey: true }), true);
assert.equal(isAltGraphEvent({ ctrlKey: true, altKey: false, getModifierState: key => key === "AltGraph" }), true);

let zoom = 1;
let prevented = 0;
const scrollport = {};
const event = { ctrlKey: true, altKey: false, deltaY: -100, clientX: 10, clientY: 20,
  preventDefault() { prevented += 1; } };
assert.equal(wheelZoom(event, { active: false, modalOpen: false, getZoom: () => zoom,
  setZoom: value => { zoom = value; }, scrollport }), false);
assert.equal(wheelZoom(event, { active: true, modalOpen: true, getZoom: () => zoom,
  setZoom: value => { zoom = value; }, scrollport }), false);
assert.equal(wheelZoom({ ...event, altKey: true }, { active: true, modalOpen: false,
  getZoom: () => zoom, setZoom: value => { zoom = value; }, scrollport }), false);
assert.equal(wheelZoom(event, { active: true, modalOpen: false, getZoom: () => zoom,
  setZoom: value => { zoom = value; }, scrollport }), true);
assert.ok(zoom > 1);
assert.equal(prevented, 1, "only active, non-modal Ctrl+wheel is intercepted");

console.log(JSON.stringify({ status: "passed", checks: [
  "10–500% bounds and invalid values",
  "width, page and two-page fit calculations",
  "pointer anchoring",
  "active-view, modal and AltGr wheel filtering",
] }));
