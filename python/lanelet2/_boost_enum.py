"""Boost.Python-style enumerations.

``Boost.Python.enum`` derives from ``int``, so its members compare and do
arithmetic like integers, carry a ``name``, and the class exposes ``names`` and
``values`` dictionaries. Members are also injected into the enclosing module's
namespace, which is what ``export_values()`` does.

None of that is reachable from a PyO3 ``#[pyclass]`` enum, which is an opaque type,
so the enums are built here and the Rust side looks members up by name when it has
one to return.
"""

__all__ = ["boost_enum"]


class _BoostEnumBase(int):
    """An enumeration member: an ``int`` that knows its own name.

    ``name`` is a property on the *class* rather than a per-instance attribute, so
    that it shows up in ``dir(EnumClass)`` the way Boost's does.
    """

    @property
    def name(self):
        return self._member_name

    def __repr__(self):
        return "{}.{}.{}".format(type(self).__module__, type(self).__name__, self.name)

    def __str__(self):
        return self.name


def boost_enum(name, module, members):
    """Builds an enumeration class and its members.

    ``members`` is a sequence of ``(name, value)`` pairs. The members end up as
    attributes of the returned class; the caller places them in the module
    namespace as well.
    """
    enum_class = type(name, (_BoostEnumBase,), {"__module__": module})

    names = {}
    values = {}
    for member_name, value in members:
        member = int.__new__(enum_class, value)
        member._member_name = member_name
        setattr(enum_class, member_name, member)
        names[member_name] = member
        values[value] = member

    enum_class.names = names
    enum_class.values = values
    return enum_class
