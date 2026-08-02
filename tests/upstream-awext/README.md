# The extension's own tests, unmodified

Copied verbatim from `autoware_lanelet2_extension_python/test/` at the pinned tag, and
run against our implementation. Nothing here is adapted: a test that needs editing to
pass is a compatibility gap, not a test problem.

They are kept apart from `tests/upstream/` because they need the extension's import
path, which the plain `lanelet2` tests must never depend on.

## What they do and do not cover

These are import smoke tests, and their filenames say so. A hundred lines across three
files, checking that the classes and functions can be reached. That is worth having --
it catches a packaging mistake immediately -- but it is not a check of behaviour.

The extension's substantive tests are 935 lines of C++ gtest in
`autoware_lanelet2_extension/test/src/`: `test_query.cpp`, `test_utilities.cpp`,
`test_regulatory_elements.cpp`, `test_projector.cpp`. They cannot run against a Python
implementation, and compiling them would only have the reference check itself.

So "upstream's own tests pass unmodified" means less for the extension than it does for
Lanelet2 proper, and the real verification of the extension is ours: cases 1050 through
1400, which compare every class, projector and query against the reference on generated
shapes and on the 10.5 MB Nishi-Shinjuku map, down to the bytes it writes back.

The scenarios those C++ tests consider meaningful have deliberately *not* been ported.
Doing so would replace "upstream's tests, unmodified" with tests of our own choosing,
and the diff cases already cover more surface than the C++ ones do. It is recorded here
so the gap is a decision rather than an oversight.

## On TIER IV

`tier4/autoware_lanelet2_extension` is a fork of the `autowarefoundation` repository
that this fetches from, four months behind it, with the same four test files. The
extension is TIER IV-maintained either way -- its `package.xml` maintainers are all
`@tier4.jp` -- so there is nothing separate to pull in.

`test_projection.py` is not included. It locates its input map through
`ament_index_python`, a ROS package -- the dependency is in the test's plumbing rather
than in anything it exercises, and `1250_aw_mgrs` covers the same projector against the
reference far more thoroughly.
