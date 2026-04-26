# Deployment Security

Aximo is designed as a self-hosted STT service for trusted infrastructure. The service does not enable built-in end-user authentication by default because deployments usually already terminate TLS, identity, and quota policy at an ingress gateway, API gateway, or service mesh.

## Boundary Policy

Do not expose Aximo directly to the public internet without an authenticated ingress layer. Short-audio and realtime STT endpoints are CPU/RAM-expensive even with request limits, so external deployments need both ingress authentication and rate limiting.

Use one of these controls at the edge:

- API keys, JWT, OAuth2/OIDC, or mTLS for caller identity.
- Per-token or per-client-IP rate limiting for `POST /v1/transcriptions` and `GET /v1/realtime`.
- Request body limits at the ingress that are no higher than `AXIMO_MAX_SHORT_AUDIO_BYTES`.
- WebSocket connection limits that are no higher than `AXIMO_MAX_REALTIME_SESSIONS` per Aximo replica.
- Private networking or allowlists for `/metrics`; keep `/health/live` and `/health/ready` reachable only by the orchestrator.

## Kubernetes Ingress Pattern

For Kubernetes, keep the Aximo `Service` as `ClusterIP` and put authentication/rate limiting on the ingress controller or gateway. The bundled `NetworkPolicy` is intentionally conservative but generic; in production, narrow it to the namespace and labels used by your ingress controller.

Example policy decisions:

- Internal-only deployment: expose only inside the cluster or private VPC.
- Public demo: require an API key or OIDC login at ingress and set low per-IP quotas.
- Production API: use JWT/OIDC or mTLS, per-tenant quotas, access logs, and alerting on `aximo_http_requests_total{code=...}` and `aximo_inference_timeouts_total`.

## Rate-Limit Starting Points

Tune these with real benchmarks rather than copying fixed numbers:

- Short audio: start around `1-2` concurrent short requests per model replica unless benchmarks prove more capacity.
- Realtime: start below `max_realtime_sessions` and reserve headroom for reconnects.
- Burst control: keep ingress burst size small enough that rejected requests happen before bodies are uploaded to Aximo.

The in-process limits remain the safety net. Ingress limits are the first line of defense for untrusted clients.
