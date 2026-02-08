# buildifier: disable=bzl-visibility
load("@rules_rs//rs/private:rust_deps.bzl", "rust_deps")
load("@rules_rust//rust:defs.bzl", _rust_binary = "rust_binary", _rust_library = "rust_library", _rust_test = "rust_test")

def rust_binary(
        name,
        srcs = [],
        deps = [],
        compile_data = [],
        rustc_env = {},
        edition = None,
        crate_root = None,
        proc_macro_deps = None,
        **kwargs):
    if len(srcs) != 1:
        fail("rust_binary must have exactly 1 source file, got {}".format(len(srcs)))

    if "/" in srcs[0]:
        fail("rust_binary source file must be in the package directory, not a subdirectory: {}".format(srcs[0]))

    expected_name = srcs[0][:-3]  # strip .rs
    if name != expected_name:
        fail("rust_binary target must be named after source file: name = \"{}\"".format(expected_name))

    if crate_root:
        fail("Do not set 'crate_root'; it will always be the single src.")

    if edition:
        fail("Do not set 'edition'; it is set globally via the toolchain in MODULE.bazel.")

    if proc_macro_deps:
        fail("Do not set 'proc_macro_deps'; add proc macro crates to 'deps' and they will be separated automatically.")

    dep_targets = _setup_rust_deps(name, deps)

    _rust_binary(
        name = name,
        srcs = srcs,
        deps = dep_targets.deps,
        proc_macro_deps = dep_targets.proc_macro_deps,
        compile_data = compile_data,
        rustc_env = rustc_env,
        lint_config = "//:cargo_lints",
        **kwargs
    )

def rust_library(
        name,
        srcs = [],
        deps = [],
        compile_data = [],
        rustc_env = {},
        edition = None,
        crate_name = None,
        crate_root = None,
        proc_macro_deps = None,
        **kwargs):
    expected_name = native.package_name().split("/")[-1]
    if name != expected_name:
        fail("rust_library target must be named after directory: name = \"{}\"".format(expected_name))

    if edition:
        fail("Do not set 'edition'; it is set globally via the toolchain in MODULE.bazel.")

    if crate_name:
        fail("Do not set 'crate_name'; it is auto-generated from the package path.")

    if crate_root:
        fail("Do not set 'crate_root'; it must be lib.rs.")

    if proc_macro_deps:
        fail("Do not set 'proc_macro_deps'; add proc macro crates to 'deps' and they will be separated automatically.")

    dep_targets = _setup_rust_deps(name, deps)

    _rust_library(
        name = name,
        srcs = srcs,
        deps = dep_targets.deps,
        proc_macro_deps = dep_targets.proc_macro_deps,
        compile_data = compile_data,
        rustc_env = rustc_env,
        crate_name = native.package_name().replace("/", "__"),
        crate_root = "lib.rs",
        lint_config = "//:cargo_lints",
        **kwargs
    )

def rust_test(
        name,
        srcs = [],
        deps = [],
        compile_data = [],
        rustc_env = {},
        edition = None,
        crate_root = None,
        proc_macro_deps = None,
        **kwargs):
    expected_name = native.package_name().split("/")[-1] + "_test"
    if name != expected_name:
        fail("rust_test target must be named after directory: name = \"{}\"".format(expected_name))

    if edition:
        fail("Do not set 'edition'; it is set globally via the toolchain in MODULE.bazel.")

    if crate_root:
        fail("Do not set 'crate_root'; each test file becomes its own crate root.")

    if proc_macro_deps:
        fail("Do not set 'proc_macro_deps'; add proc macro crates to 'deps' and they will be separated automatically.")

    dep_targets = _setup_rust_deps(name, deps)

    # Create one rust_test target per src, grouped under a test_suite. This
    # keeps BUILD files clean with a single macro call, while ensuring each
    # test file is a standard crate root that rust-analyzer handles correctly.
    test_targets = []
    for src in srcs:
        module_name = src.split("/")[-1].removesuffix(".rs")
        target_name = name + "__" + module_name

        _rust_test(
            name = target_name,
            srcs = [src],
            crate_root = src,
            deps = dep_targets.deps,
            proc_macro_deps = dep_targets.proc_macro_deps,
            compile_data = compile_data,
            rustc_env = rustc_env,
            lint_config = "//:cargo_lints",
            **kwargs
        )
        test_targets.append(":" + target_name)

    native.test_suite(
        name = name,
        tests = test_targets,
    )

def _setup_rust_deps(name, deps):
    """
    Create rust_deps targets that auto-filter deps vs proc_macro_deps.

    Returns a struct with deps and proc_macro_deps target references.
    """

    # rust_deps only works with crate deps that provide CrateInfo. Other deps
    # (workspace targets, proto libraries, etc.) are passed through directly.
    crate_deps = [dep for dep in deps if dep.startswith("@crates//")]
    other_deps = [dep for dep in deps if not dep.startswith("@crates//")]

    deps_name = name + "_deps"
    proc_macro_deps_name = name + "_proc_macro_deps"

    rust_deps(
        name = deps_name,
        deps = crate_deps,
        proc_macros = False,
    )
    rust_deps(
        name = proc_macro_deps_name,
        deps = crate_deps,
        proc_macros = True,
    )

    return struct(
        deps = [":" + deps_name] + other_deps,
        proc_macro_deps = [":" + proc_macro_deps_name],
    )
