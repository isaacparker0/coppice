load("@aspect_bazel_lib//lib:expand_template.bzl", "expand_template")
load("@bazel_skylib//rules:write_file.bzl", "write_file")
load("@rules_oci//oci:defs.bzl", "oci_push")

DOCR_REGISTRY = "registry.digitalocean.com/coppice"

def docr_push(name, image, repository):
    """
    Push an OCI image to DOCR.

    Always pushes stamped STABLE_COMMIT_SHA.
    If --define push_deploy=true is set, also pushes `live`.
    """
    tags_name = name + "_tags"
    tags_template_name = name + "_tags_template"

    write_file(
        name = tags_template_name,
        out = tags_template_name + ".txt",
        content = select({
            "//tools/bazel/config:push_deploy": ["__COMMIT_SHA__", "live"],
            "//conditions:default": ["__COMMIT_SHA__"],
        }),
    )

    expand_template(
        name = tags_name,
        template = ":" + tags_template_name,
        stamp_substitutions = {"__COMMIT_SHA__": "{{STABLE_COMMIT_SHA}}"},
    )

    oci_push(
        name = name,
        image = image,
        remote_tags = ":" + tags_name,
        repository = DOCR_REGISTRY + "/" + repository,
        tags = ["manual"],
    )
