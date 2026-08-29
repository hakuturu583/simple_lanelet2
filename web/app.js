// The demo page: a sidebar around a `LaneletViewer`.
//
// Everything that draws a map lives in `viewer.js` and everything that decides what
// a map looks like lives in Rust. What is left here is a file picker, a drop
// target, and some panels that read the viewer's API — which is also the point:
// if this file needs nothing privileged, neither does a Foxglove panel or a wandb
// HTML block. See EMBEDDING.md.

import { LaneletViewer } from './viewer.js';

const elements = {
  stage: document.getElementById('stage'),
  viewer: document.getElementById('viewer'),
  dropzone: document.getElementById('dropzone'),
  fileInput: document.getElementById('file-input'),
  sampleButton: document.getElementById('sample-button'),
  exportButton: document.getElementById('export-button'),
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
  threeDToggle: document.getElementById('three-d-toggle'),
  threeDControls: document.getElementById('three-d-controls'),
  reliefLabel: document.getElementById('relief-label'),
  yawInput: document.getElementById('yaw-input'),
  pitchInput: document.getElementById('pitch-input'),
  exaggerationInput: document.getElementById('exaggeration-input'),
  status: document.getElementById('status'),
  statusText: document.getElementById('status-text'),
  versionLabel: document.getElementById('version-label'),
};

const viewer = new LaneletViewer(elements.viewer, { theme: 'dark' });
let fileName = 'map';

viewer.addEventListener('loadstart', (event) => {
  fileName = event.detail.name;
  showStatus('Parsing the map…');
});

viewer.addEventListener('load', (event) => {
  hideStatus();
  showFacts(event.detail);
  showErrors(event.detail.errors, event.detail.problems);
  showLayerToggles();
  showLegend();
  showViewControls(event.detail.relief);
  elements.dropzone.hidden = true;
  elements.layersBlock.hidden = false;
  elements.optionsBlock.hidden = false;
});

viewer.addEventListener('error', (event) => showError(event.detail.message));

// The viewer has a 3D button on the canvas as well, so the sidebar follows the
// camera rather than being a second, disagreeing statement of it.
viewer.addEventListener('view3dchange', (event) => {
  const camera = event.detail;
  elements.threeDToggle.checked = camera.enabled;
  elements.threeDControls.hidden = !camera.enabled;
  elements.yawInput.value = camera.yaw;
  elements.pitchInput.value = camera.pitch;
  elements.exaggerationInput.value = camera.exaggeration;
});

// --- boot --------------------------------------------------------------------

showStatus('Loading the WebAssembly module…');
viewer.ready
  .then(({ version }) => {
    elements.versionLabel.textContent = `simple_lanelet2 v${version}`;
    hideStatus();
    wireUp();
  })
  .catch(() => {
    /* the viewer's own error event has already reported it */
  });

function wireUp() {
  elements.fileInput.addEventListener('change', () => {
    const file = elements.fileInput.files && elements.fileInput.files[0];
    if (file) viewer.loadFile(file);
    elements.fileInput.value = '';
  });

  elements.sampleButton.addEventListener('click', () => {
    showStatus('Fetching the example map…');
    viewer.loadUrl('./sample/mapping_example.osm', { name: 'mapping_example' });
  });

  elements.exportButton.addEventListener('click', () => {
    const svg = viewer.toSVG({ width: 1920, height: 1200 });
    if (!svg) return;
    const url = URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml' }));
    const link = document.createElement('a');
    link.href = url;
    link.download = `${fileName}.svg`;
    link.click();
    URL.revokeObjectURL(url);
  });

  elements.themeSelect.addEventListener('change', () => {
    const theme = elements.themeSelect.value;
    viewer.setTheme(theme);
    document.body.dataset.theme = viewer.theme;
    showLegend();
  });

  elements.coordinatesSelect.addEventListener('change', () => {
    viewer.setCoordinates(elements.coordinatesSelect.value);
  });

  elements.pointsToggle.addEventListener('change', () => {
    viewer.setDrawPoints(elements.pointsToggle.checked);
    showLayerToggles();
    showLegend();
  });

  elements.threeDToggle.addEventListener('change', () => {
    viewer.setView3d(elements.threeDToggle.checked);
    elements.threeDControls.hidden = !elements.threeDToggle.checked;
  });

  // `input` rather than `change`, so dragging a slider turns the map as you drag.
  // Each one rebuilds the scene in Rust, which for a map of any size is a frame's
  // work — the reason the angles are sliders here and not a drag on the canvas,
  // where the same cost would land on every pointer move over a city.
  for (const [input, key] of [
    [elements.yawInput, 'yaw'],
    [elements.pitchInput, 'pitch'],
    [elements.exaggerationInput, 'exaggeration'],
  ]) {
    input.addEventListener('input', () => viewer.setView3d({ [key]: Number(input.value) }));
  }

  installDragAndDrop();

  window.addEventListener('keydown', (event) => {
    if (event.target instanceof HTMLInputElement || event.target instanceof HTMLSelectElement) return;
    if (event.key === 'f' || event.key === 'F') viewer.fit();
  });
}

/// The viewer reads the dropped file; the page's share is the highlight, which is
/// the only part of this that is about a page rather than about a map.
function installDragAndDrop() {
  viewer.acceptDrops(window);
  let depth = 0;
  window.addEventListener('dragenter', (event) => {
    event.preventDefault();
    depth += 1;
    document.body.classList.add('dragging');
  });
  window.addEventListener('dragover', (event) => {
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'copy';
  });
  window.addEventListener('dragleave', (event) => {
    event.preventDefault();
    depth = Math.max(0, depth - 1);
    if (depth === 0) document.body.classList.remove('dragging');
  });
  window.addEventListener('drop', () => {
    depth = 0;
    document.body.classList.remove('dragging');
  });
}

// --- side panel --------------------------------------------------------------

function showFacts(detail) {
  const { stats } = detail;
  const rows = [
    ['Lanelets', stats.lanelets],
    ['Linestrings', stats.lineStrings],
    ['Polygons', stats.polygons],
    ['Areas', stats.areas],
    ['Regulatory elements', stats.regulatoryElements],
    ['Points', stats.points],
    // Without this there is nothing on screen saying how big the map is, and a
    // file whose extent is stretched by one distant fragment — which is most
    // real maps — reads as "only part of it was drawn".
    ['Extent', formatExtent(detail.bounds)],
    [
      'Coordinates',
      detail.coordinateSource === 'local_xy' ? 'local_x / local_y' : detail.projection.toUpperCase(),
    ],
  ];
  if (detail.coordinateSource !== 'local_xy') {
    rows.push(['Origin', `${detail.origin.lat.toFixed(5)}, ${detail.origin.lon.toFixed(5)}`]);
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

/// `[minX, minY, maxX, maxY]` in metres, as a "width × height" a person can read.
function formatExtent([minX, minY, maxX, maxY]) {
  const one = (metres) =>
    metres >= 1000 ? `${(metres / 1000).toFixed(metres >= 10000 ? 0 : 1)} km` : `${Math.round(metres)} m`;
  return `${one(maxX - minX)} × ${one(maxY - minY)}`;
}

/// The 3D controls, offered only when the file has something for them to show.
///
/// A Lanelet2 map is not required to carry `ele`, and plenty do not; tilting the
/// camera over one of those gives a flat sheet at an angle, which reads as a broken
/// viewer. So the relief is stated next to the toggle, and a map without any turns
/// the toggle off rather than leaving it to disappoint.
function showViewControls(relief) {
  const flat = !(relief >= 0.5);
  elements.threeDToggle.disabled = flat;
  elements.reliefLabel.textContent = flat ? 'no elevation in this map' : `${Math.round(relief)} m of relief`;
  if (flat && elements.threeDToggle.checked) {
    elements.threeDToggle.checked = false;
    viewer.setView3d(false);
  }
  const camera = viewer.getView3d();
  elements.yawInput.value = camera.yaw;
  elements.pitchInput.value = camera.pitch;
  elements.exaggerationInput.value = camera.exaggeration;
  elements.threeDControls.hidden = !elements.threeDToggle.checked;
}

function showLayerToggles() {
  elements.layerToggles.innerHTML = '';
  for (const layer of viewer.getLayers()) {
    const label = document.createElement('label');
    label.className = layer.count ? 'toggle' : 'toggle empty';
    const input = document.createElement('input');
    input.type = 'checkbox';
    input.checked = layer.visible;
    input.disabled = layer.count === 0;
    input.addEventListener('change', () => viewer.setLayers({ [layer.key]: input.checked }));
    const text = document.createElement('span');
    text.textContent = layer.label;
    const count = document.createElement('span');
    count.className = 'count';
    count.textContent = layer.count ? layer.count.toLocaleString() : '—';
    label.append(input, text, count);
    elements.layerToggles.append(label);
  }
}

function showLegend() {
  const items = viewer.legend;
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

function showErrors(errors, problems) {
  if (!errors.length) {
    elements.errorsBlock.hidden = true;
    return;
  }
  elements.errorsCount.textContent = `(${problems})`;
  elements.errors.textContent = errors.join('\n');
  elements.errorsBlock.hidden = false;
}

// --- status ------------------------------------------------------------------

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

function escapeHtml(text) {
  return String(text).replace(
    /[&<>"']/g,
    (character) =>
      ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[character],
  );
}
