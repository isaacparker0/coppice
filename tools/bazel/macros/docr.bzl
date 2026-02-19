load("@aspect_bazel_lib//lib:expand_template.bzl", "expand_template")
load("@rules_oci//oci:defs.bzl", "oci_push")

DOCR_REGISTRY = "registry.digitalocean.com/coppice"

def docr_push(name, image, repository):
    """
    Push an OCI image to DOCR, tagged with commit SHA.
    """
    tags_name = name + "_tags"

    expand_template(
        name = tags_name,
        stamp_substitutions = {"__COMMIT_SHA__": "{{STABLE_COMMIT_SHA}}"},
        template = ["__COMMIT_SHA__"],
    )

    oci_push(
        name = name,
        image = image,
        remote_tags = ":" + tags_name,
        repository = DOCR_REGISTRY + "/" + repository,
        tags = ["manual"],
    )
