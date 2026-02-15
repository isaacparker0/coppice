DependencyClosureInfo = provider(
    fields = ["paths"],
    doc = "Map: Label -> one path (list of labels) from root target to that label.",
)

def _collect_target_values(value):
    out = []
    stack = [value]

    # Starlark disallows recursion and while-loops. Use a bounded for-loop as
    # an explicit stack walk to support arbitrarily nested containers.
    for _ in range(10000):
        if not stack:
            return out

        current = stack.pop()
        current_type = type(current)

        if current_type == "Target":
            out.append(current)
            continue

        if current_type == "list" or current_type == "tuple":
            for item in current:
                stack.append(item)
            continue

        if current_type == "dict":
            for key, item in current.items():
                stack.append(key)
                stack.append(item)

    fail("dependency_enforcement target traversal exceeded max depth/items")

def _dependency_closure_aspect_impl(target, ctx):
    paths = {target.label: [target.label]}

    if not hasattr(ctx, "rule") or not ctx.rule:
        return [DependencyClosureInfo(paths = paths)]

    for attr_name in dir(ctx.rule.attr):
        if attr_name.startswith("_"):
            continue

        attr_value = getattr(ctx.rule.attr, attr_name)
        dep_targets = _collect_target_values(attr_value)

        for dep in dep_targets:
            if DependencyClosureInfo not in dep:
                continue

            dep_paths = dep[DependencyClosureInfo].paths
            for dep_label, dep_path in dep_paths.items():
                if dep_label not in paths:
                    paths[dep_label] = [target.label] + dep_path

    return [DependencyClosureInfo(paths = paths)]

dependency_closure_aspect = aspect(
    implementation = _dependency_closure_aspect_impl,
    attr_aspects = ["*"],
)

def _dependency_enforcement_test_impl(ctx):
    if DependencyClosureInfo in ctx.attr.target:
        paths = ctx.attr.target[DependencyClosureInfo].paths

        for forbidden_dep in ctx.attr.forbidden:
            forbidden_label = forbidden_dep.label
            if forbidden_label in paths:
                path = paths[forbidden_label]
                pretty_path = " -> ".join([str(label) for label in path])
                fail(
                    "\n\nForbidden dependency detected!\n" +
                    "  Target: %s\n" % ctx.attr.target.label +
                    "  Forbidden: %s\n" % forbidden_label +
                    "  Path: %s\n" % pretty_path,
                )

    script = ctx.actions.declare_file(ctx.label.name + ".sh")
    ctx.actions.write(script, content = "#! /usr/bin/env bash\necho OK\n", is_executable = True)
    return [DefaultInfo(executable = script)]

dependency_enforcement_test = rule(
    implementation = _dependency_enforcement_test_impl,
    attrs = {
        "target": attr.label(
            mandatory = True,
            aspects = [dependency_closure_aspect],
        ),
        "forbidden": attr.label_list(mandatory = True),
    },
    test = True,
)
