load("@gazelle//:def.bzl", "gazelle_generation_test")

def generation_tests():
    """
    Generate test targets for all gazelle_rust generation test cases.

    Each subdirectory containing a MODULE.bazel file is treated as a test case.
    Test cases must have:
      - MODULE.bazel: empty file marking this as a test case.
      - BUILD.in: initial BUILD file state (can be empty).
      - BUILD.out: expected BUILD file after running Gazelle.
      - *.rs files: Rust source files to generate rules for.

    To update expected files: UPDATE_SNAPSHOTS=true bazel run //path/to:test_target
    """
    for module_file in native.glob(["**/MODULE.bazel"]):
        dir = module_file[:-len("/MODULE.bazel")]

        gazelle_generation_test(
            name = dir,
            gazelle_binary = "//:gazelle_multilang",
            test_data = native.glob([dir + "/**"]),
        )
