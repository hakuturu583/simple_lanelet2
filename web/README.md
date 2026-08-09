# The map viewer

A drag-and-drop viewer for Lanelet2 `.osm` maps, published to
<https://hakuturu583.github.io/simple_lanelet2/>.

The map is parsed and styled by this repository's own Rust library, compiled to
WebAssembly. Nothing is uploaded: the file is read by the page, handed to the wasm
module as a string, and drawn from the arrays that come back.

## Where the work happens

| | |
| --- | --- |
| [`crates/ll2-viz`](../crates/ll2-viz) | reads the file, picks the projection, classifies primitives by tag, decides every colour, width, dash and arrow — and can render the result to SVG on its own |
| [`crates/ll2-wasm`](../crates/ll2-wasm) | hands a scene across the WebAssembly boundary as flat typed arrays |
| [`main.js`](main.js) | draws those arrays to a `<canvas>` and handles pan, zoom and picking |

The split is deliberate: nothing in `main.js` knows what a `line_thin` is. The same
scene drives the canvas here and the SVG that the **Export SVG** button produces.

## Building it

```bash
tools/build_web.sh --serve      # then open http://localhost:8000
```

or, without the server:

```bash
just web                        # build web/pkg and run the smoke test
```

`tools/build_web.sh` needs the `wasm32-unknown-unknown` target and a `wasm-bindgen`
binary of exactly the version in `Cargo.lock`:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version "$(tools/build_web.sh --print-version)"
```

`web/pkg` (the built module) and `web/sample` (a copy of the example map) are build
outputs and are not committed.

`file://` will not do — the page is an ES module and fetches its own wasm, both of
which need a real origin.

## Deployment

[`.github/workflows/pages.yml`](../.github/workflows/pages.yml) builds this
directory and uploads it on every push to `main`. Pull requests run the build and
the smoke test but do not deploy.

Publishing needs Pages switched to **GitHub Actions** as its source, once, under
the repository's *Settings → Pages*.
