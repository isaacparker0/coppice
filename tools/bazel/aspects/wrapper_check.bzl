"""
Global aspect to enforce that certain rules are only used wrapper macros.
"""

_WRAPPED_RULES = [
    "rust_binary",
    "rust_library",
    "rust_test",
]

def _wrapper_check_impl(target, ctx):
    if target.label.workspace_name != "":
        return []

    if ctx.rule.kind in _WRAPPED_RULES:
        if not getattr(ctx.rule.attr, "generator_function", None):
            fail("%s uses %s directly. Use wrapper from //tools/bazel/macros." % (
                target.label,
                ctx.rule.kind,
            ))
    return []

wrapper_check_aspect = aspect(
    implementation = _wrapper_check_impl,
    attr_aspects = ["deps"],
)
