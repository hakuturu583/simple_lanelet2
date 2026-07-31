"""`AttributeMap` — dict-like, ordered by key, and a live view of its owner.

Upstream backs it with `std::map`, so iteration order is sorted rather than
insertion-ordered, and that ordering is observable in `repr` (and later in the tag
order of a written OSM file). Iterating yields entry objects rather than bare keys,
a `map_indexing_suite` convention inherited from Boost.Python.
"""

from canon import emit, expect_raises, run

from lanelet2.core import AttributeMap


def main():
    # Deliberately not in key order, to pin that iteration sorts.
    attributes = AttributeMap({"type": "line_thin", "subtype": "solid", "ele": "3"})

    emit("len", len(attributes))
    emit("repr", repr(attributes))
    emit("str", str(attributes))
    emit("keys", list(attributes.keys()))
    emit("values", list(attributes.values()))
    emit("items", [list(item) for item in attributes.items()])
    emit("keys_type", type(attributes.keys()).__name__)

    entries = list(attributes)
    emit("iter_type", type(entries[0]).__name__)
    emit("iter_module", type(entries[0]).__module__)
    emit("iter_str", [str(entry) for entry in entries])
    emit("iter_key_data", [[entry.key(), entry.data()] for entry in entries])

    emit("getitem", attributes["type"])
    emit("contains_present", "type" in attributes)
    emit("contains_absent", "nope" in attributes)
    expect_raises("getitem_missing", lambda: attributes["nope"], msg=True)

    emit("eq_same", attributes == AttributeMap({"ele": "3", "subtype": "solid", "type": "line_thin"}))
    emit("eq_dict", attributes == {"ele": "3", "subtype": "solid", "type": "line_thin"})
    emit("eq_different", attributes == {"type": "line_thin"})
    emit("ne_different", attributes != {"type": "line_thin"})

    attributes["new"] = "value"
    emit("after_setitem", [len(attributes), attributes["new"]])
    del attributes["new"]
    emit("after_delitem", len(attributes))

    empty = AttributeMap()
    emit("empty_len", len(empty))
    emit("empty_repr", repr(empty))
    emit("empty_str", str(empty))

    # Python's own quoting rules must show through, since upstream renders the map
    # by handing a real dict to Python's repr.
    emit("quoting", repr(AttributeMap({"a'b": 'c"d', "plain": "x"})))


run(main)
