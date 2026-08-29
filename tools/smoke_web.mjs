// Instantiates the built WebAssembly module outside a browser and puts a real map
// through it.
//
// `cargo test` exercises the same Rust on the host, where `wasm-bindgen`'s glue is
// inert. What it cannot tell you is whether the *module* instantiates, whether the
// exported names survived, or whether the arrays that cross the boundary line up —
// and a page that fails on any of those looks exactly like a page that deployed
// fine, until someone opens it.
//
//   node tools/smoke_web.mjs
//
// Run it after tools/build_web.sh; it reads web/pkg and web/sample.

import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const failures = [];

function check(condition, description) {
  if (condition) console.log(`  ok   ${description}`);
  else {
    console.log(`  FAIL ${description}`);
    failures.push(description);
  }
}

const wasm = await import(join(root, 'web/pkg/ll2_wasm.js'));
// `--target web` reaches for `fetch` on a URL by default, which has no meaning
// against the filesystem; handing it the bytes skips that entirely.
await wasm.default({ module_or_path: await readFile(join(root, 'web/pkg/ll2_wasm_bg.wasm')) });

console.log(`simple_lanelet2 ${wasm.version()}`);
check(/^\d+\.\d+\.\d+$/.test(wasm.version()), 'the module reports a version');

const text = await readFile(join(root, 'web/sample/mapping_example.osm'), 'utf8');
const handle = wasm.load_osm(text, 'auto');

const stats = JSON.parse(handle.stats_json());
console.log('stats:', JSON.stringify(stats));
check(stats.lanelets > 300, 'the example map yields its lanelets');
check(stats.points > 2000, 'the example map yields its points');
check(handle.coordinate_source() === 'projected', 'a georeferenced map projects');
check(handle.projection() === 'utm', 'and does so through UTM');
check(Math.abs(handle.origin_lat() - 49) < 1, 'the origin comes from the file');
check(handle.errors().length === 0, 'the example map parses without warnings');

const options = new wasm.SceneOptions();
options.centerlines = true;
const scene = handle.build_scene(options);

const coords = scene.coords();
const offsets = scene.offsets();
const styles = scene.styles();
const layers = scene.layers();
const closed = scene.closed();
const ids = scene.ids();
const styleTable = JSON.parse(scene.styles_json());
const layerTable = JSON.parse(wasm.layers());
const count = scene.shape_count();

console.log(
  `scene: ${count} shapes, ${coords.length / 2} vertices, ` +
    `${styleTable.length} styles, ${layerTable.length} layers`,
);

check(count > 1000, 'the scene has shapes');
check(coords instanceof Float32Array, 'coords cross as a Float32Array');
check(offsets instanceof Uint32Array, 'offsets cross as a Uint32Array');
check(closed instanceof Uint8Array, 'closed flags cross as a Uint8Array');
check(ids instanceof Float64Array, 'ids cross as a Float64Array');
check(offsets.length === count + 1, 'offsets have one entry per shape plus an end');
check(offsets[offsets.length - 1] * 2 === coords.length, 'offsets end where coords do');
check(
  [styles, layers, closed, ids].every((array) => array.length === count),
  'every per-shape array is the same length',
);

let monotonic = true;
for (let i = 0; i + 1 < offsets.length; i += 1) if (offsets[i + 1] < offsets[i]) monotonic = false;
check(monotonic, 'offsets are monotonic');
check(
  styles.every((index) => index < styleTable.length),
  'every style index resolves',
);
check(
  layers.every((index) => index < layerTable.length),
  'every layer index resolves',
);
check(
  layerTable.map((layer) => layer.key).includes('lanelet_fill'),
  'the layer table names its layers',
);
check(
  layerTable[0].key === 'lanelet_fill' && typeof layerTable[0].default === 'boolean',
  'the layer table is ordered and carries the default visible set',
);
check(
  styleTable.every((s) => typeof s.hideBelowScale === 'number' && typeof s.solidBelowScale === 'number'),
  'styles carry their level-of-detail thresholds',
);
check(/^#[0-9a-f]{6}$/.test(scene.highlight()), 'the theme reports a highlight colour');
check(scene.label(0).length > 0, 'shapes carry a label');
check(scene.label(count + 10) === '', 'an out-of-range label is empty, not a throw');
check(/^#[0-9a-f]{6}$/.test(scene.background()), 'the theme reports a background colour');

const bounds = scene.bounds();
check(bounds.length === 4 && bounds[2] > bounds[0], 'the scene reports usable bounds');
// Recentring is what keeps f32 precise; a coordinate far from zero means it broke.
const extreme = coords.reduce((worst, value) => Math.max(worst, Math.abs(value)), 0);
check(extreme < (bounds[2] - bounds[0] + bounds[3] - bounds[1]), 'coordinates are recentred');

const svg = handle.to_svg(options, 800, 600);
check(svg.startsWith('<svg'), 'SVG export produces a document');
check(svg.includes('non-scaling-stroke'), 'SVG strokes are zoom-independent');
check(svg.trimEnd().endsWith('</svg>'), 'the SVG document is closed');

// The 3D view crosses the boundary as the same arrays with different numbers in
// them, which is the whole reason the canvas renderer needs no notion of it.
check(handle.relief() > 0, 'the example map has some elevation to draw');
check(handle.has_relief(), 'and enough of it to be worth tilting');
options.three_d = true;
const tilted = handle.build_scene(options);
const tiltedCoords = tilted.coords();
check(tilted.shape_count() === count, '3D draws the same shapes');
check(tiltedCoords.length === coords.length, 'and hands over the same array shape');
check(
  tiltedCoords.some((value, index) => value !== coords[index]),
  'and different coordinates in it',
);
check(handle.to_svg(options, 800, 600) !== svg, 'the SVG export follows the camera too');
options.three_d = false;

// The default camera is Rust's, so a renderer never states one of its own.
const cameraTable = JSON.parse(wasm.camera());
check(
  Number.isFinite(cameraTable.yaw) && Number.isFinite(cameraTable.pitch),
  'the default camera crosses as numbers',
);
check(
  cameraTable.pitchRange[0] < cameraTable.pitch && cameraTable.pitch <= cameraTable.pitchRange[1],
  'and sits inside the range it advertises',
);
check(cameraTable.minRelief > 0, 'and says how much relief is worth tilting');

let threw = false;
try {
  wasm.load_osm('this is not a map', 'auto');
} catch {
  threw = true;
}
check(threw, 'a file that is not a map raises rather than returning an empty scene');

if (failures.length) {
  console.error(`\n${failures.length} check(s) failed`);
  process.exit(1);
}
console.log('\nall checks passed');
