# Kubernetes

The manifests in `deploy/kubernetes` are a minimal starting point for running Aximo with externally mounted model files.

```bash
kubectl apply -k deploy/kubernetes
```

## Model Volume

The Deployment mounts a PersistentVolumeClaim named `aximo-models` at `/var/lib/aximo/models`. Populate that volume with the same layout used by Docker:

```text
/var/lib/aximo/models/
├── parakeet-tdt-0.6b-v3-int8/
└── giga-am-v3/
```

Use an init container, a one-shot Job, or your platform's artifact-sync mechanism to place model bundles there. The image does not include model files.

## Probes

The example uses:

- `GET /health/live` for liveness;
- `GET /health/ready` for readiness.

`runtime_degraded_policy = "readiness_only"` lets Kubernetes remove a degraded pod from Service endpoints without immediately failing direct in-flight clients. For standalone fail-fast behavior, set `AXIMO_RUNTIME_DEGRADED_POLICY=fail_fast_inference`. Fail-fast mode still probes recovery: after `AXIMO_RUNTIME_DEGRADED_RECOVERY_COOLDOWN_MS`, one half-open request is admitted; success clears the component, failure reopens it.

## Sizing

The default example requests 2 CPU cores and 3 GiB memory and limits the pod to 4 CPU cores and 6 GiB memory. Treat those as starting values only. Measure Parakeet and GigaAM with `docs/benchmarks.md`, then tune:

- CPU request/limit for target RTF;
- memory for model residency and decode buffers;
- replica count for throughput, because one loaded model instance has one execution slot.
