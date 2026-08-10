# The map viewer

A Lanelet2 `.osm` viewer that runs entirely in the browser, published to
<https://hakuturu583.github.io/simple_lanelet2/>.

The map is parsed and styled by this repository's own Rust library, compiled to
WebAssembly. Nothing is uploaded: the file is read by the page, handed to the wasm
module as a string, and drawn from the arrays that come back.

**It is a component, not a page.** `index.html` is one consumer of it; a Foxglove
panel or a wandb HTML block is another. See [EMBEDDING.md](EMBEDDING.md), or
<https://hakuturu583.github.io/simple_lanelet2/embed-example.html> for both routes
side by side.

## What is what

| | |
| --- | --- |
| [`crates/ll2-viz`](../crates/ll2-viz) | reads the file, picks the projection, classifies primitives by tag, decides every colour, width, dash and arrow — and renders to SVG on its own |
| [`crates/ll2-wasm`](../crates/ll2-wasm) | hands a scene across the WebAssembly boundary as flat typed arrays |
| [`viewer.js`](viewer.js) | **the embeddable component**: `LaneletViewer` and `<lanelet2-viewer>`. Shadow DOM, `ResizeObserver`, no globals, a real `destroy()` |
| [`embed.html`](embed.html) + [`embed.js`](embed.js) | the iframe face: query parameters in, `postMessage` both ways |
| [`app.js`](app.js) | the demo's sidebar — a consumer of `viewer.js`, with no privileges the others lack |

Nothing in `viewer.js` knows what a `line_thin` is; nothing in `app.js` knows what
a canvas transform is. The same scene drives the canvas and the SVG that **Export
SVG** produces.

## Building it

```bash
tools/build_web.sh --serve      # then open http://localhost:8000
```

or, without the server:

```bash
just web                        # build web/pkg, then smoke-test the module
just web-test                   # and drive it in a real browser
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

## Testing it

| | |
| --- | --- |
| `node tools/smoke_web.mjs` | instantiates the module outside a browser and checks the arrays that cross the boundary line up |
| `node tools/browser_test.mjs` | drives the demo, the iframe and the element in Chromium — shadow-root event delivery, the element upgrading, `ResizeObserver`, `destroy()`, the postMessage round trip |

The second needs Playwright (`npm install --no-save playwright && npx playwright
install chromium`); without it, it says so and exits 0. CI runs both.

## Deployment

[`.github/workflows/pages.yml`](../.github/workflows/pages.yml) builds this
directory, runs both test scripts, and uploads it on every push to `main`. Pull
requests build and test but do not deploy.

Publishing needs Pages switched to **GitHub Actions** as its source, once, under
the repository's *Settings → Pages*.
