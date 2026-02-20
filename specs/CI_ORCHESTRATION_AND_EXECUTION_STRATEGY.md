# CI Orchestration and Execution Strategy (Draft)

## Purpose

Define a practical CI/deploy architecture for this repository that preserves
fast Bazel builds while keeping orchestration expressive and reliable for a
small project.

## Context

- Hosted GitHub Actions runners are too slow for Bazel-heavy workloads because
  Bazel loading/analysis still runs on the runner even when remote execution is
  enabled.
- BuildBuddy workflows provide strong Bazel performance but limited
  orchestration primitives for deploy flow control.
- We want reproducible, code-defined infra and low operational overhead.

## Decision

Use:

- BuildBuddy for Bazel remote execution and remote cache.
- A single always-on DigitalOcean machine as the primary CI runner/orchestration
  host.
- Kubernetes-based scaling only if queueing becomes a real bottleneck.

## Execution Boundary

- Runs on CI runner host:
  - workflow step execution
  - Bazel server startup
  - Bazel loading/module resolution/analysis
- Runs on BuildBuddy remote executors:
  - Bazel action execution when `--remote_executor` is enabled

## Why this approach

- Keeps orchestration flexible (conditions, concurrency controls, deploy
  sequencing).
- Preserves BuildBuddy performance where it matters most (action execution and
  caching).
- Avoids immediate complexity of autoscaling infrastructure for a hobby-sized
  project.

## Deployment Safety Requirements

- Serialize deploys per environment (no cancel-in-progress for active deploys).
- Guard against stale commits deploying after newer commits.
- Keep manual deploy capability available.

## Tooling Approach for Infrastructure

- OpenTofu binary is Bazel-managed via `rules_multitool`.
- Developers invoke `tofu` via repo-local wrapper (`bin/tofu`) exposed through
  `direnv`.
- No machine-level OpenTofu installation step is required.

## IaC Direction

- Create and manage runner infrastructure as code.
- Start with a single DigitalOcean instance managed via Terraform/OpenTofu.
- Evolve to Kubernetes/DOKS autoscaling only if throughput needs increase.

## Non-Goals (Current Phase)

- Multi-runner autoscaling from day one.
- Full migration to a different CI platform before validating the single-runner
  model.

## Rollout Plan

1. Stabilize single-runner workflow and deploy safety behavior.
2. Measure queue time and wall-clock CI latency.
3. Introduce additional runners or Kubernetes autoscaling only when justified by
   data.
