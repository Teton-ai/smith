---
title: Integrations
description: Optional services the API connects to when their environment variables are set.
---

The Smith API includes several pre-built integrations. They are all optional and are enabled by setting environment variables in your deployment configuration.

## Available integrations

### Sentry

[Sentry](https://sentry.io/) provides real-time error tracking and monitoring for the API.

**Configuration:**
- Set `SENTRY_URL` to your Sentry DSN (Data Source Name), e.g. `SENTRY_URL=https://abc123@sentry.io/123456`

**Benefits:**
- Automatically captures and reports API exceptions
- Tracks performance issues
- Provides detailed error context for faster debugging

### Slack

[Slack](https://slack.com/) notifications are sent to your workspace when important events occur.

**Configuration:**
- Set `SLACK_HOOK_URL` to your Slack Incoming Webhook URL, e.g. `SLACK_HOOK_URL=https://hooks.slack.com/services/T00000000/B00000000/XXXXXXXXXXXXXXXXXXXXXXXX`

**Benefits:**
- Real-time alerts when new devices register with the API
- Keep your team informed of system activity
- Customize notification content in Slack's webhook settings

### Victoria Metrics

[Victoria Metrics](https://victoriametrics.com/) is a time-series database for collecting and visualizing API metrics.

**Configuration:**
- Set `VICTORIA_METRICS_URL_FLEET` to your Victoria Metrics instance URL
- Set `VICTORIA_METRICS_AUTH_TOKEN_FLEET` to your authentication token

```bash
VICTORIA_METRICS_URL_FLEET=https://your-vm-instance.example.com
VICTORIA_METRICS_AUTH_TOKEN_FLEET=your-auth-token
```

**Benefits:**
- High-performance metrics collection
- Long-term storage of monitoring data
- Compatible with Prometheus querying and visualization tools

### IP-API geolocation

[IP-API](https://ip-api.com/) enriches device IP addresses with geolocation data including country, city, ISP and coordinates.

**Configuration:**
- Set `IP_API_KEY` to your IP-API Pro key, e.g. `IP_API_KEY=your-pro-api-key`

**Features:**
- **Smart updates** — only refreshes geolocation data older than 24 hours, minimizing API calls
- **Background processing** — lookups happen asynchronously without blocking device ping responses
- **Comprehensive data** — collects country, city, region, ISP, coordinates, proxy/hosting detection
- **Graceful fallback** — without an API key, only IP addresses are stored

**Database schema:** geolocation data is stored in the `ip_address` table with the following fields:
- `continent`, `continent_code`
- `country_code`, `country`
- `region`, `city`
- `isp`
- `coordinates` (PostgreSQL `POINT` for latitude/longitude)
- `proxy`, `hosting` (boolean flags)
- `created_at`, `updated_at` (automatic timestamps)

**Benefits:**
- Track device geographical distribution
- Identify unusual network activity (proxy/hosting detection)
- Generate location-based analytics and insights
- Minimal impact on API performance due to smart caching

## Example configuration

```bash
# Error reporting
SENTRY_URL=https://your-sentry-dsn

# Event notifications
SLACK_HOOK_URL=https://hooks.slack.com/services/your-webhook-url

# Metrics and monitoring
VICTORIA_METRICS_URL_FLEET=https://your-vm-instance.example.com
VICTORIA_METRICS_AUTH_TOKEN_FLEET=your-auth-token

# IP geolocation
IP_API_KEY=your-pro-api-key
```

For more details on configuring these integrations or for troubleshooting, refer to each provider's documentation linked above.
