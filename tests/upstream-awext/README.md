# The extension's own tests, unmodified

Copied verbatim from `autoware_lanelet2_extension_python/test/` at the pinned tag, and
run against our implementation. Nothing here is adapted: a test that needs editing to
pass is a compatibility gap, not a test problem.

They are kept apart from `tests/upstream/` because they need the extension's import
path, which the plain `lanelet2` tests must never depend on.

`test_projection.py` is not included. It locates its input map through
`ament_index_python`, a ROS package -- the dependency is in the test's plumbing rather
than in anything it exercises, and `1250_aw_mgrs` covers the same projector against the
reference far more thoroughly.
