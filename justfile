# simple_lanelet2 task runner.
#
# The two virtualenvs are the heart of the workflow:
#   .venv      our library, built by maturin
#   .venv-ref  the real lanelet2==1.2.3 from PyPI, kept pristine
# `just diff` runs every test case under both and compares the output.

default: build diff

# Create both virtualenvs. Safe to re-run.
venvs:
    uv venv --python 3.11 .venv
    uv pip install --python .venv maturin
    uv venv --python 3.11 .venv-ref
    uv pip install --python .venv-ref lanelet2==1.2.3
    uv pip freeze --python .venv-ref > tests/ref-requirements.txt

# Build and install into .venv. --no-sync keeps `uv run` from re-syncing (and thus
# rebuilding and reinstalling) the project on every invocation.
build:
    uv run --no-sync --python .venv maturin develop --uv

# The Autoware oracle's counterpart: our wheel on Python 3.10, so that both sides of
# an `# ORACLE: aw` case speak the same ABI. The reference itself is a ROS overlay
# workspace, reached by sourcing it -- see SIMPLE_LL2_AW_SETUP in tests/runner.py.
venv-aw: build-wheel
    # 3.12 to match the pinned oracle: both sides of an `# ORACLE: aw`
    # comparison must be the same interpreter, and RoboStack picks it.
    uv venv --python 3.12 .venv-aw
    uv pip install --python .venv-aw --reinstall target/wheels/*.whl

# A real wheel rather than an editable install; `# ORACLE: aw` cases need one.
build-wheel:
    rm -f target/wheels/*.whl
    uv run --no-sync --python .venv maturin build --out target/wheels

build-release:
    uv run --no-sync --python .venv maturin develop --uv --release

# The cross-implementation diff harness. Runs under .venv's interpreter because it
# needs tomllib (3.11+); it imports nothing from either environment itself.
diff *ARGS:
    ./.venv/bin/python tests/runner.py {{ARGS}}

# Faster dev loop: only assert REF == COMPAT, skip the COMPAT-vs-FIXED check.
diff-compat *ARGS:
    ./.venv/bin/python tests/runner.py --mode compat {{ARGS}}

# API-surface burn-down.
coverage:
    ./.venv/bin/python tests/runner.py --coverage

# All three streams for one case, side by side.
show CASE:
    ./.venv/bin/python tests/runner.py --show {{CASE}}

test-rust:
    cargo test --workspace

# Upstream's own test suite, run unmodified against our implementation, in both
# modes. It passes either way: none of the repaired defects is one upstream tests.
upstream-tests:
    LANELET2_BUG_COMPAT=1 ./.venv/bin/python -m pytest tests/upstream -q
    ./.venv/bin/python -m pytest tests/upstream -q

# Regenerate the type stubs from the built extension.
stubs: build
    ./.venv/bin/python tools/gen_stubs.py

fmt:
    cargo fmt --all

lint:
    cargo clippy --workspace --all-targets -- -D warnings

clean:
    cargo clean
    rm -rf .venv .venv-ref target/wheels
