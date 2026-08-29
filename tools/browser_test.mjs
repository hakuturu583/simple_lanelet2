// Drives the built viewer in a real browser.
//
// `cargo test` covers the Rust and `tools/smoke_web.mjs` covers the module's
// exports, but neither can tell you whether the thing works as a *component*:
// whether a canvas inside a shadow root receives a wheel event, whether the
// element upgrades, whether a resized container reaches the ResizeObserver,
// whether `destroy()` leaves anything behind, whether the postMessage protocol
// round-trips. Those are the promises EMBEDDING.md makes to a host, so they are
// the ones worth a browser.
//
//   node tools/browser_test.mjs
//
// Needs Playwright and a Chromium; both are on the GitHub runner image. If
// Playwright is not installed the script says so and exits 0, so a contributor
// without it is not blocked — CI installs it and does run these.

import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, join, normalize } from 'node:path';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const web = join(root, 'web');

let chromium;
try {
  ({ chromium } = await import('playwright'));
} catch {
  console.log('Playwright is not installed — skipping the browser tests.');
  console.log('  npm i -D playwright && npx playwright install chromium');
  process.exit(0);
}

// --- a static server, so the test needs nothing running beside it ------------

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.wasm': 'application/wasm',
  '.osm': 'application/xml; charset=utf-8',
  '.map': 'application/json',
};

const server = createServer(async (request, response) => {
  const path = normalize(decodeURIComponent(new URL(request.url, 'http://x').pathname));
  // `normalize` has already collapsed `..`; refusing what is left outside `web`
  // is what stops a crafted path from reading the repository.
  const file = join(web, path === '/' ? '/index.html' : path);
  if (!file.startsWith(web)) {
    response.writeHead(403).end();
    return;
  }
  try {
    await stat(file);
    const extension = file.slice(file.lastIndexOf('.'));
    response.writeHead(200, { 'content-type': TYPES[extension] ?? 'application/octet-stream' });
    response.end(await readFile(file));
  } catch {
    response.writeHead(404).end('not found');
  }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const BASE = `http://127.0.0.1:${server.address().port}`;

// --- the tests ---------------------------------------------------------------

let failures = 0;
const check = (ok, what) => {
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${what}`);
  if (!ok) failures += 1;
};

const browser = await chromium.launch({
  // The CI image and the local install put it in different places; letting
  // Playwright decide unless told otherwise covers both.
  executablePath: process.env.CHROMIUM_PATH || undefined,
});

const tooltipText = (page, frame = '') =>
  page.evaluate(
    (selector) => {
      const doc = selector ? document.querySelector(selector).contentDocument : document;
      const tip = doc.querySelector('#viewer > div').shadowRoot.querySelector('.tooltip');
      return tip.hidden ? null : tip.textContent;
    },
    frame,
  );

try {
  // ------------------------------------------------------------- the demo page
  console.log('demo page');
  {
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
    const problems = [];
    page.on('pageerror', (error) => problems.push(error.message));
    page.on('console', (m) => m.type() === 'error' && problems.push(m.text()));

    await page.goto(`${BASE}/index.html`);
    await page.click('#sample-button');
    await page.waitForSelector('#layers-block:not([hidden])', { timeout: 60000 });
    await page.waitForFunction(() => document.getElementById('status').hidden, { timeout: 60000 });

    check((await page.$eval('#facts', (n) => n.innerText)).includes('371'), 'the example map loads');
    check((await page.$eval('#legend', (n) => n.children.length)) > 5, 'the legend is populated');
    const isolation = await page.evaluate(() => ({
      shadow: Boolean(document.querySelector('#viewer > div')?.shadowRoot),
      strayCanvases: document.querySelectorAll('canvas').length,
    }));
    check(isolation.shadow, 'the viewer mounts a shadow root');
    check(isolation.strayCanvases === 0, 'nothing of the viewer leaks into the page document');

    const box = await page.$eval('#stage', (n) => {
      const r = n.getBoundingClientRect();
      return { x: r.x, y: r.y, w: r.width, h: r.height };
    });
    let tooltip = null;
    sweep: for (let px = 0.05; px < 0.98; px += 0.02) {
      for (let py = 0.05; py < 0.98; py += 0.03) {
        await page.mouse.move(box.x + box.w * px, box.y + box.h * py);
        tooltip = await tooltipText(page);
        if (tooltip) break sweep;
      }
    }
    check(Boolean(tooltip), `hover picks a primitive (${tooltip})`);

    // A wheel event has to reach a canvas inside a shadow root, which is not
    // something to assume. The scale bar is the visible consequence.
    const scaleLabel = () =>
      page.evaluate(
        () => document.querySelector('#viewer > div').shadowRoot.querySelector('.scalebar .label').textContent,
      );
    const before = await scaleLabel();
    await page.mouse.move(box.x + box.w * 0.3, box.y + box.h * 0.6);
    for (let i = 0; i < 10; i += 1) {
      await page.mouse.wheel(0, -120);
      await page.waitForTimeout(20);
    }
    await page.waitForTimeout(200);
    check(before !== (await scaleLabel()), 'wheel zoom reaches the shadow canvas');

    await page.selectOption('#theme-select', 'light');
    await page.waitForTimeout(400);
    check((await page.$eval('body', (n) => n.dataset.theme)) === 'light', 'the theme control works');
    await page.selectOption('#theme-select', 'dark');

    // The 3D view rebuilds the scene in Rust and hands back projected vertices,
    // so the evidence that it worked is that the geometry moved and the sliders
    // that steer it appeared.
    const scaleBefore = await scaleLabel();
    await page.check('#three-d-toggle');
    await page.waitForTimeout(600);
    check(
      await page.$eval('#three-d-controls', (n) => !n.hidden),
      'the 3D toggle reveals the camera controls',
    );
    check(
      (await page.$eval('#relief-label', (n) => n.textContent)).includes('relief'),
      "the panel states the map's relief",
    );
    // Tilting refits the map, so the zoom the wheel left behind is gone.
    check(scaleBefore !== (await scaleLabel()), 'turning on 3D reframes the map');
    await page.fill('#pitch-input', '25');
    await page.dispatchEvent('#pitch-input', 'input');
    await page.waitForTimeout(400);
    await page.uncheck('#three-d-toggle');
    await page.waitForTimeout(400);
    check(
      await page.$eval('#three-d-controls', (n) => n.hidden),
      'and turning it off hides them again',
    );

    const download = page.waitForEvent('download', { timeout: 30000 });
    await page.click('#export-button');
    check((await download).suggestedFilename() === 'mapping_example.svg', 'SVG export downloads');

    await page.evaluate(async () => {
      const text = await (await fetch('./sample/mapping_example.osm')).text();
      const transfer = new DataTransfer();
      transfer.items.add(new File([text], 'dropped.osm'));
      window.dispatchEvent(new DragEvent('drop', { dataTransfer: transfer, bubbles: true }));
    });
    await page.waitForTimeout(2500);
    check(await page.$eval('#status', (n) => n.hidden), 'drag and drop loads a map');
    check(problems.length === 0, `no page errors (${problems.join('; ')})`);
    await page.close();
  }

  // ------------------------------------------------- the iframe and the element
  console.log('embedding');
  {
    const page = await browser.newPage({ viewport: { width: 1200, height: 1400 } });
    const problems = [];
    page.on('pageerror', (error) => problems.push(error.message));

    await page.goto(`${BASE}/embed-example.html`);
    const logged = (text) =>
      page.waitForFunction((t) => document.getElementById('log').textContent.includes(t), text, {
        timeout: 60000,
      });

    await logged('ready:');
    check(true, 'the framed viewer announces itself with lanelet2.ready');

    await page.click('#send-map');
    await logged('loaded:');
    check(
      /loaded: 371 lanelets/.test(await page.$eval('#log', (n) => n.textContent)),
      'postMessage load round-trips with statistics',
    );

    await page.click('#light');
    await page.waitForTimeout(500);
    await page.click('#roads-only');
    await page.waitForTimeout(300);
    await page.click('#all-layers');
    await page.click('#dark');
    await page.waitForTimeout(400);
    await page.click('#export');
    await logged('svg:');
    check(
      /svg: [\d,]+ bytes/.test(await page.$eval('#log', (n) => n.textContent)),
      'exportSvg replies with a document',
    );

    // Round-trips `lanelet2.setView3d` and the `lanelet2.view3d` the frame answers
    // with — the half of the protocol a host needs to keep a control of its own in
    // step with the viewer's.
    await page.click('#toggle-3d');
    await logged('view3d: on');
    check(true, 'setView3d round-trips through the iframe protocol');
    await page.click('#toggle-3d');
    await logged('view3d: off');

    check(
      /element: 371 lanelets/.test(await page.$eval('#log', (n) => n.textContent)),
      '<lanelet2-viewer src=…> loads on its own',
    );
    check(await page.$eval('#element', (n) => Boolean(n.viewer)), 'the element exposes its viewer');
    // A theme change re-colours the scene without rebuilding it, and the guard
    // that makes that safe has to actually hold on a real map.
    const themed = await page.$eval('#element', async (element) => {
      const viewer = element.viewer;
      const before = {
        // Not `backgroundColor`: this element asks for a transparent one, which
        // is the same under either theme. The road's own colour is the signal.
        roadFill: viewer._geometry.styles.map((style) => style.fill).join(),
        vertices: viewer._geometry.coords,
        styles: viewer._geometry.styles.length,
        legend: viewer.legend.length,
      };
      viewer.setTheme('light');
      await new Promise((resolve) => setTimeout(resolve, 300));
      return {
        changed: viewer._geometry.styles.map((style) => style.fill).join() !== before.roadFill,
        sameBuffer: viewer._geometry.coords === before.vertices,
        sameStyles: viewer._geometry.styles.length === before.styles,
        sameLegend: viewer.legend.length === before.legend,
      };
    });
    check(themed.changed, 'a theme change re-colours the map');
    check(themed.sameBuffer, 'and does so without re-flattening the geometry');
    check(themed.sameStyles && themed.sameLegend, 'and keeps the style table aligned');
    await page.$eval('#element', (element) => element.viewer.setTheme('dark'));

    // The 3D view, where the geometry is actually reachable: the same shapes, the
    // same style table, different coordinates, and the projection done in Rust.
    const relief = await page.$eval('#element', async (element) => {
      const viewer = element.viewer;
      const before = Array.from(viewer._geometry.coords.slice(0, 400));
      const shapes = viewer._geometry.count;
      const events = [];
      viewer.addEventListener('view3dchange', (event) => events.push(event.detail));
      viewer.setView3d({ enabled: true, pitch: 40, exaggeration: 5 });
      await new Promise((resolve) => setTimeout(resolve, 600));
      const after = Array.from(viewer._geometry.coords.slice(0, 400));
      const announced = events.length === 1 && events[0].enabled && events[0].pitch === 40;
      viewer.setView3d(false);
      return {
        relief: viewer.relief,
        shapes: viewer._geometry.count === shapes,
        moved: after.some((value, index) => value !== before[index]),
        length: after.length === before.length,
        // One event for the way in and one for the way out, and the second says
        // the camera is off again.
        announced: announced && events.length === 2 && !events[1].enabled,
      };
    });
    check(relief.relief > 0, `the element reports the map's relief (${relief.relief.toFixed(1)} m)`);
    check(relief.moved && relief.length, '3D reprojects the vertices in place');
    check(relief.shapes, 'without adding or losing a shape');
    check(relief.announced, 'and announces the camera it was given');

    await page.click('#element-centerlines');
    await page.waitForTimeout(200);
    check(
      await page.$eval('#element', (n) => n.viewer.getLayers().find((l) => l.key === 'centerline').visible),
      'setLayers through the element works',
    );
    check(problems.length === 0, `no page errors (${problems.join('; ')})`);
    await page.close();
  }

  // ------------------------------------------- the promises EMBEDDING.md makes
  console.log('embeddability');
  {
    const page = await browser.newPage({ viewport: { width: 900, height: 700 } });
    const problems = [];
    page.on('pageerror', (error) => problems.push(error.message));

    await page.goto(
      `${BASE}/embed.html?map=./sample/mapping_example.osm&theme=light&controls=0&scalebar=0` +
        '&background=transparent&layers=lanelet_fill,bound',
    );
    await page.waitForFunction(() => document.getElementById('message').hidden, { timeout: 60000 });

    const chrome = await page.evaluate(() => {
      const root = document.querySelector('#viewer > div').shadowRoot;
      const canvas = root.querySelector('canvas');
      const pixel = canvas.getContext('2d').getImageData(2, 2, 1, 1).data;
      return {
        controls: root.querySelector('.controls').hidden,
        scalebar: root.querySelector('.scalebar').hidden,
        cornerAlpha: pixel[3],
      };
    });
    check(chrome.controls, 'controls=0 hides the controls');
    check(chrome.scalebar, 'scalebar=0 hides the scale bar');
    check(chrome.cornerAlpha === 0, 'background=transparent leaves the canvas transparent');

    // The whole point of the ResizeObserver: a host resizes the box, not the window.
    const sizes = await page.evaluate(async () => {
      const host = document.querySelector('#viewer');
      const canvas = document.querySelector('#viewer > div').shadowRoot.querySelector('canvas');
      const before = canvas.width;
      host.style.inset = 'auto';
      host.style.width = '300px';
      host.style.height = '200px';
      await new Promise((resolve) => setTimeout(resolve, 400));
      return { before, after: canvas.width };
    });
    check(sizes.after > 0 && sizes.after !== sizes.before, `the canvas follows its container (${sizes.before} -> ${sizes.after})`);

    const second = await page.evaluate(async () => {
      const { LaneletViewer } = await import('./viewer.js');
      const box = document.createElement('div');
      box.style.cssText = 'width:200px;height:150px';
      document.body.append(box);
      const viewer = new LaneletViewer(box, { theme: 'dark', controls: false });
      await viewer.ready;
      await viewer.loadOsm(await (await fetch('./sample/mapping_example.osm')).text());
      const lanelets = viewer.stats.lanelets;
      viewer.destroy();
      return { lanelets, leftBehind: box.children.length };
    });
    check(second.lanelets === 371, 'a second viewer on the same page loads independently');
    check(second.leftBehind === 0, 'destroy() removes everything it made');
    check(problems.length === 0, `no page errors (${problems.join('; ')})`);
    await page.close();
  }
} finally {
  await browser.close();
  server.close();
}

if (failures) {
  console.error(`\n${failures} check(s) failed`);
  process.exit(1);
}
console.log('\nall checks passed');
