// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { pageScrollTop, visiblePage } from "../web/page-navigation.mjs";

const pages = [100, 200, 150].map(height => ({ page: { bounds: { height } } }));
assert.equal(pageScrollTop(pages, 1, 2, 20), 0);
assert.equal(pageScrollTop(pages, 3, 2, 20), 640);
assert.deepEqual(visiblePage(pages, { zoom: 2, gap: 20, scrollTop: 0, height: 100 }), { page: 1, totalPages: 3 });
assert.equal(visiblePage(pages, { zoom: 2, gap: 20, scrollTop: 210, height: 0 }).page, 1);
assert.equal(visiblePage(pages, { zoom: 2, gap: 20, scrollTop: 211, height: 0 }).page, 2);
assert.equal(visiblePage(pages, { zoom: 2, gap: 20, scrollTop: 700, height: 100 }).page, 3);
assert.equal(visiblePage(pages, { zoom: 2, gap: 20, scrollTop: 9999, height: 100 }).page, 3);
assert.deepEqual(visiblePage([], { zoom: 1, gap: 20, scrollTop: 0, height: 100 }), { page: 0, totalPages: 0 });
for (const invalid of [0, 4, 1.5, NaN]) assert.throws(() => pageScrollTop(pages, invalid, 1, 20));
assert.throws(() => visiblePage(pages, { zoom: 0, gap: 20, scrollTop: 0, height: 100 }));
console.log(JSON.stringify({ status: "passed", checks: 13 }));
