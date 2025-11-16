# HTTP Timeout Configuration

This document explains how to configure HTTP timeouts in Codex to optimize performance and reliability when communicating with different LLM providers.

## Overview

Codex provides three types of HTTP timeout configurations:

1. **HTTP Connection Timeout** (`http_connect_timeout_ms`) - Maximum time to establish a TCP connection
2. **HTTP Request Total Timeout** (`http_request_timeout_ms`) - Maximum total duration for an entire HTTP request
3. **SSE Stream Idle Timeout** (`stream_idle_timeout_ms`) - Maximum idle time between Server-Sent Events chunks

## Configuration Hierarchy

Timeouts are resolved using a **3-tier priority system** (highest to lowest):

```
Per-Provider Override → Global Config → Environment Variable → Hardcoded Default
```

### Priority Details

1. **Per-Provider Override** - Set in provider-specific configuration (highest priority)
2. **Global Config** - Set in `~/.codex/config.toml` or profile
3. **Environment Variable** - Set via `CODEX_HTTP_*_TIMEOUT_MS` (for connection and request timeouts only)
4. **Hardcoded Default** - Built-in fallback values (lowest priority)

## Default Values

| Timeout Type | Default Value | Environment Variable | Per-Provider Field |
|-------------|---------------|---------------------|-------------------|
| Connection Timeout | 30,000 ms (30s) | `CODEX_HTTP_CONNECT_TIMEOUT_MS` | N/A |
| Request Total Timeout | 600,000 ms (10min) | `CODEX_HTTP_REQUEST_TIMEOUT_MS` | `request_timeout_ms` |
| Stream Idle Timeout | 300,000 ms (5min) | N/A | `stream_idle_timeout_ms` |

## Configuration Examples

### Global Configuration

Configure global defaults in `~/.codex/config.toml`:

```toml
# Global HTTP connection timeout (30 seconds)
http_connect_timeout_ms = 30000

# Global HTTP request total timeout (10 minutes)
http_request_timeout_ms = 600000

# Global SSE stream idle timeout (5 minutes)
stream_idle_timeout_ms = 300000
```

### Per-Provider Configuration

Override timeouts for specific providers in `~/.codex/providers/`:

**Example: Fast provider (shorter timeouts)**

```toml
# ~/.codex/providers/fast-gateway.toml
[[providers]]
name = "fast-gateway"
display_name = "Fast Gateway"
base_url = "https://fast.example.com/v1"
adapter = "gpt_openapi"

# Override: faster provider needs shorter timeouts
request_timeout_ms = 180000        # 3 minutes
stream_idle_timeout_ms = 60000     # 1 minute
```

**Example: Slow provider (longer timeouts)**

```toml
# ~/.codex/providers/slow-gateway.toml
[[providers]]
name = "slow-gateway"
display_name = "Slow Gateway"
base_url = "https://slow.example.com/v1"
adapter = "gpt_openapi"

# Override: slower provider needs longer timeouts
request_timeout_ms = 900000         # 15 minutes
stream_idle_timeout_ms = 600000     # 10 minutes
```

### Profile-Based Configuration

Use profiles to manage different timeout configurations:

```toml
# ~/.codex/config.toml

# Default global settings
http_connect_timeout_ms = 30000
http_request_timeout_ms = 600000
stream_idle_timeout_ms = 300000

[profiles.fast]
# Profile for fast providers
http_request_timeout_ms = 180000
stream_idle_timeout_ms = 60000

[profiles.slow]
# Profile for slow providers
http_request_timeout_ms = 900000
stream_idle_timeout_ms = 600000
```

Activate a profile with:
```bash
codex --profile fast
```

### Environment Variable Configuration

Set timeouts at runtime without modifying config files:

```bash
# Set connection timeout to 10 seconds
export CODEX_HTTP_CONNECT_TIMEOUT_MS=10000

# Set request timeout to 5 minutes
export CODEX_HTTP_REQUEST_TIMEOUT_MS=300000

# Launch codex (environment variables take effect)
codex
```

**Note**: Environment variables only work for `http_connect_timeout_ms` and `http_request_timeout_ms`. The `stream_idle_timeout_ms` must be configured via TOML files.

## Timeout Behavior Details

### Connection Timeout

- **Controls**: TCP connection establishment phase
- **Applies to**: Initial handshake with the LLM provider
- **Typical values**: 10-30 seconds
- **When to adjust**:
  - Increase for providers with slow network routing
  - Decrease for fast local or same-region providers

### Request Total Timeout

- **Controls**: Entire HTTP request from start to finish
- **Applies to**: Complete request-response cycle including streaming
- **Typical values**: 5-15 minutes
- **When to adjust**:
  - Increase for long-running tasks (code generation, analysis)
  - Decrease for quick queries (autocomplete, simple Q&A)
- **Note**: Can be overridden per-provider using `request_timeout_ms` field

### Stream Idle Timeout

- **Controls**: Maximum time between SSE chunks
- **Applies to**: Server-Sent Events streaming responses
- **Typical values**: 1-10 minutes
- **When to adjust**:
  - Increase for providers with variable response speeds
  - Decrease for providers with consistent streaming rates
- **Note**: Can be overridden per-provider using `stream_idle_timeout_ms` field

## Use Cases

### Use Case 1: Multiple Providers with Different Performance

**Scenario**: Using both OpenAI (fast) and a self-hosted model (slow)

```toml
# ~/.codex/config.toml - Global defaults for OpenAI
http_request_timeout_ms = 300000    # 5 minutes
stream_idle_timeout_ms = 120000     # 2 minutes

# ~/.codex/providers/self-hosted.toml
[[providers]]
name = "self-hosted"
base_url = "http://localhost:8080/v1"
adapter = "gpt_openapi"
request_timeout_ms = 1800000        # 30 minutes (override)
stream_idle_timeout_ms = 600000     # 10 minutes (override)
```

### Use Case 2: Network-Constrained Environment

**Scenario**: Running on slow or unreliable network

```toml
# ~/.codex/config.toml
http_connect_timeout_ms = 60000     # 1 minute (increased)
http_request_timeout_ms = 900000    # 15 minutes (increased)
stream_idle_timeout_ms = 600000     # 10 minutes (increased)
```

### Use Case 3: CI/CD with Strict Time Limits

**Scenario**: Automated testing with 5-minute timeout limit

```bash
# Set aggressive timeouts via environment variables
export CODEX_HTTP_CONNECT_TIMEOUT_MS=5000      # 5 seconds
export CODEX_HTTP_REQUEST_TIMEOUT_MS=270000    # 4.5 minutes
```

## Troubleshooting

### "Connection timeout" Error

**Symptom**: `Connection timeout after X ms`

**Possible causes**:
- Network routing issues
- Provider server is down
- Firewall blocking connection

**Solutions**:
1. Check network connectivity: `ping provider.example.com`
2. Increase `http_connect_timeout_ms` in config
3. Try different provider or network

### "Request timeout" Error

**Symptom**: `Request timeout after X ms`

**Possible causes**:
- Request took longer than configured timeout
- Provider is processing slowly
- Large response generation

**Solutions**:
1. Increase `http_request_timeout_ms` globally or per-provider
2. Use faster model variant
3. Split large requests into smaller chunks

### "Stream idle timeout" Error

**Symptom**: `Stream idle timeout - no data received for X ms`

**Possible causes**:
- Provider stopped streaming mid-response
- Network interruption
- Provider-side rate limiting

**Solutions**:
1. Increase `stream_idle_timeout_ms` for the specific provider
2. Check provider status/rate limits
3. Retry the request

### Debugging Timeout Configuration

To verify which timeout values are being used:

```bash
# Enable debug logging
RUST_LOG=debug codex

# Check for timeout-related log messages:
# - "HTTP client created with connect_timeout=..., request_timeout=..."
# - "Using effective request timeout: ..."
# - "Using effective stream idle timeout: ..."
```

## Best Practices

1. **Start with defaults**: Use default values unless you have specific performance issues

2. **Monitor and adjust**: Track timeout errors and adjust based on actual provider behavior

3. **Use per-provider overrides**: Set specific timeouts for problematic providers instead of changing global defaults

4. **Profile-based configuration**: Use profiles for different use cases (e.g., `--profile fast` for quick queries, `--profile thorough` for analysis)

5. **Environment variables for testing**: Use environment variables to test different timeout values before committing to config files

6. **Consider retry settings**: Timeouts work together with retry settings (`request_max_retries`, `stream_max_retries`) - adjust both for optimal behavior

## Related Configuration

- **Retry Settings**: See `request_max_retries` and `stream_max_retries` in config
- **Backoff Algorithm**: Exponential backoff with jitter (200ms * 2^(attempt-1) * random(0.9, 1.1))
- **Provider Configuration**: See `docs/providers.md` for provider-specific settings

## Implementation Notes

For developers working on timeout-related code:

- Connection and request timeouts are set at HTTP client level (`core/src/default_client.rs`)
- Per-request timeout override uses `reqwest::RequestBuilder::timeout()`
- Stream idle timeout is enforced in SSE processing (`core/src/adapters/http.rs`)
- Timeout resolution logic is in `ModelProviderInfo::effective_*_timeout()` methods
- Environment variable parsing is in `default_client.rs::get_*_timeout()` functions
