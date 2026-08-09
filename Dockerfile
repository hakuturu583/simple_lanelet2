# A runtime image for the `lanelet2` wheel: an Alpine CPython with the extension
# module installed and nothing else that is not needed to import it.
#
# Three stages, and the split matters. `builder` needs a Rust toolchain, a C compiler
# and maturin -- over a gigabyte of them -- to produce one wheel. `rootfs` unpacks
# that wheel on the *runtime* base, so the `.so` is laid out against the interpreter
# that will load it, and prunes what a runtime does not use. Only the last is
# shipped, and it holds that pruned filesystem alone; neither cargo, nor rustc, nor
# pip, nor a line of this repository's source is in it.
#
# Alpine, so musl, so the wheel is tagged musllinux rather than manylinux. That is
# fine here and only here: the wheel is built and consumed inside this one build.
# Releases to PyPI are glibc manylinux wheels and are built elsewhere -- see
# .github/workflows/release.yml. Do not lift the artefact out of this image.
#
#   docker build -t simple-lanelet2 .
#   docker run --rm -v "$PWD:/work" simple-lanelet2 python -c "import lanelet2"

# Keep RUST_VERSION in step with the channel in rust-toolchain.toml. CI reads that
# file and passes it in, so the two cannot drift there; this default is for a bare
# `docker build` on a workstation. rust-toolchain.toml itself is deliberately kept
# out of the build context (see .dockerignore) -- were it present, rustup would
# fetch the rustfmt and clippy components it names, which this build never calls.
ARG RUST_VERSION=1.95.0
ARG PYTHON_VERSION=3.13
ARG ALPINE_VERSION=3.22


FROM rust:${RUST_VERSION}-alpine${ALPINE_VERSION} AS builder

# gcc and musl-dev are already in the rust:alpine image; python3 is the only thing
# missing. Not python3-dev: PyO3 declares the C ABI itself rather than including
# Python.h, and an abi3 extension-module build links against no libpython either, so
# the interpreter is wanted only for maturin to query and to run pip.
#
# patchelf is not optional here. The extension links against libgcc_s.so.1, which
# rust:alpine has and python:alpine does not, so maturin has to copy it into the
# wheel and rewrite the RPATH -- and it shells out to patchelf to do it. Without
# this the build dies at the very end, after the whole Rust compile, with "Failed to
# execute 'patchelf'".
RUN apk add --no-cache python3 py3-pip patchelf

# Alpine marks its system Python externally-managed (PEP 668). A venv would be the
# answer on a machine one has to live with; this stage is thrown away at the end of
# the build, so overriding the marker is the honest, one-line version. maturin builds
# against whichever interpreter invokes it -- abi3-py39 means the wheel it produces
# is loadable by any CPython >= 3.9 regardless, including the runtime stage's 3.13.
RUN pip install --no-cache-dir --break-system-packages "maturin>=1.9,<2.0"

WORKDIR /src
COPY Cargo.toml Cargo.lock pyproject.toml README.md LICENSE NOTICE ./
COPY crates crates
COPY python python

# The cache mounts make a rebuild after an edit cheap locally, and are simply cold
# in CI. `--out` copies the wheel to a real path, so nothing needed later lives in
# a mount. `--locked` fails rather than quietly resolving a dependency Cargo.lock
# does not pin.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    maturin build --release --locked --out /wheels


# The shipped filesystem, assembled but not yet shipped.
#
# Unpacking happens on the runtime base, not the builder: pip picks the wheel whose
# tags match the interpreter doing the installing, and it is the runtime's
# interpreter whose ABI has to be satisfied. `--target` gives a self-contained
# directory, which is also where maturin's vendored libgcc lands, so the RPATH it
# wrote still resolves.
#
# Then everything the base image carries for *building and installing* packages
# goes: pip and its vendored wheels, the bootstrapper that reinstalls them, IDLE,
# the CPython headers and the static build config. About 12 MB, and their absence is
# also why nothing in this image can fetch a package at runtime -- asserted here
# rather than only in CI, so that a base image that reorganises these paths fails
# the build that produced it instead of shipping a quietly fatter image.
FROM python:${PYTHON_VERSION}-alpine${ALPINE_VERSION} AS rootfs

COPY --from=builder /wheels /wheels
RUN pip install --no-cache-dir --no-deps --no-compile --target /opt/lanelet2 /wheels/*.whl \
 && python -c "import sys; sys.path.insert(0, '/opt/lanelet2'); import lanelet2" \
 && rm -rf /wheels \
           /usr/local/lib/python*/ensurepip \
           /usr/local/lib/python*/idlelib \
           /usr/local/lib/python*/config-*/ \
           /usr/local/lib/python*/site-packages/pip* \
           /usr/local/include/python* \
           /usr/local/bin/pip* \
           /usr/local/bin/idle* \
           /usr/local/bin/python*-config \
 && find /usr/local/lib/python*/ -type d -name '__pycache__' -prune -exec rm -rf {} + \
 && [ ! -e /usr/local/bin/pip ] \
 && python -c "import importlib.util as u; assert not any(u.find_spec(m) for m in ('pip', 'ensurepip'))" \
 && adduser -D -u 1000 lanelet2


# `rm` in a layer above the base does not make an image smaller -- it writes a
# whiteout and the deleted bytes still ship in the layer underneath. Copying the
# finished filesystem onto `scratch` is what actually reclaims them: one layer,
# holding only what survived. The cost is that this image shares no layer with
# `python:alpine`, which is the right trade for a leaf image nobody builds FROM.
FROM scratch

ARG VERSION=dev
LABEL org.opencontainers.image.title="simple_lanelet2" \
      org.opencontainers.image.description="A drop-in reimplementation of the Lanelet2 Python API, in Rust" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.licenses="BSD-3-Clause" \
      org.opencontainers.image.source="https://github.com/hakuturu583/simple_lanelet2"

COPY --from=rootfs / /

# Starting from `scratch` means inheriting no environment either, so PATH has to be
# restated -- it is the base image's, verbatim. PYTHONPATH rather than copying into
# site-packages, so that the path carries no Python version in it and a base-image
# bump cannot silently strand the package.
ENV PATH=/usr/local/bin:/usr/local/sbin:/usr/sbin:/usr/bin:/sbin:/bin \
    PYTHONPATH=/opt/lanelet2 \
    PYTHONDONTWRITEBYTECODE=1

USER lanelet2
WORKDIR /work

# A library, not a program: the useful default is an interpreter with it importable.
CMD ["python3"]
