# OpenTelemetry Integration

Git-AI supports exporting metrics to OpenTelemetry-compatible backends (like Grafana, Prometheus, Jaeger) via the OTLP protocol. This enables visualization of AI code generation metrics in dashboards.

## Prerequisites

To use OpenTelemetry export, you need to build git-ai with the `otel` feature enabled:

```bash
cargo build --release --features otel
```

## Configuration

OpenTelemetry export can be configured via environment variables or the config file (`~/.git-ai/config.json`).

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `GIT_AI_OTEL_ENABLED` | Enable OTel export (`1`, `true`, or `false`) | `false` |
| `GIT_AI_OTEL_ENDPOINT` | OTLP gRPC endpoint URL | `http://localhost:4317` |
| `GIT_AI_OTEL_EXPORT_INTERVAL` | Export interval in seconds | `60` |

### Config File

Add the following to `~/.git-ai/config.json`:

```json
{
  "otel_enabled": true,
  "otel_endpoint": "http://localhost:4317",
  "otel_export_interval_secs": 60
}
```

Environment variables take precedence over config file settings.

## Exported Metrics

Git-AI exports the following metrics to OpenTelemetry:

### Committed Metrics (on git commit)

| Metric Name | Type | Description |
|-------------|------|-------------|
| `git_ai.committed.human_additions` | Counter | Number of human-written lines committed |
| `git_ai.committed.ai_additions` | Counter | Number of AI-generated lines committed |
| `git_ai.committed.diff_added` | Counter | Total lines added in git diff |
| `git_ai.committed.diff_deleted` | Counter | Total lines deleted in git diff |
| `git_ai.committed.ai_accepted` | Counter | Number of AI-generated lines accepted into commit |

### Agent Usage Metrics (on AI tool usage)

| Metric Name | Type | Description |
|-------------|------|-------------|
| `git_ai.agent_usage.count` | Counter | Number of AI agent usage events |

### Checkpoint Metrics (on checkpoint creation)

| Metric Name | Type | Description |
|-------------|------|-------------|
| `git_ai.checkpoint.count` | Counter | Number of checkpoint events |
| `git_ai.checkpoint.lines_added` | Histogram | Lines added per checkpoint |
| `git_ai.checkpoint.lines_deleted` | Histogram | Lines deleted per checkpoint |

### Common Attributes

All metrics include the following attributes when available:

| Attribute | Description |
|-----------|-------------|
| `repo_url` | Repository URL |
| `author` | Commit author |
| `commit_sha` | Commit SHA |
| `base_commit_sha` | Base commit SHA |
| `branch` | Git branch name |
| `tool` | AI tool name (e.g., "cursor", "claude-code", "copilot") |
| `model` | AI model name |
| `prompt_id` | Prompt identifier |

## Setting Up with Grafana

### 1. Run OpenTelemetry Collector

Create a `otel-collector-config.yaml`:

```yaml
receivers:
  otlp:
    protocols:
      grpc:
        endpoint: 0.0.0.0:4317

exporters:
  prometheus:
    endpoint: "0.0.0.0:8889"

service:
  pipelines:
    metrics:
      receivers: [otlp]
      exporters: [prometheus]
```

Run the collector:

```bash
docker run -p 4317:4317 -p 8889:8889 \
  -v $(pwd)/otel-collector-config.yaml:/etc/otel-collector-config.yaml \
  otel/opentelemetry-collector:latest \
  --config=/etc/otel-collector-config.yaml
```

### 2. Configure Prometheus

Add the OTel Collector as a scrape target in `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'otel-collector'
    static_configs:
      - targets: ['localhost:8889']
```

### 3. Configure Grafana

1. Add Prometheus as a data source in Grafana
2. Create a new dashboard
3. Add panels with queries like:

```promql
# Total AI lines committed over time
sum(rate(git_ai_committed_ai_additions_total[5m])) by (tool)

# Human vs AI additions ratio
sum(git_ai_committed_human_additions_total) / sum(git_ai_committed_ai_additions_total)

# Agent usage by tool
sum(rate(git_ai_agent_usage_count_total[1h])) by (tool)

# Checkpoint activity
sum(rate(git_ai_checkpoint_count_total[1h])) by (repo_url)
```

## Docker Compose Example

Here's a complete Docker Compose setup for local development:

```yaml
version: '3.8'

services:
  otel-collector:
    image: otel/opentelemetry-collector:latest
    command: ["--config=/etc/otel-collector-config.yaml"]
    volumes:
      - ./otel-collector-config.yaml:/etc/otel-collector-config.yaml
    ports:
      - "4317:4317"   # OTLP gRPC
      - "8889:8889"   # Prometheus metrics

  prometheus:
    image: prom/prometheus:latest
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml
    ports:
      - "9090:9090"

  grafana:
    image: grafana/grafana:latest
    ports:
      - "3000:3000"
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
```

## Troubleshooting

### Metrics not appearing

1. Verify OTel is enabled:
   ```bash
   echo $GIT_AI_OTEL_ENABLED
   ```

2. Check the endpoint is reachable:
   ```bash
   curl -v http://localhost:4317
   ```

3. Verify git-ai was built with the `otel` feature:
   ```bash
   cargo build --release --features otel
   ```

### Performance considerations

- OTel export is non-blocking and runs in the background
- The default export interval is 60 seconds to minimize overhead
- If OTel export fails, it won't affect the existing metrics pipeline (API upload + SQLite fallback)

## Resource Attributes

The following resource attributes are set on all exported metrics:

| Attribute | Value |
|-----------|-------|
| `service.name` | `git-ai` |
| `service.version` | Current git-ai version |
