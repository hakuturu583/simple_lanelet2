// The iframe face of the viewer: query parameters in, `postMessage` both ways.
//
// This exists because half the places you would want a Lanelet2 map — a wandb HTML
// panel, a notebook output cell, a Grafana text panel, a documentation page, a
// dashboard someone assembled out of iframes — can host a URL but cannot host a
// bundler. Those get:
//
//   <iframe src="https://…/embed.html?map=…&theme=auto" style="width:100%;height:420px;border:0"></iframe>
//
// Hosts that *can* run JavaScript in the same document — a Foxglove panel
// extension, a React app — should import `viewer.js` directly instead and skip the
// iframe entirely. See EMBEDDING.md.
//
// ## Query parameters
//
// | name | meaning |
// | --- | --- |
// | `map` | URL of a `.osm` to fetch. Cross-origin needs CORS headers on it. |
// | `theme` | `dark` (default), `light`, `auto` |
// | `coordinates` | `auto` (default), `projected`, `local` |
// | `layers` | comma-separated layer keys to show; omit for the defaults |
// | `points` | `1` to draw individual map points |
// | `controls`, `tooltip`, `scalebar` | `0` to hide that piece of chrome |
// | `interactive` | `0` for a static picture: no pan, zoom or picking |
// | `background` | a CSS colour. `transparent` is accepted but see the note below |
// | `dnd` | `1` to accept files dropped onto the frame |
// | `origin` | restrict outbound messages to this exact origin |
//
// ## Messages in
//
//   { type: 'lanelet2.load',       osm: '<?xml …', name?, coordinates? }
//   { type: 'lanelet2.loadUrl',    url: 'https://…/map.osm', name? }
//   { type: 'lanelet2.setOptions', theme?, layers?, points?, background?, interactive?, coordinates? }
//   { type: 'lanelet2.setView',    x?, y?, scale? }        // map coordinates
//   { type: 'lanelet2.fit' }
//   { type: 'lanelet2.highlight',  ids: [123, 456] }
//   { type: 'lanelet2.focus',      id: 123 }
//   { type: 'lanelet2.exportSvg',  width?, height?, requestId? }
//   { type: 'lanelet2.clear' }
//
// ## Messages out
//
//   { type: 'lanelet2.ready',   version }
//   { type: 'lanelet2.loadstart', name }
//   { type: 'lanelet2.loaded',  name, stats, errors, coordinateSource, projection, origin, bounds }
//   { type: 'lanelet2.error',   message }
//   { type: 'lanelet2.hover',   shape: {id, label, layer} | null }
//   { type: 'lanelet2.select',  shape: {id, label, layer} | null }
//   { type: 'lanelet2.view',    x, y, scale }
//   { type: 'lanelet2.svg',     svg, requestId }
//
// Every inbound message is answered on the `MessageChannel` port it arrived with,
// when it came with one; otherwise replies and events go to the opener. Nothing is
// evaluated, no message can reach the filesystem, and the only network access is
// `loadUrl` — which the host asked for.
//
// ## A note on `background=transparent`
//
// It makes the *canvas* transparent, which is what it says. Whether that reaches
// the host is another matter: a browser is free to composite a frame onto an opaque
// base, and Chromium does exactly that for a same-origin frame — the host's colour
// behind the iframe never shows and you get white. So a host that wants the panel
// to match its own chrome should pass that colour instead:
//
//   embed.html?background=%23161a22
//
// which this page also paints on itself, so there is no white flash before the map
// arrives. Genuine transparency is available, reliably, by importing `viewer.js`
// into the host document rather than framing this page.

import { LaneletViewer, DEFAULT_LAYERS } from './viewer.js';

const parameters = new URLSearchParams(location.search);
const messageElement = document.getElementById('message');

/// `'*'` unless the host pinned an origin, which it should when the map is
/// sensitive: without it, any window that obtains a handle to this frame's parent
/// chain could observe the metadata we post.
const targetOrigin = parameters.get('origin') || '*';

const viewer = new LaneletViewer(document.getElementById('viewer'), {
  theme: parameters.get('theme') || 'dark',
  coordinates: parameters.get('coordinates') || 'auto',
  layers: parameters.has('layers') ? splitList(parameters.get('layers')) : DEFAULT_LAYERS.slice(),
  drawPoints: flag('points', false),
  controls: flag('controls', true),
  tooltip: flag('tooltip', true),
  scalebar: flag('scalebar', true),
  interactive: flag('interactive', true),
  background: parameters.get('background'),
});

for (const [type, build] of [
  ['loadstart', (detail) => ({ name: detail.name })],
  ['load', (detail) => ({ ...detail })],
  ['error', (detail) => ({ message: detail.message })],
  ['hover', (detail) => ({ shape: detail })],
  ['select', (detail) => ({ shape: detail })],
  ['viewchange', (detail) => ({ ...detail })],
]) {
  viewer.addEventListener(type, (event) => {
    const name = { load: 'loaded', viewchange: 'view' }[type] ?? type;
    post({ type: `lanelet2.${name}`, ...build(event.detail ?? {}) });
  });
}

viewer.addEventListener('load', () => {
  setMessage(null);
  paintPage();
});
viewer.addEventListener('loadstart', () => setMessage('Parsing the map…'));
viewer.addEventListener('error', (event) => setMessage(event.detail.message, true));

/// Paints the document itself in the viewer's colour, so the frame is one colour
/// end to end: no white flash before the first map, and no white gutter if the
/// canvas has not caught up with a resize.
function paintPage() {
  // `backgroundColor` is the override when there is one — `transparent` included —
  // and the theme's own colour otherwise.
  document.documentElement.style.background = viewer.backgroundColor;
}

paintPage();

viewer.ready
  .then(({ version }) => {
    setMessage(parameters.has('map') ? 'Loading…' : 'Waiting for a map…');
    post({ type: 'lanelet2.ready', version });
    if (parameters.has('map')) viewer.loadUrl(parameters.get('map'));
  })
  .catch(() => {
    /* reported through the viewer's error event */
  });

if (flag('dnd', false)) installDragAndDrop();

window.addEventListener('message', (event) => {
  const data = event.data;
  if (!data || typeof data !== 'object' || typeof data.type !== 'string') return;
  if (!data.type.startsWith('lanelet2.')) return;
  // A reply goes back the way the request came, which is what makes
  // `MessageChannel` work for hosts that prefer it over the window relationship.
  const reply = event.ports && event.ports[0] ? (payload) => event.ports[0].postMessage(payload) : post;
  handle(data, reply);
});

function handle(data, reply) {
  switch (data.type) {
    case 'lanelet2.load':
      viewer.loadOsm(String(data.osm ?? ''), { name: data.name, coordinates: data.coordinates });
      break;
    case 'lanelet2.loadUrl':
      viewer.loadUrl(String(data.url ?? ''), { name: data.name });
      break;
    case 'lanelet2.setOptions':
      if (data.theme) viewer.setTheme(data.theme);
      if (data.layers !== undefined) viewer.setLayers(data.layers);
      if (data.points !== undefined) viewer.setDrawPoints(data.points);
      if (data.background !== undefined) viewer.setBackground(data.background);
      if (data.interactive !== undefined) viewer.setInteractive(data.interactive);
      if (data.coordinates) viewer.setCoordinates(data.coordinates);
      // After the options have landed, so the page follows whatever they made of
      // the background rather than what it was a moment ago.
      paintPage();
      break;
    case 'lanelet2.setView':
      viewer.setView(data);
      break;
    case 'lanelet2.fit':
      viewer.fit();
      break;
    case 'lanelet2.highlight':
      viewer.setHighlight(data.ids ?? null);
      break;
    case 'lanelet2.focus':
      viewer.focusOn(data.id, { fraction: data.fraction });
      break;
    case 'lanelet2.exportSvg':
      reply({
        type: 'lanelet2.svg',
        requestId: data.requestId ?? null,
        svg: viewer.toSVG({ width: data.width, height: data.height }),
      });
      break;
    case 'lanelet2.clear':
      viewer.clear();
      setMessage('Waiting for a map…');
      break;
    default:
      break;
  }
}

function installDragAndDrop() {
  const stop = (event) => event.preventDefault();
  window.addEventListener('dragover', stop);
  window.addEventListener('drop', (event) => {
    event.preventDefault();
    const file = event.dataTransfer?.files?.[0];
    if (file) viewer.loadFile(file);
  });
}

function post(payload) {
  // `parent` is `window` itself when the page is opened directly, and posting to
  // yourself is harmless — it just means a standalone embed.html is inspectable.
  parent.postMessage(payload, targetOrigin);
}

function setMessage(text, isError = false) {
  messageElement.hidden = !text;
  messageElement.textContent = text ?? '';
  messageElement.classList.toggle('error', Boolean(isError));
}

function splitList(value) {
  return String(value ?? '')
    .split(/[\s,]+/)
    .filter(Boolean);
}

function flag(name, fallback) {
  if (!parameters.has(name)) return fallback;
  const value = parameters.get(name);
  return value !== 'false' && value !== '0' && value !== 'off';
}
