# Embedding the viewer

The viewer is a component, not a page. `index.html` is one consumer of it; a
Foxglove panel or a wandb HTML block is another, and neither needs anything the
demo has that the others do not.

There are two ways in, and which one you want is decided by a single question:
**can the host run your JavaScript in its own document?**

| | use | you get |
| --- | --- | --- |
| Foxglove panel extension, React/Vue app, docs site with a bundler, plain HTML page | [`viewer.js`](viewer.js) directly | the full API, real transparency, no message plumbing |
| wandb HTML panel, notebook output, Grafana text panel, a dashboard someone assembled out of iframes | [`embed.html`](embed.html) in an `<iframe>` | a URL and a `postMessage` protocol |

Both are live at
<https://hakuturu583.github.io/simple_lanelet2/embed-example.html>, which is also
the file to read if you would rather see it than read about it.

---

## 1 · The element, in your document

```html
<script type="module" src="https://hakuturu583.github.io/simple_lanelet2/viewer.js"></script>

<lanelet2-viewer
  src="map.osm"
  theme="auto"
  layers="lanelet_fill,bound,regulatory"
  style="display:block; width:100%; height:420px"
></lanelet2-viewer>
```

Attributes are live — set `theme` and the theme changes. The full API is on
`element.viewer`, or construct it yourself and skip the element:

```js
import { LaneletViewer } from './viewer.js';

const viewer = new LaneletViewer(container, { theme: 'auto', background: 'transparent' });
viewer.addEventListener('load', (e) => console.log(e.detail.stats));
viewer.addEventListener('select', (e) => console.log(e.detail?.id));
await viewer.loadOsm(osmText);
```

### What it promises a host

- **No global state.** Nothing touches `document.body`, the page title, the URL, or
  a global stylesheet. Several viewers can share a page; the demo page and the
  example page both do.
- **Nothing leaks either way.** Everything lives under a shadow root, so the
  host's CSS reset cannot reach the canvas and the viewer's styles cannot reach the
  host.
- **It sizes itself from its container**, with a `ResizeObserver`. A panel is
  resized by its host far more often than a window is; a `resize` listener would
  miss every one of those.
- **`destroy()` really releases everything** — DOM, observers, and the Rust-side
  objects. Panels are unmounted and remounted as a matter of course.

### API

| | |
| --- | --- |
| `new LaneletViewer(container, options)` | mounts into `container` |
| `viewer.ready` | resolves to `{version}` when the wasm module is up |
| `loadOsm(text, {name, coordinates})` | parse and display |
| `loadUrl(url)` / `loadFile(file)` | fetch, or read a `File`/`Blob` |
| `clear()` | discard the map |
| `setTheme('dark'\|'light'\|'auto')` | `auto` follows `prefers-color-scheme` and keeps following it |
| `setLayers(keysOrMap)` / `getLayers()` | show and hide layers; instant, no reparse |
| `setDrawPoints(bool)` | individual map points — the one option that rebuilds |
| `setCoordinates('auto'\|'projected'\|'local')` | re-reads the file |
| `setBackground(colour\|'transparent'\|null)` | `null` uses the theme's own |
| `setInteractive(bool)` | off for a static picture |
| `fit()` / `getView()` / `setView({x, y, scale})` | the view, in map coordinates |
| `setHighlight(ids)` / `focusOn(id)` | outline or centre on primitives by id |
| `toSVG({width, height})` | a standalone SVG, from the same Rust renderer |
| `viewer.stats`, `viewer.legend`, `viewer.backgroundColor` | after `load` |
| `destroy()` | |

Events, as `CustomEvent`s: `loadstart`, `load`, `error`, `hover`, `select`,
`viewchange`. `hover` and `select` carry `{id, label, layer}` or `null`.

Layer keys: `lanelet_fill`, `area`, `polygon`, `bound`, `regulatory`,
`centerline`, `direction`, `point`.

### Foxglove

A Foxglove panel extension is given a `panelElement` and asked to return a
teardown function, which is exactly the shape `LaneletViewer` has. Sketch — check
the API against the Studio version you are building for:

```ts
import { ExtensionContext, PanelExtensionContext } from '@foxglove/extension';
import { LaneletViewer } from './viewer.js';   // vendored into the extension bundle

function initPanel(context: PanelExtensionContext) {
  const viewer = new LaneletViewer(context.panelElement, { theme: 'auto' });

  context.onRender = (renderState, done) => {
    // Highlight the lanelet the ego vehicle is on, wherever you get that from.
    const current = readCurrentLaneletId(renderState);
    viewer.setHighlight(current ?? null);
    done();
  };

  loadYourMap().then((osm) => viewer.loadOsm(osm));
  return () => viewer.destroy();          // Studio unmounts panels freely
}

export function activate(context: ExtensionContext) {
  context.registerPanel({ name: 'Lanelet2 map', initPanel });
}
```

Bundle `viewer.js` and `pkg/` with the extension — Studio will not fetch them from
a CDN for you — and point `initWasm({ wasmUrl })` at wherever your bundler put the
`.wasm` if it is not beside `viewer.js`:

```js
import wasmUrl from './pkg/ll2_wasm_bg.wasm?url';
import { initWasm } from './viewer.js';
await initWasm({ wasmUrl });
```

Foxglove has no Lanelet2 message type, so the map comes from wherever your setup
keeps it: a file the extension ships, a URL, or a topic carrying the OSM text.

---

## 2 · The iframe

```html
<iframe
  src="https://hakuturu583.github.io/simple_lanelet2/embed.html?map=https://example.org/map.osm&theme=dark&background=%23161a22"
  style="width:100%; height:420px; border:0"
  title="Lanelet2 map"
></iframe>
```

That alone is a working map. Everything past it is optional.

### Query parameters

| name | meaning |
| --- | --- |
| `map` | URL of a `.osm` to fetch. Cross-origin needs CORS headers **on that URL** |
| `theme` | `dark` (default), `light`, `auto` |
| `coordinates` | `auto` (default), `projected`, `local` |
| `layers` | comma-separated layer keys; omit for the defaults |
| `points` | `1` to draw individual map points |
| `controls`, `tooltip`, `scalebar` | `0` to hide that piece of chrome |
| `interactive` | `0` for a static picture — no pan, zoom or picking |
| `background` | a CSS colour (URL-encode the `#`), or `transparent` — see below |
| `dnd` | `1` to accept files dropped onto the frame |
| `origin` | restrict outbound messages to this exact origin |

### Messages

Post to `frame.contentWindow`:

```js
frame.contentWindow.postMessage({ type: 'lanelet2.load', osm: text }, '*');
```

| in | |
| --- | --- |
| `lanelet2.load` | `{osm, name?, coordinates?}` |
| `lanelet2.loadUrl` | `{url, name?}` |
| `lanelet2.setOptions` | `{theme?, layers?, points?, background?, interactive?, coordinates?}` |
| `lanelet2.setView` | `{x?, y?, scale?}` in map coordinates |
| `lanelet2.fit` | |
| `lanelet2.highlight` | `{ids: [123, 456]}` |
| `lanelet2.focus` | `{id, fraction?}` |
| `lanelet2.exportSvg` | `{width?, height?, requestId?}` |
| `lanelet2.clear` | |

| out | |
| --- | --- |
| `lanelet2.ready` | `{version}` — post nothing before this arrives |
| `lanelet2.loadstart` | `{name}` |
| `lanelet2.loaded` | `{name, stats, errors, coordinateSource, projection, origin, bounds}` |
| `lanelet2.error` | `{message}` |
| `lanelet2.hover` / `lanelet2.select` | `{shape}` — `{id, label, layer}` or `null` |
| `lanelet2.view` | `{x, y, scale}` |
| `lanelet2.svg` | `{svg, requestId}` |

A request that arrives with a `MessagePort` is answered on that port, so
`MessageChannel` works if you would rather not filter on `window`'s message
stream.

Outbound messages go to `'*'` unless you pass `origin=`. Pass it if the map is
sensitive.

### wandb, notebooks, dashboards

Anything that renders raw HTML can host the iframe:

```python
import wandb
wandb.log({"map": wandb.Html(
    '<iframe src="https://hakuturu583.github.io/simple_lanelet2/embed.html'
    '?map=https://example.org/map.osm&theme=dark&background=%23161a22"'
    ' style="width:100%;height:420px;border:0"></iframe>'
)})
```

Two things to check in your own instance rather than take on trust: whether the
host sandboxes its HTML block in a way that forbids a nested frame, and whether
the map URL you point at sends CORS headers. If the outer sandbox is the problem,
serve `embed.html` from the same origin as the host.

There is no equivalent slot in the rerun viewer today — it renders its own
spaces, not arbitrary HTML — so the practical pattern next to rerun is the
notebook or dashboard cell beside it, not a panel inside it.

---

## Backgrounds, honestly

`background=transparent` makes the *canvas* transparent, and in the element that
is the end of the story — the gradient behind the second map on the example page
belongs to the page, not to the viewer.

In an **iframe** it is unreliable. A browser may composite a frame onto an opaque
base whatever the framed document asks for, and Chromium does exactly that for a
same-origin frame: the colour you put behind the iframe never shows and you get
white. So for a frame, pass your panel's colour instead —
`?background=%23161a22` — which `embed.html` also paints on itself, so there is no
white flash before the first map arrives.

## Hosting it yourself

Everything the viewer needs is static: `viewer.js`, `embed.html`, `embed.js` and
`pkg/`. Copy them next to each other and serve them over HTTP — `file://` will
not do, because the page is an ES module and fetches its own `.wasm`. Build them
with `tools/build_web.sh`; see [README.md](README.md).

No CDN, no analytics, no network access of its own: the only fetch the viewer ever
makes is the one you asked for with `map` or `loadUrl`.
