// A Lanelet2 map viewer you can drop into someone else's application.
//
// This file is the product; `app.js` next to it is one consumer, and `embed.js` is
// another. It is written to survive being mounted inside a host it knows nothing
// about — a Foxglove panel, a wandb HTML block, a notebook output cell, a docs page:
//
//   * everything lives under a shadow root, so no stylesheet of the host's can
//     reach in and nothing of ours leaks out. There is no CSS file to include.
//   * it sizes itself from its container with a ResizeObserver, not from the
//     window, because a panel is resized by its host far more often than a window is.
//   * it never touches `document.body`, `window.location`, the page title, or any
//     other global — several viewers can share a page.
//   * `destroy()` really releases everything, including the Rust-side objects,
//     because a panel is unmounted and remounted as a matter of course.
//
// The rendering itself: shapes arrive from Rust as flat typed arrays and are
// grouped by (layer, style) into one `Path2D` each, once per load. A frame sets the
// canvas transform and issues one fill/stroke per group — a few dozen calls — no
// matter how large the map is. Stroke widths are divided by the scale so a lane
// marking stays one pixel wide at every zoom.
//
// That holds for the 3D view too, and it is why there is no 3D renderer here. A
// Lanelet2 map has an elevation on every node; `setView3d` asks Rust to project the
// scene from a tilted camera instead of from straight above, and what arrives is
// the same flat arrays with different numbers in them, already in painter's order.
// So relief costs a rebuild when the camera moves and nothing at all per frame.
//
//   import { LaneletViewer } from './viewer.js';
//   const viewer = new LaneletViewer(element, { theme: 'auto' });
//   viewer.addEventListener('load', (event) => console.log(event.detail.stats));
//   await viewer.loadOsm(await file.text());
//
// or, declaratively:
//
//   <script type="module" src="./viewer.js"></script>
//   <lanelet2-viewer src="map.osm" theme="auto"></lanelet2-viewer>

import init, { layers as layerTable, load_osm, SceneOptions, version } from './pkg/ll2_wasm.js';

/// The layer table, `[{key, label, default}]`, in the order the Rust side indexes
/// layers by. Empty until [`initWasm`] resolves, because it *is* the Rust table —
/// restating it here would be a cross-language ordering invariant with nothing to
/// enforce it, and adding a layer would silently mislabel every shape.
export const LAYERS = [];

/// The keys of the layers a viewer shows when nobody says otherwise. Also from
/// Rust: it is `VizOptions::default()`.
export function defaultLayers() {
  return LAYERS.filter((layer) => layer.default).map((layer) => layer.key);
}

/// The one layer a viewer builds on demand rather than building and hiding: a
/// city map has millions of points and a shape each is not free.
const EXPENSIVE_LAYER = 'point';

let wasmPromise = null;

/**
 * Loads and instantiates the WebAssembly module. Idempotent, and safe to call
 * before or in parallel with constructing viewers — they await it themselves.
 *
 * @param {{wasmUrl?: string|URL|Response|ArrayBuffer}} [options] where to fetch the
 *   `.wasm` from, when it is not the `pkg/` directory next to this module. Hosts
 *   that bundle their own assets will need this.
 * @returns {Promise<{version: string}>}
 */
export function initWasm(options = {}) {
  if (!wasmPromise) {
    wasmPromise = init(options.wasmUrl ? { module_or_path: options.wasmUrl } : undefined)
      .then(() => {
        LAYERS.splice(0, LAYERS.length, ...JSON.parse(layerTable()));
        return { version: version() };
      })
      .catch((error) => {
        // A failed instantiation must not be cached as a resolved promise, or every
        // later viewer on the page silently renders nothing.
        wasmPromise = null;
        throw error;
      });
  }
  return wasmPromise;
}

const STYLE_TEXT = `
:host { display: block; position: relative; contain: layout paint; }
.root { position: absolute; inset: 0; overflow: hidden; }
canvas { display: block; width: 100%; height: 100%; touch-action: none; }
canvas.interactive { cursor: grab; }
canvas.panning { cursor: grabbing; }
.tooltip {
  position: absolute; z-index: 3; padding: 4px 8px; border-radius: 6px;
  border: 1px solid var(--ll2-border, rgba(128,140,160,0.35));
  background: var(--ll2-surface, rgba(22,26,34,0.94));
  color: var(--ll2-text, #e6eaf0);
  font: 12px/1.35 ui-sans-serif, system-ui, sans-serif;
  pointer-events: none; max-width: 22rem; white-space: nowrap;
  overflow: hidden; text-overflow: ellipsis;
}
.controls { position: absolute; right: 10px; bottom: 10px; z-index: 2; display: grid; gap: 5px; }
.controls button {
  width: 30px; height: 30px; padding: 0; border-radius: 7px; cursor: pointer;
  border: 1px solid var(--ll2-border, rgba(128,140,160,0.35));
  background: var(--ll2-surface, rgba(22,26,34,0.88));
  color: var(--ll2-text, #e6eaf0);
  font: 15px/1 ui-sans-serif, system-ui, sans-serif;
}
.controls button.small { font-size: 11px; }
.controls button:hover { border-color: var(--ll2-accent, #4dd0e1); }
/* A map whose nodes carry no elevation has nothing to draw in relief. */
.controls button:disabled { opacity: 0.4; cursor: default; }
.controls button:disabled:hover { border-color: var(--ll2-border, rgba(128,140,160,0.35)); }
.scalebar {
  position: absolute; left: 10px; bottom: 10px; z-index: 2; pointer-events: none;
  color: var(--ll2-muted, #98a2b3);
  font: 11px/1 ui-sans-serif, system-ui, sans-serif;
}
.scalebar .bar { height: 6px; min-width: 30px; border: 1px solid currentColor; border-top: none; }
.scalebar .label { display: block; margin-bottom: 3px; font-variant-numeric: tabular-nums; }
[hidden] { display: none !important; }
`;

/// The light and dark chrome colours, which have to match the Rust palette closely
/// enough that the overlays do not look bolted on.
const CHROME = {
  dark: {
    '--ll2-surface': 'rgba(22, 26, 34, 0.9)',
    '--ll2-border': 'rgba(120, 132, 152, 0.35)',
    '--ll2-text': '#e6eaf0',
    '--ll2-muted': '#98a2b3',
    '--ll2-accent': '#4dd0e1',
    highlight: '#ffd74a',
  },
  light: {
    '--ll2-surface': 'rgba(255, 255, 255, 0.94)',
    '--ll2-border': 'rgba(90, 100, 120, 0.3)',
    '--ll2-text': '#1b1f27',
    '--ll2-muted': '#5c6675',
    '--ll2-accent': '#0b8499',
    highlight: '#d98b00',
  },
};

/**
 * A map viewer bound to one container element.
 *
 * Dispatches, as `CustomEvent`s:
 *
 * | event | `detail` |
 * | --- | --- |
 * | `loadstart` | `{name}` |
 * | `load` | `{name, stats, errors, problems, coordinateSource, projection, origin, bounds, relief}` |
 * | `error` | `{message}` |
 * | `hover` | `{id, label, layer} \| null` |
 * | `select` | `{id, label, layer} \| null` |
 * | `viewchange` | `{x, y, scale}` in map coordinates |
 * | `view3dchange` | `{enabled, yaw, pitch, exaggeration}` |
 */
export class LaneletViewer extends EventTarget {
  /**
   * @param {HTMLElement} container mounted into, and sized from. Anything with a
   *   layout box will do; the viewer positions itself absolutely inside it.
   * @param {object} [options]
   */
  constructor(container, options = {}) {
    super();
    if (!container) throw new Error('LaneletViewer needs a container element');

    this.options = {
      theme: 'dark', // 'dark' | 'light' | 'auto'
      coordinates: 'auto', // 'auto' | 'projected' | 'local'
      /// Layer keys to show. `null` means the Rust defaults, which are not known
      /// until the module is up.
      layers: null,
      drawPoints: false,
      /// Draw the map in relief rather than from straight above. `false` is the
      /// map view; the angles below are only read when it is on.
      threeD: false,
      /// Degrees the map is turned about the vertical, then degrees above the
      /// horizon, then the multiplier on every elevation.
      yaw: 30,
      pitch: 55,
      exaggeration: 1,
      controls: true,
      tooltip: true,
      scalebar: true,
      interactive: true,
      /// `null` uses the theme's own colour; a CSS colour overrides it, and
      /// `'transparent'` lets a host's own background show through.
      background: null,
      wasmUrl: undefined,
      ...options,
    };

    this.container = container;
    this._visible = new Set(this.options.layers ?? []);
    this._groups = [];
    this._geometry = null;
    this._scene = null;
    this._handle = null;
    this._index = null;
    this._view = { scale: 1, tx: 0, ty: 0 };
    this._hover = -1;
    this._pinned = null;
    this._highlight = new Set();
    this._frame = 0;
    this._destroyed = false;
    this._source = null;
    this._name = null;
    this.stats = null;
    this.legend = [];

    this._buildDom();
    this._observeSize();
    this._observeTheme();
    this._installPointerHandlers();

    this.ready = initWasm({ wasmUrl: this.options.wasmUrl })
      .then((info) => {
        // The default visible set lives in Rust with the layer table, so it can
        // only be resolved once the module is up. Nothing draws before then.
        if (this.options.layers === null) this._visible = new Set(defaultLayers());
        return info;
      })
      .catch((error) => {
        this._fail(`The WebAssembly module failed to load: ${message(error)}`);
        throw error;
      });
  }

  // --- lifecycle -------------------------------------------------------------

  _buildDom() {
    // A shadow root on a wrapper we own, rather than on the container: the caller
    // may already have put children in there, and a shadow root would hide them.
    this._host = document.createElement('div');
    this._host.style.cssText = 'position:absolute;inset:0;';
    if (getComputedStyle(this.container).position === 'static') {
      this.container.style.position = 'relative';
    }
    this.container.append(this._host);
    this._shadow = this._host.attachShadow({ mode: 'open' });

    const style = document.createElement('style');
    style.textContent = STYLE_TEXT;
    this._root = document.createElement('div');
    this._root.className = 'root';

    this._canvas = document.createElement('canvas');
    this._context = this._canvas.getContext('2d', { alpha: true });

    this._tooltip = document.createElement('div');
    this._tooltip.className = 'tooltip';
    this._tooltip.hidden = true;

    this._scalebar = document.createElement('div');
    this._scalebar.className = 'scalebar';
    this._scalebar.innerHTML = '<span class="label"></span><div class="bar"></div>';
    this._scalebarLabel = this._scalebar.querySelector('.label');
    this._scalebarBar = this._scalebar.querySelector('.bar');
    this._scalebar.hidden = true;

    this._controls = document.createElement('div');
    this._controls.className = 'controls';
    this._view3dButton = this._controlButton(
      '3D',
      'Draw the map in relief, using the elevation of its nodes',
      () => this.setView3d(!this.options.threeD),
      'small',
    );
    this._controls.append(
      this._controlButton('+', 'Zoom in', () => this.zoomBy(1.4)),
      this._controlButton('−', 'Zoom out', () => this.zoomBy(1 / 1.4)),
      this._controlButton('Fit', 'Fit the map to the view', () => this.fit(), 'small'),
      this._view3dButton,
    );

    this._root.append(this._canvas, this._scalebar, this._controls, this._tooltip);
    this._shadow.append(style, this._root);

    this._applyChrome();
    this._applyChromeVisibility();
  }

  _controlButton(text, title, onClick, className = '') {
    const button = document.createElement('button');
    button.type = 'button';
    button.textContent = text;
    button.title = title;
    button.className = className;
    button.addEventListener('click', onClick);
    return button;
  }

  _applyChromeVisibility() {
    this._controls.hidden = !this.options.controls || !this.options.interactive;
    this._scalebar.hidden = !this.options.scalebar || !this._geometry;
    this._canvas.classList.toggle('interactive', this.options.interactive);
    if (!this.options.tooltip) this._tooltip.hidden = true;
    // The button says what it will give you, and offering relief on a map that has
    // none would give a tilted flat sheet and no way to tell why. It stays live
    // while the 3D view is on, whatever the map: the way out must not be the thing
    // that gets disabled.
    this._view3dButton.textContent = this.options.threeD ? '2D' : '3D';
    this._view3dButton.disabled =
      Boolean(this._geometry) && !this.options.threeD && this.relief < 0.5;
    this._view3dButton.title = this._view3dButton.disabled
      ? 'Every node in this map is at the same elevation'
      : this.options.threeD
        ? 'Back to the map view'
        : 'Draw the map in relief, using the elevation of its nodes';
  }

  _observeSize() {
    // A panel is resized by its host constantly and by the window almost never,
    // which is exactly the wrong way round for a `resize` listener.
    this._resizeObserver = new ResizeObserver(() => {
      this._resize();
      this._draw();
    });
    this._resizeObserver.observe(this.container);
    this._resize();
  }

  _observeTheme() {
    if (typeof matchMedia !== 'function') return;
    this._media = matchMedia('(prefers-color-scheme: dark)');
    this._onMediaChange = () => {
      if (this.options.theme === 'auto') this._restyle();
    };
    // Safari before 14 has only the deprecated form.
    if (this._media.addEventListener) this._media.addEventListener('change', this._onMediaChange);
    else if (this._media.addListener) this._media.addListener(this._onMediaChange);
  }

  /** Releases the DOM, the observers and the Rust-side objects. */
  destroy() {
    if (this._destroyed) return;
    this._destroyed = true;
    if (this._frame) cancelAnimationFrame(this._frame);
    this._resizeObserver?.disconnect();
    if (this._media) {
      if (this._media.removeEventListener) this._media.removeEventListener('change', this._onMediaChange);
      else if (this._media.removeListener) this._media.removeListener(this._onMediaChange);
    }
    this._freeScene();
    this._freeHandle();
    this._host.remove();
    this._groups = [];
    this._geometry = null;
    this._index = null;
    this._source = null;
  }

  _freeScene() {
    try {
      this._scene?.free();
    } catch {
      /* already freed */
    }
    this._scene = null;
  }

  _freeHandle() {
    try {
      this._handle?.free();
    } catch {
      /* already freed */
    }
    this._handle = null;
  }

  // --- loading ---------------------------------------------------------------

  /**
   * Parses and displays OSM XML.
   *
   * @param {string} text the file's contents
   * @param {{name?: string, coordinates?: string, keepView?: boolean}} [options]
   * @returns {Promise<object>} the same detail the `load` event carries
   */
  async loadOsm(text, options = {}) {
    await this.ready;
    if (this._destroyed) return null;
    const name = options.name ?? this._name ?? 'map';
    this._name = name;
    this._emit('loadstart', { name });
    // Yield once, so a host that paints a spinner on `loadstart` gets to.
    await new Promise((resolve) => setTimeout(resolve, 0));

    if (options.coordinates) this.options.coordinates = options.coordinates;
    let handle;
    try {
      handle = load_osm(text, this.options.coordinates);
    } catch (error) {
      this._fail(message(error));
      return null;
    }
    this._freeHandle();
    this._handle = handle;
    // Only kept when there is nothing else to re-read from: for a 50 MB map this
    // is 100 MB of UTF-16 held for the viewer's lifetime, on top of the copy
    // wasm already made.
    this._source = options.source ?? { text };

    this._rebuild({ keepView: options.keepView === true });

    const detail = {
      name,
      stats: JSON.parse(handle.stats_json()),
      errors: handle.errors(),
      problems: handle.error_count(),
      coordinateSource: handle.coordinate_source(),
      projection: handle.projection(),
      origin: { lat: handle.origin_lat(), lon: handle.origin_lon() },
      bounds: Array.from(this._scene.bounds()),
      // Metres from the map's lowest point to its highest, and zero for the many
      // files whose nodes carry no `ele` — the answer to "is there anything here
      // for a 3D view to show?", which a host has no other way to ask.
      relief: this._relief,
    };
    this.stats = detail.stats;
    this._emit('load', detail);
    return detail;
  }

  /**
   * Fetches a map and displays it. The URL must be readable by this page —
   * cross-origin means the server has to send CORS headers.
   */
  async loadUrl(url, options = {}) {
    await this.ready;
    const name = options.name ?? basename(String(url));
    this._emit('loadstart', { name });
    let text;
    try {
      const response = await fetch(url, { credentials: options.credentials ?? 'same-origin' });
      if (!response.ok) throw new Error(`HTTP ${response.status} ${response.statusText}`);
      text = await response.text();
    } catch (error) {
      this._fail(`Could not fetch ${url}: ${message(error)}`);
      return null;
    }
    return this.loadOsm(text, { ...options, name, source: { url } });
  }

  /** Reads a `File` or `Blob` — what a drop handler or an `<input type=file>` has. */
  async loadFile(file, options = {}) {
    const name = options.name ?? (file.name ? basename(file.name) : 'map');
    return this.loadOsm(await file.text(), { ...options, name, source: { file } });
  }

  /**
   * Loads any `.osm` dropped on `target`, and returns a function that stops.
   *
   * The half of a drop handler that is about maps rather than about a page, so
   * both the demo and the iframe get it from here instead of writing it twice.
   */
  acceptDrops(target = this.container) {
    const over = (event) => event.preventDefault();
    const drop = (event) => {
      event.preventDefault();
      const file = event.dataTransfer?.files?.[0];
      if (file) this.loadFile(file);
    };
    target.addEventListener('dragover', over);
    target.addEventListener('drop', drop);
    return () => {
      target.removeEventListener('dragover', over);
      target.removeEventListener('drop', drop);
    };
  }

  /** Discards the current map, leaving an empty viewer. */
  clear() {
    this._freeScene();
    this._freeHandle();
    this._groups = [];
    this._geometry = null;
    this._index = null;
    this._source = null;
    this._relief = 0;
    this.stats = null;
    this.legend = [];
    this._hover = -1;
    this._pinned = null;
    this._scalebar.hidden = true;
    this._tooltip.hidden = true;
    this._draw();
  }

  // --- options ---------------------------------------------------------------

  /** `'dark'`, `'light'`, or `'auto'` to follow `prefers-color-scheme`. */
  setTheme(theme) {
    if (!['dark', 'light', 'auto'].includes(theme)) return;
    this.options.theme = theme;
    this._applyChrome();
    if (this._handle) this._restyle();
    else this._draw();
  }

  /** The theme actually in use, with `'auto'` resolved. */
  get theme() {
    if (this.options.theme !== 'auto') return this.options.theme;
    return this._media && this._media.matches === false ? 'light' : 'dark';
  }

  /**
   * @param {string|string[]|Record<string, boolean>} layers a key, a list of keys
   *   to make the whole visible set, or a partial map of key to visibility.
   */
  setLayers(layers) {
    if (typeof layers === 'string') layers = [layers];
    if (Array.isArray(layers)) {
      this._visible = new Set(layers.filter((key) => LAYERS.some((l) => l.key === key)));
    } else {
      for (const [key, visible] of Object.entries(layers ?? {})) {
        if (!LAYERS.some((l) => l.key === key)) continue;
        if (visible) this._visible.add(key);
        else this._visible.delete(key);
      }
    }
    this._hover = -1;
    this._tooltip.hidden = true;
    this._draw();
  }

  /** `[{key, label, visible, count}]`, with counts once a map is loaded. */
  getLayers() {
    return LAYERS.map((layer) => ({
      ...layer,
      visible: this._visible.has(layer.key),
      count: this._layerCounts?.get(layer.key) ?? 0,
    }));
  }

  /**
   * Individual map points are off by default: a city map has millions of them, and
   * unlike every other layer this one really does rebuild the scene.
   */
  setDrawPoints(enabled) {
    this.options.drawPoints = Boolean(enabled);
    if (this._handle) this._rebuild({ keepView: true });
  }

  /**
   * Draws the map in relief, from the elevation its nodes carry, instead of from
   * straight above.
   *
   * Rebuilds the scene, because the projection happens in Rust — so this is a knob
   * to set, not one to animate. Pass a partial object to change one angle:
   * `setView3d({yaw: 90})` turns the map without leaving 3D.
   *
   * @param {boolean|{enabled?: boolean, yaw?: number, pitch?: number,
   *   exaggeration?: number}} view `true`/`false` is shorthand for `{enabled}`.
   */
  setView3d(view) {
    const settings = typeof view === 'boolean' ? { enabled: view } : (view ?? {});
    if (settings.enabled !== undefined) this.options.threeD = Boolean(settings.enabled);
    for (const key of ['yaw', 'pitch', 'exaggeration']) {
      if (Number.isFinite(settings[key])) this.options[key] = settings[key];
    }
    // Turning the camera moves everything on the page, so a view kept from before
    // would leave the map half out of frame; refitting is what a person expects.
    if (this._handle) this._rebuild({ keepView: false });
    this._applyChromeVisibility();
    // The viewer has a 3D button of its own, so a host with a control for it needs
    // to hear about the presses it did not make.
    this._emit('view3dchange', this.getView3d());
  }

  /** `{enabled, yaw, pitch, exaggeration}` — the camera, as `setView3d` takes it. */
  getView3d() {
    const { threeD, yaw, pitch, exaggeration } = this.options;
    return { enabled: threeD, yaw, pitch, exaggeration };
  }

  /**
   * Metres between the loaded map's lowest and highest point, or `0` when its nodes
   * carry no elevation at all — in which case there is nothing for the 3D view to
   * show, and a host offering the option should say so.
   */
  get relief() {
    return this._relief ?? 0;
  }

  /** `'auto'`, `'projected'` or `'local'`. Re-parses the file that is loaded. */
  async setCoordinates(source) {
    this.options.coordinates = source;
    if (!this._source) return;
    const { text, file, url } = this._source;
    if (file) await this.loadFile(file);
    else if (url) await this.loadUrl(url);
    else await this.loadOsm(text, { source: this._source });
  }

  /** A CSS colour, `'transparent'` to show the host's own background, or `null`. */
  setBackground(background) {
    this.options.background = background;
    this._draw();
  }

  /**
   * The colour the canvas is actually painted with — the override if one was set,
   * otherwise the theme's own. A host that wants its panel to match the map,
   * rather than the other way round, reads this after `load`.
   */
  get backgroundColor() {
    if (this.options.background) return this.options.background;
    return this._sceneBackground ?? (this.theme === 'light' ? '#f7f8fa' : '#11141a');
  }

  /**
   * Shows or hides the viewer's own overlays.
   *
   * @param {{controls?: boolean, tooltip?: boolean, scalebar?: boolean}} chrome
   */
  setChrome(chrome = {}) {
    for (const key of ['controls', 'tooltip', 'scalebar']) {
      if (chrome[key] !== undefined) this.options[key] = Boolean(chrome[key]);
    }
    this._applyChromeVisibility();
    this._draw();
  }

  /** Turns pan, zoom and picking off — for a thumbnail, or a host-driven view. */
  setInteractive(enabled) {
    this.options.interactive = Boolean(enabled);
    this._applyChromeVisibility();
    if (!enabled) {
      this._hover = -1;
      this._tooltip.hidden = true;
    }
    this._draw();
  }

  // --- view ------------------------------------------------------------------

  /** Frames the whole map. */
  fit() {
    if (!this._geometry) return;
    const [minX, minY, maxX, maxY] = this._geometry.bounds;
    const spanX = Math.max(maxX - minX, 1e-6);
    const spanY = Math.max(maxY - minY, 1e-6);
    this._view.scale = Math.min(this._width / spanX, this._height / spanY) * 0.94;
    // Scene coordinates are already relative to the map's centre, so centring the
    // map is exactly centring the world origin.
    this._view.tx = this._width / 2;
    this._view.ty = this._height / 2;
    this._draw();
    this._emitViewChange();
  }

  /**
   * `{x, y, scale}` — the map coordinate at the centre, and pixels per metre.
   *
   * Under the 3D view these are the *drawing* coordinates rather than the map's: a
   * tilted camera has no one map coordinate at the centre of the screen, since a
   * point on a hill and one on the ground behind it are drawn in the same place.
   */
  getView() {
    const centre = this._geometry?.centre ?? [0, 0];
    return {
      x: centre[0] + (this._width / 2 - this._view.tx) / this._view.scale,
      y: centre[1] + (this._view.ty - this._height / 2) / this._view.scale,
      scale: this._view.scale,
    };
  }

  /** Moves the view. Coordinates are the map's own, as `getView` reports them. */
  setView({ x, y, scale } = {}) {
    if (!this._geometry) return;
    const centre = this._geometry.centre;
    if (Number.isFinite(scale)) this._view.scale = clamp(scale, 1e-4, 5000);
    if (Number.isFinite(x)) this._view.tx = this._width / 2 - (x - centre[0]) * this._view.scale;
    if (Number.isFinite(y)) this._view.ty = this._height / 2 + (y - centre[1]) * this._view.scale;
    this._draw();
    this._emitViewChange();
  }

  zoomBy(factor) {
    this._zoomAt(this._width / 2, this._height / 2, factor);
    this._draw();
    this._emitViewChange();
  }

  /**
   * Outlines the primitives with these ids — the lanelet the ego vehicle is on,
   * a route, a search result. Pass nothing to clear.
   */
  setHighlight(ids) {
    const list = ids === null || ids === undefined ? [] : Array.isArray(ids) ? ids : [ids];
    this._highlight = new Set(list.map(Number));
    // Resolved to a path here rather than in the frame: a host tracking an ego
    // vehicle calls this continuously, and scanning every shape in the map on
    // every frame is the difference between free and unusable.
    this._highlightPath = null;
    if (this._highlight.size && this._geometry) {
      const path = new Path2D();
      let any = false;
      for (const id of this._highlight) {
        for (const shape of this._byId.get(id) ?? []) {
          appendShapeTo(path, this._geometry, shape);
          any = true;
        }
      }
      if (any) this._highlightPath = path;
    }
    this._draw();
  }

  /** Centres the view on a primitive, optionally zooming to fill `fraction`. */
  focusOn(id, { fraction = 0.4 } = {}) {
    const shape = this._findShape(Number(id));
    if (shape < 0) return false;
    const box = shapeBounds(this._geometry, shape);
    const centre = this._geometry.centre;
    const spanX = Math.max(box[2] - box[0], 5);
    const spanY = Math.max(box[3] - box[1], 5);
    const scale = Math.min(this._width / spanX, this._height / spanY) * fraction;
    this.setView({
      x: centre[0] + (box[0] + box[2]) / 2,
      y: centre[1] + (box[1] + box[3]) / 2,
      scale,
    });
    return true;
  }

  // --- export ----------------------------------------------------------------

  /**
   * Renders the map to a standalone SVG document, through the same Rust renderer
   * the canvas is fed by — so what you export is what you see.
   */
  toSVG({ width = 1920, height = 1200 } = {}) {
    if (!this._handle) return null;
    const options = this._sceneOptions({ forExport: true });
    const svg = this._handle.to_svg(options, width, height);
    options.free();
    return svg;
  }

  // --- scene -----------------------------------------------------------------

  /**
   * Every layer but `point` is always built and hidden here, so a toggle costs a
   * repaint rather than a rebuild. Points are the exception because a city map has
   * millions of them and building a shape for each is not free.
   */
  _sceneOptions({ forExport = false } = {}) {
    const options = new SceneOptions();
    options.theme = this.theme;
    // The camera, so an SVG export is drawn from where the canvas is being looked
    // at rather than always from above.
    options.three_d = this.options.threeD;
    options.yaw = this.options.yaw;
    options.pitch = this.options.pitch;
    options.exaggeration = this.options.exaggeration;
    for (const { key } of LAYERS) {
      // Points are the one layer that costs enough to build on demand; the rest
      // are always built and hidden, so a toggle is a repaint.
      const wanted = key === EXPENSIVE_LAYER ? this.options.drawPoints : true;
      options.set_layer(key, wanted && (!forExport || this._visible.has(key)));
    }
    return options;
  }

  /// Re-colours the scene without rebuilding it.
  ///
  /// A style's *name* comes from the map's tags, never from the palette, so a
  /// scene built under either theme interns the same names in the same order and
  /// every `styleOf` index still points at the right entry. That makes a theme
  /// change a table swap rather than a re-flatten of every vertex, a re-copy of
  /// six arrays across the boundary, and a rebuild of every path and the spatial
  /// index. The length check is the guard: if that ever stops being true, this
  /// falls back to the honest thing.
  _restyle() {
    const options = this._sceneOptions();
    const data = this._handle.build_scene(options);
    options.free();
    const styles = JSON.parse(data.styles_json());
    if (styles.length !== this._geometry.styles.length) {
      data.free();
      this._rebuild({ keepView: true });
      return;
    }
    this._geometry.styles = styles;
    for (const group of this._groups) group.style = styles[group.styleIndex];
    this._sceneBackground = data.background();
    this._highlightColour = data.highlight();
    this.legend = buildLegend(this._geometry);
    this._freeScene();
    this._scene = data;
    this._draw();
  }

  _rebuild({ keepView }) {
    const options = this._sceneOptions();
    const data = this._handle.build_scene(options);
    options.free();
    this._freeScene();
    this._scene = data;

    const styles = JSON.parse(data.styles_json());
    const geometry = {
      coords: data.coords(),
      offsets: data.offsets(),
      styleOf: data.styles(),
      layerOf: data.layers(),
      closed: data.closed(),
      ids: data.ids(),
      bounds: Array.from(data.bounds()),
      centre: [data.centre_x(), data.centre_y()],
      count: data.shape_count(),
      styles,
    };
    this._geometry = geometry;
    this._relief = data.relief();
    this._sceneBackground = data.background();
    this._highlightColour = data.highlight();
    this._groups = buildGroups(geometry);
    this._index = buildIndex(geometry);
    this._byId = buildIdIndex(geometry);
    this._highlightPath = null;
    this._layerCounts = countLayers(geometry);
    this.legend = buildLegend(geometry);
    this._hover = -1;
    this._pinned = null;

    this._applyChromeVisibility();
    if (!keepView) this.fit();
    else this._draw();
  }

  // --- drawing ---------------------------------------------------------------

  _resize() {
    const rect = this.container.getBoundingClientRect();
    this._width = Math.max(1, rect.width);
    this._height = Math.max(1, rect.height);
    this._ratio = Math.min(globalThis.devicePixelRatio || 1, 2);
    this._canvas.width = Math.round(this._width * this._ratio);
    this._canvas.height = Math.round(this._height * this._ratio);
    this._rect = rect;
  }

  /// The canvas rect, without forcing a layout on every pointer event. Scrolling
  /// or a host reflow moves it without resizing it, so it is re-measured when the
  /// cached one no longer agrees with the pointer's own coordinates.
  _canvasRect() {
    return this._rect ?? this._canvas.getBoundingClientRect();
  }

  _draw() {
    if (this._frame || this._destroyed) return;
    this._frame = requestAnimationFrame(() => {
      this._frame = 0;
      if (!this._destroyed) this._render();
    });
  }

  _render() {
    const context = this._context;
    const { scale, tx, ty } = this._view;
    const ratio = this._ratio || 1;

    context.setTransform(1, 0, 0, 1, 0, 0);
    context.clearRect(0, 0, this._canvas.width, this._canvas.height);
    const background = this.backgroundColor;
    if (background !== 'transparent') {
      context.fillStyle = background;
      context.fillRect(0, 0, this._canvas.width, this._canvas.height);
    }
    if (!this._groups.length) {
      this._updateScalebar();
      return;
    }

    // The y flip lives in the transform, so nothing downstream has to remember it.
    context.setTransform(scale * ratio, 0, 0, -scale * ratio, tx * ratio, ty * ratio);
    context.lineJoin = 'round';
    context.lineCap = 'round';

    for (const group of this._groups) {
      if (!this._visible.has(group.layerKey)) continue;
      const style = group.style;
      // Which detail survives which zoom is a property of the style, decided in
      // Rust beside the dash pattern it overrides — so the SVG export drops the
      // same things this does, and nothing here knows what a `line_thin` is.
      if (style.hideBelowScale > 0 && scale < style.hideBelowScale) continue;
      if (style.fill) {
        context.globalAlpha = style.fillOpacity;
        context.fillStyle = style.fill;
        context.fill(group.path);
      }
      if (style.stroke) {
        context.globalAlpha = style.strokeOpacity;
        context.strokeStyle = style.stroke;
        // Widths and dashes are specified in pixels; dividing by the scale is what
        // keeps them that way through the transform.
        context.lineWidth = style.strokeWidth / scale;
        const dash = style.dash && scale >= style.solidBelowScale;
        context.setLineDash(dash ? style.dash.map((value) => value / scale) : []);
        context.stroke(group.path);
      }
    }
    context.setLineDash([]);
    context.globalAlpha = 1;

    this._drawEmphasis();
    context.setTransform(1, 0, 0, 1, 0, 0);
    this._updateScalebar();
  }

  _drawEmphasis() {
    const focused = this._pinned ?? this._hover;
    if (!this._highlightPath && focused < 0) return;

    const context = this._context;
    context.globalAlpha = 1;
    // The colour is a decision about what the map looks like, so it comes from
    // the palette with every other one rather than from the rasteriser.
    context.strokeStyle = this._highlightColour ?? '#ffd74a';
    context.lineWidth = 3 / this._view.scale;
    context.setLineDash([]);
    if (this._highlightPath) context.stroke(this._highlightPath);
    if (focused >= 0) {
      const path = new Path2D();
      appendShapeTo(path, this._geometry, focused);
      context.stroke(path);
    }
  }

  /// The bar measures across the screen, and the projection never foreshortens that
  /// axis — a tilt compresses the map along screen y and leaves screen x as it found
  /// it. So the scale bar is telling the truth in the 3D view too, which is the one
  /// place it would have been easy to leave quietly lying.
  _updateScalebar() {
    if (!this.options.scalebar || !this._geometry) {
      this._scalebar.hidden = true;
      return;
    }
    this._scalebar.hidden = false;
    const target = Math.min(160, this._width * 0.3);
    const metres = niceRound(target / this._view.scale);
    this._scalebarBar.style.width = `${metres * this._view.scale}px`;
    this._scalebarLabel.textContent =
      metres >= 1000 ? `${(metres / 1000).toLocaleString()} km` : `${metres.toLocaleString()} m`;
  }

  _applyChrome() {
    for (const [name, value] of Object.entries(CHROME[this.theme])) {
      if (name.startsWith('--')) this._host.style.setProperty(name, value);
    }
  }

  // --- interaction -----------------------------------------------------------

  _installPointerHandlers() {
    const canvas = this._canvas;
    const pointers = new Map();
    let pinch = null;
    let dragged = false;

    canvas.addEventListener('pointerdown', (event) => {
      if (!this.options.interactive) return;
      canvas.setPointerCapture(event.pointerId);
      pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
      dragged = false;
      if (pointers.size === 2) pinch = this._pinchState(pointers);
      canvas.classList.add('panning');
      this._tooltip.hidden = true;
    });

    canvas.addEventListener('pointermove', (event) => {
      if (!this.options.interactive) return;
      const previous = pointers.get(event.pointerId);
      if (!previous) {
        this._hoverAt(event);
        return;
      }
      const dx = event.clientX - previous.x;
      const dy = event.clientY - previous.y;
      pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
      if (Math.abs(dx) + Math.abs(dy) > 1) dragged = true;

      if (pointers.size >= 2) {
        const now = this._pinchState(pointers);
        if (pinch && now.distance > 0 && pinch.distance > 0) {
          this._zoomAt(now.x, now.y, now.distance / pinch.distance);
          this._view.tx += now.x - pinch.x;
          this._view.ty += now.y - pinch.y;
          this._draw();
        }
        pinch = now;
        return;
      }
      this._view.tx += dx;
      this._view.ty += dy;
      this._draw();
      this._emitViewChange();
    });

    const release = (event) => {
      const wasDown = pointers.delete(event.pointerId);
      if (pointers.size < 2) pinch = null;
      if (pointers.size === 0) canvas.classList.remove('panning');
      if (!this.options.interactive) return;
      if (wasDown && !dragged) {
        // A click that did not pan is a selection.
        this._hoverAt(event);
        this._pinned = this._hover >= 0 ? this._hover : null;
        this._emit('select', this._describeShape(this._pinned));
        this._draw();
      }
    };
    canvas.addEventListener('pointerup', release);
    canvas.addEventListener('pointercancel', release);
    canvas.addEventListener('pointerleave', () => {
      if (this._hover !== -1) {
        this._hover = -1;
        this._emit('hover', null);
        this._draw();
      }
      this._tooltip.hidden = true;
    });

    canvas.addEventListener(
      'wheel',
      (event) => {
        if (!this.options.interactive) return;
        event.preventDefault();
        const rect = this._canvasRect();
        // Trackpads report pixels and mice report lines; normalising keeps one
        // notch of a wheel worth about the same as a short two-finger swipe.
        const step = event.deltaMode === 1 ? event.deltaY * 16 : event.deltaY;
        this._zoomAt(event.clientX - rect.left, event.clientY - rect.top, Math.exp(-step * 0.0016));
        this._draw();
        this._emitViewChange();
      },
      { passive: false },
    );
  }

  _pinchState(pointers) {
    const [a, b] = [...pointers.values()];
    const rect = this._canvasRect();
    return {
      x: (a.x + b.x) / 2 - rect.left,
      y: (a.y + b.y) / 2 - rect.top,
      distance: Math.hypot(a.x - b.x, a.y - b.y),
    };
  }

  /// Scales about a screen point, so whatever is under the cursor stays under it.
  _zoomAt(screenX, screenY, factor) {
    const before = this._view.scale;
    const after = clamp(before * factor, 1e-4, 5000);
    if (after === before) return;
    this._view.tx = screenX - ((screenX - this._view.tx) * after) / before;
    this._view.ty = screenY - ((screenY - this._view.ty) * after) / before;
    this._view.scale = after;
  }

  _hoverAt(event) {
    if (!this._geometry) return;
    const rect = this._canvasRect();
    const screenX = event.clientX - rect.left;
    const screenY = event.clientY - rect.top;
    const worldX = (screenX - this._view.tx) / this._view.scale;
    const worldY = (this._view.ty - screenY) / this._view.scale;

    const found = this._pick(worldX, worldY, 6 / this._view.scale);
    if (found !== this._hover) {
      this._hover = found;
      // One crossing of the wasm boundary for the label, shared by the event and
      // the tooltip; it is the same string.
      this._hoverLabel = found >= 0 ? this._scene.label(found) : '';
      this._emit('hover', found >= 0 ? this._describeShape(found, this._hoverLabel) : null);
      this._draw();
    }
    if (found < 0 || !this.options.tooltip) {
      this._tooltip.hidden = true;
      return;
    }
    this._showTooltip(this._hoverLabel, screenX, screenY);
  }

  _showTooltip(text, x, y) {
    if (!text) {
      this._tooltip.hidden = true;
      return;
    }
    this._tooltip.textContent = text;
    this._tooltip.hidden = false;
    const width = this._tooltip.offsetWidth;
    const height = this._tooltip.offsetHeight;
    this._tooltip.style.left = `${clamp(x + 14, 4, Math.max(4, this._width - width - 4))}px`;
    this._tooltip.style.top = `${clamp(y + 14, 4, Math.max(4, this._height - height - 4))}px`;
  }

  _describeShape(shape, label) {
    if (shape === null || shape === undefined || shape < 0 || !this._geometry) return null;
    return {
      id: this._geometry.ids[shape],
      label: label ?? this._scene.label(shape),
      layer: LAYERS[this._geometry.layerOf[shape]].key,
    };
  }

  _findShape(id) {
    return this._byId?.get(id)?.[0] ?? -1;
  }

  _pick(x, y, tolerance) {
    const index = this._index;
    const geometry = this._geometry;
    if (!index || !geometry) return -1;

    const column = clamp(Math.floor((x - index.originX) / index.cell), 0, index.cols - 1);
    const row = clamp(Math.floor((y - index.originY) / index.cell), 0, index.rows - 1);
    const candidates = [];
    for (let dc = -1; dc <= 1; dc += 1) {
      for (let dr = -1; dr <= 1; dr += 1) {
        if (column + dc < 0 || column + dc >= index.cols) continue;
        if (row + dr < 0 || row + dr >= index.rows) continue;
        const bucket = index.buckets.get((row + dr) * index.cols + (column + dc));
        if (bucket) candidates.push(...bucket);
      }
    }
    candidates.push(...index.large);
    if (!candidates.length) return -1;

    // Topmost wins, which is what "the thing you are pointing at" means.
    const z = (shape) => geometry.styles[geometry.styleOf[shape]].z;
    candidates.sort((a, b) => z(b) - z(a));
    for (const shape of candidates) {
      if (!this._visible.has(LAYERS[geometry.layerOf[shape]].key)) continue;
      if (hits(geometry, shape, x, y, tolerance)) return shape;
    }
    return -1;
  }

  // --- events ----------------------------------------------------------------

  _emit(type, detail) {
    this.dispatchEvent(new CustomEvent(type, { detail }));
  }

  _emitViewChange() {
    if (this._geometry) this._emit('viewchange', this.getView());
  }

  _fail(text) {
    this._emit('error', { message: text });
  }
}

// --- geometry helpers, shared by every viewer on the page --------------------

/// One `Path2D` per (layer, style) pair, in painter's order.
function buildGroups(geometry) {
  const byKey = new Map();
  for (let shape = 0; shape < geometry.count; shape += 1) {
    const style = geometry.styleOf[shape];
    const layer = geometry.layerOf[shape];
    const key = layer * 4096 + style;
    let group = byKey.get(key);
    if (group === undefined) {
      group = {
        layerKey: LAYERS[layer].key,
        styleIndex: style,
        style: geometry.styles[style],
        path: new Path2D(),
      };
      byKey.set(key, group);
    }
    appendShape(
      group.path,
      geometry.coords,
      geometry.offsets[shape],
      geometry.offsets[shape + 1],
      geometry.closed[shape] === 1,
    );
  }
  return [...byKey.values()].sort((a, b) => a.style.z - b.style.z);
}

/// Adds one shape of a scene to a path. The scene-indexed form, which is what
/// every caller but `buildGroups` actually has.
function appendShapeTo(path, geometry, shape) {
  appendShape(
    path,
    geometry.coords,
    geometry.offsets[shape],
    geometry.offsets[shape + 1],
    geometry.closed[shape] === 1,
  );
}

/// A shape's bounding box, as `[minX, minY, maxX, maxY]`.
function shapeBounds(geometry, shape) {
  const { coords, offsets } = geometry;
  let lowX = Infinity;
  let lowY = Infinity;
  let highX = -Infinity;
  let highY = -Infinity;
  for (let vertex = offsets[shape]; vertex < offsets[shape + 1]; vertex += 1) {
    const x = coords[vertex * 2];
    const y = coords[vertex * 2 + 1];
    if (x < lowX) lowX = x;
    if (x > highX) highX = x;
    if (y < lowY) lowY = y;
    if (y > highY) highY = y;
  }
  return [lowX, lowY, highX, highY];
}

/// Primitive id to the shapes drawn from it — a lanelet is a fill, a centerline
/// and an arrowhead every 25 metres, all carrying its id.
function buildIdIndex(geometry) {
  const byId = new Map();
  for (let shape = 0; shape < geometry.count; shape += 1) {
    const id = geometry.ids[shape];
    const shapes = byId.get(id);
    if (shapes) shapes.push(shape);
    else byId.set(id, [shape]);
  }
  return byId;
}

function appendShape(path, coords, start, end, closed) {
  if (end <= start) return;
  path.moveTo(coords[start * 2], coords[start * 2 + 1]);
  if (end - start === 1) {
    // A lone map point. A round-capped hair of a segment draws as a dot whose size
    // comes from the line width, and so stays constant on screen.
    path.lineTo(coords[start * 2] + 1e-4, coords[start * 2 + 1]);
    return;
  }
  for (let vertex = start + 1; vertex < end; vertex += 1) {
    path.lineTo(coords[vertex * 2], coords[vertex * 2 + 1]);
  }
  if (closed) path.closePath();
}

function countLayers(geometry) {
  const counts = new Map();
  for (let shape = 0; shape < geometry.count; shape += 1) {
    const key = LAYERS[geometry.layerOf[shape]].key;
    counts.set(key, (counts.get(key) || 0) + 1);
  }
  return counts;
}

/// The styles actually used, deduplicated by label and in painter's order.
function buildLegend(geometry) {
  const used = new Set(geometry.styleOf);
  const seen = new Set();
  const items = [];
  for (const index of [...used].sort((a, b) => geometry.styles[a].z - geometry.styles[b].z)) {
    const style = geometry.styles[index];
    if (!style.inLegend || seen.has(style.label)) continue;
    seen.add(style.label);
    items.push(style);
  }
  return items;
}

/// A uniform grid over shape bounding boxes.
///
/// Shapes that would span an unreasonable number of cells — a boundary running the
/// length of the map — go into an always-checked list instead of being smeared
/// across the grid, which keeps the buckets small without losing anything.
function buildIndex(geometry) {
  const [minX, minY, maxX, maxY] = geometry.bounds;
  const spanX = Math.max(maxX - minX, 1e-6);
  const spanY = Math.max(maxY - minY, 1e-6);
  const cell = Math.max(Math.max(spanX, spanY) / 220, 0.5);
  const cols = Math.max(1, Math.ceil(spanX / cell));
  const rows = Math.max(1, Math.ceil(spanY / cell));
  const buckets = new Map();
  const large = [];
  // Scene coordinates are centred, so the grid's own origin is the negative half
  // span rather than the map's minimum.
  const originX = -spanX / 2;
  const originY = -spanY / 2;

  for (let shape = 0; shape < geometry.count; shape += 1) {
    if (geometry.offsets[shape + 1] <= geometry.offsets[shape]) continue;
    const [lowX, lowY, highX, highY] = shapeBounds(geometry, shape);
    const columnFrom = clamp(Math.floor((lowX - originX) / cell), 0, cols - 1);
    const columnTo = clamp(Math.floor((highX - originX) / cell), 0, cols - 1);
    const rowFrom = clamp(Math.floor((lowY - originY) / cell), 0, rows - 1);
    const rowTo = clamp(Math.floor((highY - originY) / cell), 0, rows - 1);
    if ((columnTo - columnFrom + 1) * (rowTo - rowFrom + 1) > 256) {
      large.push(shape);
      continue;
    }
    for (let column = columnFrom; column <= columnTo; column += 1) {
      for (let row = rowFrom; row <= rowTo; row += 1) {
        const key = row * cols + column;
        const bucket = buckets.get(key);
        if (bucket) bucket.push(shape);
        else buckets.set(key, [shape]);
      }
    }
  }
  return { cell, cols, rows, originX, originY, buckets, large };
}

function hits(geometry, shape, x, y, tolerance) {
  const start = geometry.offsets[shape];
  const end = geometry.offsets[shape + 1];
  const coords = geometry.coords;
  if (end - start === 1) {
    return Math.hypot(coords[start * 2] - x, coords[start * 2 + 1] - y) <= tolerance;
  }
  if (geometry.closed[shape] === 1 && insidePolygon(coords, start, end, x, y)) return true;
  for (let vertex = start; vertex + 1 < end; vertex += 1) {
    if (
      distanceToSegment(
        x,
        y,
        coords[vertex * 2],
        coords[vertex * 2 + 1],
        coords[vertex * 2 + 2],
        coords[vertex * 2 + 3],
      ) <= tolerance
    ) {
      return true;
    }
  }
  return false;
}

function insidePolygon(coords, start, end, x, y) {
  let inside = false;
  for (let i = start, j = end - 1; i < end; j = i, i += 1) {
    const xi = coords[i * 2];
    const yi = coords[i * 2 + 1];
    const xj = coords[j * 2];
    const yj = coords[j * 2 + 1];
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

function distanceToSegment(px, py, ax, ay, bx, by) {
  const dx = bx - ax;
  const dy = by - ay;
  const lengthSquared = dx * dx + dy * dy;
  if (lengthSquared === 0) return Math.hypot(px - ax, py - ay);
  const t = clamp(((px - ax) * dx + (py - ay) * dy) / lengthSquared, 0, 1);
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
}

/// The largest of 1, 2 or 5 times a power of ten that fits in `value`.
function niceRound(value) {
  if (!isFinite(value) || value <= 0) return 1;
  const magnitude = Math.pow(10, Math.floor(Math.log10(value)));
  for (const step of [5, 2, 1]) {
    if (magnitude * step <= value) return magnitude * step;
  }
  return magnitude;
}

function clamp(value, low, high) {
  return value < low ? low : value > high ? high : value;
}

function message(error) {
  if (!error) return 'unknown error';
  return error.message || String(error);
}

function basename(path) {
  return String(path).split(/[\\/]/).pop().replace(/\.osm$/i, '') || 'map';
}

// --- the custom element ------------------------------------------------------

/**
 * `<lanelet2-viewer src="map.osm" theme="auto" layers="lanelet_fill,bound">`
 *
 * Attributes mirror the constructor options and are live: setting `theme` on the
 * element changes the theme. The underlying [`LaneletViewer`] is on `.viewer`, so a
 * host that needs the full API is one property away.
 */
export class Lanelet2ViewerElement extends HTMLElement {
  static observedAttributes = ['src', 'theme', 'layers', 'coordinates', 'background', 'controls', 'tooltip', 'scalebar', 'interactive', 'points', 'three-d', 'yaw', 'pitch', 'exaggeration'];

  connectedCallback() {
    if (this.viewer) return;
    const { enabled, ...angles } = this._camera();
    this.viewer = new LaneletViewer(this, {
      theme: this.getAttribute('theme') || 'dark',
      coordinates: this.getAttribute('coordinates') || 'auto',
      layers: this.hasAttribute('layers') ? splitList(this.getAttribute('layers')) : null,
      background: this.getAttribute('background'),
      controls: flag(this, 'controls', true),
      tooltip: flag(this, 'tooltip', true),
      scalebar: flag(this, 'scalebar', true),
      interactive: flag(this, 'interactive', true),
      drawPoints: flag(this, 'points', false),
      threeD: enabled,
      // Only the angles actually given: an absent attribute must leave the
      // viewer's own default alone rather than overwrite it with `undefined`.
      ...angles,
      wasmUrl: this.getAttribute('wasm') || undefined,
    });
    // Re-dispatch on the element, so `addEventListener` on the tag works.
    for (const type of ['loadstart', 'load', 'error', 'hover', 'select', 'viewchange', 'view3dchange']) {
      this.viewer.addEventListener(type, (event) => {
        this.dispatchEvent(new CustomEvent(type, { detail: event.detail, bubbles: false }));
      });
    }
    if (this.hasAttribute('src')) this.viewer.loadUrl(this.getAttribute('src'));
  }

  disconnectedCallback() {
    this.viewer?.destroy();
    this.viewer = null;
  }

  attributeChangedCallback(name, previous, value) {
    const viewer = this.viewer;
    if (!viewer || previous === value) return;
    switch (name) {
      case 'src':
        if (value) viewer.loadUrl(value);
        else viewer.clear();
        break;
      case 'theme':
        viewer.setTheme(value || 'dark');
        break;
      case 'layers':
        viewer.setLayers(value === null ? defaultLayers() : splitList(value));
        break;
      case 'coordinates':
        viewer.setCoordinates(value || 'auto');
        break;
      case 'background':
        viewer.setBackground(value);
        break;
      case 'points':
        viewer.setDrawPoints(flag(this, 'points', false));
        break;
      case 'interactive':
        viewer.setInteractive(flag(this, 'interactive', true));
        break;
      case 'three-d':
      case 'yaw':
      case 'pitch':
      case 'exaggeration':
        viewer.setView3d(this._camera());
        break;
      default:
        viewer.setChrome({ [name]: flag(this, name, true) });
        break;
    }
  }

  /// The camera attributes that are actually present. An absent or unparseable one
  /// is left out entirely rather than reported as `undefined`, so neither the
  /// constructor nor `setView3d` can overwrite a default with nothing.
  _camera() {
    const camera = { enabled: flag(this, 'three-d', false) };
    for (const name of ['yaw', 'pitch', 'exaggeration']) {
      if (!this.hasAttribute(name)) continue;
      const value = Number(this.getAttribute(name));
      if (Number.isFinite(value)) camera[name] = value;
    }
    return camera;
  }

  /** Convenience passthrough, so `element.loadOsm(text)` reads naturally. */
  loadOsm(text, options) {
    return this.viewer.loadOsm(text, options);
  }
}

/// Splits a comma- or space-separated list, as `layers="a,b"` and `?layers=a,b`
/// both use. Exported so the iframe page parses them by the same rule.
export function splitList(value) {
  return String(value ?? '')
    .split(/[\s,]+/)
    .filter(Boolean);
}

/// The truthiness rule shared by attributes and query parameters: absent means
/// the default, and `false`/`0`/`off` are the only ways to say no.
export function readFlag(value, fallback) {
  if (value === null || value === undefined) return fallback;
  return value !== 'false' && value !== '0' && value !== 'off';
}

function flag(element, name, fallback) {
  return readFlag(element.hasAttribute(name) ? element.getAttribute(name) : null, fallback);
}

if (typeof customElements !== 'undefined' && !customElements.get('lanelet2-viewer')) {
  customElements.define('lanelet2-viewer', Lanelet2ViewerElement);
}
