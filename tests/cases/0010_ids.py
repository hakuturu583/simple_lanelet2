"""The global id counter.

Upstream's counter is a process-wide ``std::atomic<Id>`` initialised to 1000, and
``registerId`` raises it to ``id + 1`` without ever rewinding it
(``lanelet2_core/src/LaneletMap.cpp:881-891``). Because the runner gives every case
a fresh process, ids are fully deterministic and can be compared directly -- which
is what lets every later case construct primitives without seeding anything.
"""

from canon import emit, run

from lanelet2.core import getId, registerId


def main():
    emit("first", getId())
    emit("second", getId())
    emit("third", getId())

    registerId(50000)
    emit("after_register", getId())
    emit("after_register_2", getId())

    # Registering a lower id must not rewind the counter.
    registerId(10)
    emit("after_low_register", getId())

    # Registering the id the counter is about to hand out must skip past it.
    nxt = getId()
    registerId(nxt + 1)
    emit("after_register_next", getId())


run(main)
