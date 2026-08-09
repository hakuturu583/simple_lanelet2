// The demo's whole job is to be a rasteriser. Everything that decides *what* a
// Lanelet2 map looks like — reading the file, picking the projection, classifying
// primitives by tag, choosing colours, widths and dash patterns, placing the
// direction arrows — happens in Rust, in `ll2-viz`, and arrives here as flat typed
// arrays. See `crates/ll2-wasm/src/lib.rs` for the shape of that hand-over.
//
// The one thing worth knowing about the drawing: shapes are grouped by
// (layer, style) into a `Path2D` each, once per load. A frame then sets the canvas
// transform and issues one fill/stroke per group — a few dozen calls — no matter
// whether the map has a thousand shapes or a million. Line widths are divided by
// the scale so a lane marking stays one pixel wide at every zoom, which is the same
// rule the SVG writer expresses as `vector-effect="non-scaling-stroke"`.

import init, { load_osm, SceneOptions, version } from './pkg/ll2_wasm.js';

const elements = {
  canvas: document.getElementById('canvas'),
  dropzone: document.getElementById('dropzone'),
  fileInput: document.getElementById('file-input'),
  sampleButton: document.getElementById('sample-button'),
  exportButton: document.getElementById('export-button'),
  fitButton: document.getElementById('fit-button'),
  zoomIn: document.getElementById('zoom-in'),
  zoomOut: document.getElementById('zoom-out'),
  facts: document.getElementById('facts'),
  layerToggles: document.getElementById('layer-toggles'),
  layersBlock: document.getElementById('layers-block'),
  optionsBlock: document.getElementById('options-block'),
  legend: document.getElementById('legend'),
  legendBlock: document.getElementById('legend-block'),
  errors: document.getElementById('errors'),
  errorsBlock: document.getElementById('errors-block'),
  errorsCount: document.getElementById('errors-count'),
  themeSelect: document.getElementById('theme-select'),
  coordinatesSelect: document.getElementById('coordinates-select'),
  pointsToggle: document.getElementById('points-toggle'),
  status: document.getElementById('status'),
  statusText: document.getElementById('status-text'),
  tooltip: document.getElementById('tooltip'),
  scalebar: document.getElementById('scalebar'),
  scalebarLine: document.querySelector('#scalebar .scalebar-line'),
  scalebarLabel: document.getElementById('scalebar-label'),
  versionLabel: document.getElementById('version-label'),
};

const context = elements.canvas.getContext('2d', { alpha: false });

const state = {
  handle: null, // the parsed map, kept so scenes can be rebuilt without reparsing
  scene: null, // the flattened arrays for the scene currently drawn
  groups: [], // one Path2D per (layer, style), in painter's order
  index: null, // uniform grid over shape bounding boxes, for hover picking
  visible: new Set(), // layer keys the user has left switched on
  layers: [], // [{key, label}] as the Rust side orders them
  view: { scale: 1, tx: 0, ty: 0 },
  theme: 'dark',
  coordinates: 'auto',
  drawPoints: false,
  fileName: 'map',
  hover: -1,
  frame: 0,
};

/// A traffic lane, in metres. Only ever used to turn the zoom into a statement
/// about how much of the map a viewer can actually make out.
const LANE_WIDTH = 3.5;

const LAYER_DEFAULTS = {
  lanelet_fill: true,
  area: true,
  polygon: true,
  bound: true,
  regulatory: true,
  centerline: false,
  direction: true,
  point: true,
};

// --- boot --------------------------------------------------------------------

async function boot() {
  showStatus('Loading the WebAssembly module…');
  try {
    await init();
  } catch (error) {
    showError(`The WebAssembly module failed to load: ${describe(error)}`);
    return;
  }
  elements.versionLabel.textContent = `simple_lanelet2 v${version()}`;
  hideStatus();
  resizeCanvas();
  wireUp();
}

function wireUp() {
  elements.fileInput.addEventListener('change', () => {
    const file = elements.fileInput.files && elements.fileInput.files[0];
    if (file) openFile(file);
    elements.fileInput.value = '';
  });
  elements.sampleButton.addEventListener('click', loadSample);
  elements.exportButton.addEventListener('click', exportSvg);
  elements.fitButton.addEventListener('click', () => {
    fitToMap();
    draw();
  });
  elements.zoomIn.addEventListener('click', () => zoomBy(1.4));
  elements.zoomOut.addEventListener('click', () => zoomBy(1 / 1.4));

  elements.themeSelect.addEventListener('change', () => {
    state.theme = elements.themeSelect.value;
    document.body.dataset.theme = state.theme;
    if (state.handle) rebuildScene({ keepView: true });
  });
  elements.coordinatesSelect.addEventListener('change', () => {
    state.coordinates = elements.coordinatesSelect.value;
    if (state.lastText) parseText(state.lastText, state.fileName);
  });
  elements.pointsToggle.addEventListener('change', () => {
    state.drawPoints = elements.pointsToggle.checked;
    if (state.handle) rebuildScene({ keepView: true });
  });

  installDragAndDrop();
  installPanZoom();

  window.addEventListener('resize', () => {
    resizeCanvas();
    draw();
  });
  window.addEventListener('keydown', (event) => {
    if (event.key === 'f' || event.key === 'F') {
      fitToMap();
      draw();
    }
  });
}

// --- files -------------------------------------------------------------------

function installDragAndDrop() {
  let depth = 0;
  window.addEventListener('dragenter', (event) => {
    event.preventDefault();
    depth += 1;
    document.body.classList.add('dragging');
  });
  window.addEventListener('dragover', (event) => {
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'copy';
  });
  window.addEventListener('dragleave', (event) => {
    event.preventDefault();
    depth = Math.max(0, depth - 1);
    if (depth === 0) document.body.classList.remove('dragging');
  });
  window.addEventListener('drop', (event) => {
    event.preventDefault();
    depth = 0;
    document.body.classList.remove('dragging');
    const file = event.dataTransfer && event.dataTransfer.files && event.dataTransfer.files[0];
    if (file) openFile(file);
  });
}

async function openFile(file) {
  showStatus(`Reading ${file.name}…`);
  let text;
  try {
    text = await file.text();
  } catch (error) {
    showError(`Could not read ${file.name}: ${describe(error)}`);
    return;
  }
  await parseText(text, file.name.replace(/\.osm$/i, ''));
}

async function loadSample() {
  showStatus('Fetching the example map…');
  try {
    const response = await fetch('./sample/mapping_example.osm');
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    await parseText(await response.text(), 'mapping_example');
  } catch (error) {
    showError(`Could not fetch the example map: ${describe(error)}`);
  }
}

async function parseText(text, name) {
  state.fileName = name;
  state.lastText = text;
  showStatus('Parsing the map…');
  // One frame, so the status actually paints before the parse blocks the thread.
  await nextFrame();

  let handle;
  try {
    handle = load_osm(text, state.coordinates);
  } catch (error) {
    showError(describe(error));
    return;
  }
  if (state.handle) state.handle.free();
  state.handle = handle;

  showFacts(handle);
  showErrors(handle.errors());
  rebuildScene({ keepView: false });
  hideStatus();
  elements.dropzone.hidden = true;
  elements.layersBlock.hidden = false;
  elements.optionsBlock.hidden = false;
}

// --- scene -------------------------------------------------------------------

/// Every layer is built on the Rust side and hidden here, so a toggle is instant.
/// Points are the exception: a city map has millions of them and building shapes
/// for all of them costs real time, so that one really does rebuild.
function buildOptions({ forExport } = {}) {
  const options = new SceneOptions();
  options.theme = state.theme;
  options.lanelet_fill = true;
  options.areas = true;
  options.polygons = true;
  options.bounds = true;
  options.regulatory = true;
  options.centerlines = true;
  options.direction_arrows = true;
  options.points = state.drawPoints;
  if (forExport) {
    options.lanelet_fill = state.visible.has('lanelet_fill');
    options.areas = state.visible.has('area');
    options.polygons = state.visible.has('polygon');
    options.bounds = state.visible.has('bound');
    options.regulatory = state.visible.has('regulatory');
    options.centerlines = state.visible.has('centerline');
    options.direction_arrows = state.visible.has('direction');
    options.points = state.drawPoints && state.visible.has('point');
  }
  return options;
}

function rebuildScene({ keepView }) {
  const options = buildOptions();
  const data = state.handle.build_scene(options);
  options.free();

  if (state.scene) state.scene.free();
  state.scene = data;

  const styles = JSON.parse(data.styles_json());
  const layers = JSON.parse(data.layers_json());
  state.layers = layers;
  if (state.visible.size === 0) {
    for (const layer of layers) {
      if (LAYER_DEFAULTS[layer.key]) state.visible.add(layer.key);
    }
  }

  const scene = {
    coords: data.coords(),
    offsets: data.offsets(),
    styleOf: data.styles(),
    layerOf: data.layers(),
    closed: data.closed(),
    ids: data.ids(),
    bounds: data.bounds(),
    count: data.shape_count(),
    styles,
    layers,
  };
  state.geometry = scene;
  state.groups = buildGroups(scene);
  state.index = buildIndex(scene);
  state.hover = -1;

  // On `body`, not on the root: the light-theme rule redefines `--canvas` on
  // `body`, and that would win over an inline value set further up the tree.
  state.background = data.background();
  document.body.style.setProperty('--canvas', state.background);
  showLayerToggles(scene);
  showLegend(scene);
  if (!keepView) fitToMap();
  draw();
}

/// One `Path2D` per (layer, style) pair, in painter's order.
function buildGroups(scene) {
  const byKey = new Map();
  for (let shape = 0; shape < scene.count; shape += 1) {
    const style = scene.styleOf[shape];
    const layer = scene.layerOf[shape];
    const key = layer * 4096 + style;
    let group = byKey.get(key);
    if (group === undefined) {
      group = {
        layerKey: scene.layers[layer].key,
        style: scene.styles[style],
        path: new Path2D(),
      };
      byKey.set(key, group);
    }
    const start = scene.offsets[shape];
    const end = scene.offsets[shape + 1];
    appendShape(group.path, scene.coords, start, end, scene.closed[shape] === 1);
  }
  return [...byKey.values()].sort((a, b) => a.style.z - b.style.z);
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

// --- drawing -----------------------------------------------------------------

function resizeCanvas() {
  const ratio = Math.min(window.devicePixelRatio || 1, 2);
  const rect = elements.canvas.getBoundingClientRect();
  elements.canvas.width = Math.max(1, Math.round(rect.width * ratio));
  elements.canvas.height = Math.max(1, Math.round(rect.height * ratio));
  state.ratio = ratio;
  state.width = rect.width;
  state.height = rect.height;
}

function fitToMap() {
  if (!state.geometry) return;
  const [minX, minY, maxX, maxY] = state.geometry.bounds;
  const spanX = Math.max(maxX - minX, 1e-6);
  const spanY = Math.max(maxY - minY, 1e-6);
  state.view.scale = Math.min(state.width / spanX, state.height / spanY) * 0.94;
  // Scene coordinates are already relative to the map's centre, so centring the
  // map is exactly centring the world origin.
  state.view.tx = state.width / 2;
  state.view.ty = state.height / 2;
}

function draw() {
  if (state.frame) return;
  state.frame = requestAnimationFrame(() => {
    state.frame = 0;
    render();
  });
}

function render() {
  const { scale, tx, ty } = state.view;
  const ratio = state.ratio || 1;

  context.setTransform(1, 0, 0, 1, 0, 0);
  context.fillStyle = state.background || '#11141a';
  context.fillRect(0, 0, elements.canvas.width, elements.canvas.height);
  if (!state.groups.length) {
    updateScalebar();
    return;
  }

  // The y flip lives in the transform, so nothing downstream has to remember it.
  context.setTransform(scale * ratio, 0, 0, -scale * ratio, tx * ratio, ty * ratio);
  context.lineJoin = 'round';
  context.lineCap = 'round';

  // Two pieces of detail are dropped when a lane is only a few pixels across.
  // Both are honest: at that zoom a dash pattern is a grey smear over lines that
  // already overlap, and an arrowhead is a speck. Both are also, measurably, most
  // of the frame — dashing a city's worth of lane markings costs more than every
  // fill and solid stroke in the map put together.
  const laneWidthInPixels = scale * LANE_WIDTH;
  const dashesAreLegible = laneWidthInPixels >= 5;
  const arrowsAreLegible = laneWidthInPixels >= 4;

  for (const group of state.groups) {
    if (!state.visible.has(group.layerKey)) continue;
    if (group.layerKey === 'direction' && !arrowsAreLegible) continue;
    const style = group.style;
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
      const dash = style.dash && dashesAreLegible;
      context.setLineDash(dash ? style.dash.map((value) => value / scale) : []);
      context.stroke(group.path);
    }
  }
  context.setLineDash([]);
  context.globalAlpha = 1;

  drawHighlight();
  context.setTransform(1, 0, 0, 1, 0, 0);
  updateScalebar();
}

function drawHighlight() {
  if (state.hover < 0 || !state.geometry) return;
  const scene = state.geometry;
  const path = new Path2D();
  appendShape(
    path,
    scene.coords,
    scene.offsets[state.hover],
    scene.offsets[state.hover + 1],
    scene.closed[state.hover] === 1,
  );
  context.globalAlpha = 1;
  context.strokeStyle = state.theme === 'light' ? '#d98b00' : '#ffd74a';
  context.lineWidth = 3 / state.view.scale;
  context.setLineDash([]);
  context.stroke(path);
}

function updateScalebar() {
  if (!state.geometry) {
    elements.scalebar.hidden = true;
    return;
  }
  elements.scalebar.hidden = false;
  const target = Math.min(180, state.width * 0.25);
  const metres = niceRound(target / state.view.scale);
  elements.scalebarLine.style.width = `${metres * state.view.scale}px`;
  elements.scalebarLabel.textContent =
    metres >= 1000 ? `${(metres / 1000).toLocaleString()} km` : `${metres.toLocaleString()} m`;
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

// --- interaction -------------------------------------------------------------

function installPanZoom() {
  const canvas = elements.canvas;
  const pointers = new Map();
  let pinch = null;
  let dragged = false;

  canvas.addEventListener('pointerdown', (event) => {
    canvas.setPointerCapture(event.pointerId);
    pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
    dragged = false;
    if (pointers.size === 2) pinch = pinchState(pointers);
    canvas.classList.add('panning');
    hideTooltip();
  });

  canvas.addEventListener('pointermove', (event) => {
    const previous = pointers.get(event.pointerId);
    if (!previous) {
      hover(event);
      return;
    }
    const dx = event.clientX - previous.x;
    const dy = event.clientY - previous.y;
    pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
    if (Math.abs(dx) + Math.abs(dy) > 1) dragged = true;

    if (pointers.size >= 2) {
      const now = pinchState(pointers);
      if (pinch && now.distance > 0 && pinch.distance > 0) {
        zoomAt(now.x, now.y, now.distance / pinch.distance);
        state.view.tx += now.x - pinch.x;
        state.view.ty += now.y - pinch.y;
        draw();
      }
      pinch = now;
      return;
    }
    state.view.tx += dx;
    state.view.ty += dy;
    draw();
  });

  const release = (event) => {
    pointers.delete(event.pointerId);
    if (pointers.size < 2) pinch = null;
    if (pointers.size === 0) canvas.classList.remove('panning');
    if (!dragged) hover(event);
  };
  canvas.addEventListener('pointerup', release);
  canvas.addEventListener('pointercancel', release);
  canvas.addEventListener('pointerleave', () => {
    if (state.hover !== -1) {
      state.hover = -1;
      draw();
    }
    hideTooltip();
  });

  canvas.addEventListener(
    'wheel',
    (event) => {
      event.preventDefault();
      const rect = canvas.getBoundingClientRect();
      // Trackpads report pixels and mice report lines; normalising keeps one
      // notch of a wheel worth about the same as a short two-finger swipe.
      const step = event.deltaMode === 1 ? event.deltaY * 16 : event.deltaY;
      zoomAt(event.clientX - rect.left, event.clientY - rect.top, Math.exp(-step * 0.0016));
      draw();
    },
    { passive: false },
  );
}

function pinchState(pointers) {
  const [a, b] = [...pointers.values()];
  const rect = elements.canvas.getBoundingClientRect();
  return {
    x: (a.x + b.x) / 2 - rect.left,
    y: (a.y + b.y) / 2 - rect.top,
    distance: Math.hypot(a.x - b.x, a.y - b.y),
  };
}

function zoomBy(factor) {
  zoomAt(state.width / 2, state.height / 2, factor);
  draw();
}

/// Scales about a screen point, so whatever is under the cursor stays under it.
function zoomAt(screenX, screenY, factor) {
  const before = state.view.scale;
  const after = clamp(before * factor, 1e-4, 5000);
  if (after === before) return;
  state.view.tx = screenX - ((screenX - state.view.tx) * after) / before;
  state.view.ty = screenY - ((screenY - state.view.ty) * after) / before;
  state.view.scale = after;
}

function hover(event) {
  if (!state.geometry) return;
  const rect = elements.canvas.getBoundingClientRect();
  const screenX = event.clientX - rect.left;
  const screenY = event.clientY - rect.top;
  const worldX = (screenX - state.view.tx) / state.view.scale;
  const worldY = (state.view.ty - screenY) / state.view.scale;

  const found = pick(worldX, worldY, 6 / state.view.scale);
  if (found !== state.hover) {
    state.hover = found;
    draw();
  }
  if (found < 0) {
    hideTooltip();
    return;
  }
  showTooltip(state.scene.label(found), screenX, screenY);
}

// --- picking -----------------------------------------------------------------

/// A uniform grid over shape bounding boxes.
///
/// Shapes that would span an unreasonable number of cells — a boundary running the
/// length of the map — go into an always-checked list instead of being smeared
/// across the grid, which keeps the buckets small without losing anything.
function buildIndex(scene) {
  const [minX, minY, maxX, maxY] = scene.bounds;
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

  for (let shape = 0; shape < scene.count; shape += 1) {
    const start = scene.offsets[shape];
    const end = scene.offsets[shape + 1];
    if (end <= start) continue;
    let lowX = Infinity;
    let lowY = Infinity;
    let highX = -Infinity;
    let highY = -Infinity;
    for (let vertex = start; vertex < end; vertex += 1) {
      const x = scene.coords[vertex * 2];
      const y = scene.coords[vertex * 2 + 1];
      if (x < lowX) lowX = x;
      if (x > highX) highX = x;
      if (y < lowY) lowY = y;
      if (y > highY) highY = y;
    }
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

function pick(x, y, tolerance) {
  const index = state.index;
  const scene = state.geometry;
  if (!index || !scene) return -1;

  const column = clamp(Math.floor((x - index.originX) / index.cell), 0, index.cols - 1);
  const row = clamp(Math.floor((y - index.originY) / index.cell), 0, index.rows - 1);
  const candidates = [];
  for (let dc = -1; dc <= 1; dc += 1) {
    for (let dr = -1; dr <= 1; dr += 1) {
      const bucket = index.buckets.get((row + dr) * index.cols + (column + dc));
      if (bucket) candidates.push(...bucket);
    }
  }
  candidates.push(...index.large);
  if (!candidates.length) return -1;

  // Topmost wins, which is what "the thing you are pointing at" means.
  candidates.sort((a, b) => scene.styles[scene.styleOf[b]].z - scene.styles[scene.styleOf[a]].z);
  for (const shape of candidates) {
    if (!state.visible.has(scene.layers[scene.layerOf[shape]].key)) continue;
    if (hits(scene, shape, x, y, tolerance)) return shape;
  }
  return -1;
}

function hits(scene, shape, x, y, tolerance) {
  const start = scene.offsets[shape];
  const end = scene.offsets[shape + 1];
  const coords = scene.coords;
  if (end - start === 1) {
    return Math.hypot(coords[start * 2] - x, coords[start * 2 + 1] - y) <= tolerance;
  }
  if (scene.closed[shape] === 1 && insidePolygon(coords, start, end, x, y)) return true;
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
  let t = ((px - ax) * dx + (py - ay) * dy) / lengthSquared;
  t = clamp(t, 0, 1);
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
}

// --- side panel --------------------------------------------------------------

function showFacts(handle) {
  const stats = JSON.parse(handle.stats_json());
  const rows = [
    ['Lanelets', stats.lanelets],
    ['Linestrings', stats.lineStrings],
    ['Polygons', stats.polygons],
    ['Areas', stats.areas],
    ['Regulatory elements', stats.regulatoryElements],
    ['Points', stats.points],
    [
      'Coordinates',
      handle.coordinate_source() === 'local_xy' ? 'local_x / local_y' : handle.projection().toUpperCase(),
    ],
  ];
  if (handle.coordinate_source() !== 'local_xy') {
    rows.push(['Origin', `${handle.origin_lat().toFixed(5)}, ${handle.origin_lon().toFixed(5)}`]);
  }
  elements.facts.innerHTML = rows
    .map(
      ([label, value]) =>
        `<dt>${escapeHtml(label)}</dt><dd>${escapeHtml(
          typeof value === 'number' ? value.toLocaleString() : String(value),
        )}</dd>`,
    )
    .join('');
  elements.facts.hidden = false;
}

function showLayerToggles(scene) {
  const counts = new Map();
  for (let shape = 0; shape < scene.count; shape += 1) {
    const key = scene.layers[scene.layerOf[shape]].key;
    counts.set(key, (counts.get(key) || 0) + 1);
  }
  elements.layerToggles.innerHTML = '';
  for (const layer of scene.layers) {
    const count = counts.get(layer.key) || 0;
    const label = document.createElement('label');
    label.className = count ? 'toggle' : 'toggle empty';
    const input = document.createElement('input');
    input.type = 'checkbox';
    input.checked = state.visible.has(layer.key);
    input.disabled = count === 0;
    input.addEventListener('change', () => {
      if (input.checked) state.visible.add(layer.key);
      else state.visible.delete(layer.key);
      state.hover = -1;
      hideTooltip();
      draw();
    });
    const text = document.createElement('span');
    text.textContent = layer.label;
    const number = document.createElement('span');
    number.className = 'count';
    number.textContent = count ? count.toLocaleString() : '—';
    label.append(input, text, number);
    elements.layerToggles.append(label);
  }
}

function showLegend(scene) {
  const used = new Set(scene.styleOf);
  const seen = new Set();
  const items = [];
  for (const styleIndex of [...used].sort((a, b) => scene.styles[a].z - scene.styles[b].z)) {
    const style = scene.styles[styleIndex];
    if (!style.inLegend || seen.has(style.label)) continue;
    seen.add(style.label);
    items.push(style);
  }
  elements.legend.innerHTML = items
    .map((style) => {
      const background = style.fill
        ? `background:${style.fill};opacity:${Math.max(style.fillOpacity, 0.35)}`
        : `background:linear-gradient(${style.stroke},${style.stroke}) center/100% ${Math.max(
            2,
            style.strokeWidth,
          )}px no-repeat`;
      return `<li><span class="swatch" style="${background}"></span>${escapeHtml(style.label)}</li>`;
    })
    .join('');
  elements.legendBlock.hidden = items.length === 0;
}

function showErrors(errors) {
  if (!errors.length) {
    elements.errorsBlock.hidden = true;
    return;
  }
  // The first line is the header `loadRobust` puts on the list; the rest are the
  // individual problems, so the count is one less than the number of lines.
  elements.errorsCount.textContent = `(${errors.length - 1})`;
  elements.errors.textContent = errors.join('\n');
  elements.errorsBlock.hidden = false;
}

// --- export ------------------------------------------------------------------

function exportSvg() {
  if (!state.handle) return;
  const options = buildOptions({ forExport: true });
  const svg = state.handle.to_svg(options, 1920, 1200);
  options.free();
  const blob = new Blob([svg], { type: 'image/svg+xml' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = `${state.fileName || 'map'}.svg`;
  link.click();
  URL.revokeObjectURL(url);
}

// --- odds and ends -----------------------------------------------------------

function showStatus(text) {
  elements.status.classList.remove('error');
  elements.statusText.textContent = text;
  elements.status.hidden = false;
}

function showError(text) {
  elements.status.classList.add('error');
  elements.statusText.textContent = text;
  elements.status.hidden = false;
}

function hideStatus() {
  elements.status.hidden = true;
}

function showTooltip(text, x, y) {
  if (!text) {
    hideTooltip();
    return;
  }
  elements.tooltip.textContent = text;
  elements.tooltip.hidden = false;
  const width = elements.tooltip.offsetWidth;
  const height = elements.tooltip.offsetHeight;
  elements.tooltip.style.left = `${clamp(x + 14, 4, Math.max(4, state.width - width - 4))}px`;
  elements.tooltip.style.top = `${clamp(y + 14, 4, Math.max(4, state.height - height - 4))}px`;
}

function hideTooltip() {
  elements.tooltip.hidden = true;
}

function nextFrame() {
  return new Promise((resolve) => requestAnimationFrame(() => setTimeout(resolve, 0)));
}

function clamp(value, low, high) {
  return value < low ? low : value > high ? high : value;
}

function describe(error) {
  if (!error) return 'unknown error';
  return error.message || String(error);
}

function escapeHtml(text) {
  return String(text).replace(
    /[&<>"']/g,
    (character) =>
      ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[character],
  );
}

boot();
