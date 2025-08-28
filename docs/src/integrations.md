# Integrations

The `smith-api` includes several pre-built integrations that can be easily configured to enhance your application.

## Available Integrations

These integrations are optional and can be enabled by setting specific environment variables in your deployment configuration.

### [Sentry API Error Reporting](https://sentry.io/)

**Purpose:** Provides real-time error tracking and monitoring for your API.

**Configuration:**
- Set the `SENTRY_URL` environment variable with your Sentry DSN (Data Source Name)
- Example: `SENTRY_URL=https://abc123@sentry.io/123456`

**Benefits:**
- Automatically captures and reports API exceptions
- Tracks performance issues
- Provides detailed error context for faster debugging

### [Slack Notifications](http://slack.com/)

**Purpose:** Sends automated notifications to your Slack workspace when important events occur.

**Configuration:**
- Set the `SLACK_HOOK_URL` environment variable with your Slack Incoming Webhook URL
- Example: `SLACK_HOOK_URL=https://hooks.slack.com/services/T00000000/B00000000/XXXXXXXXXXXXXXXXXXXXXXXX`

**Benefits:**
- Real-time alerts when new devices register with the API
- Keep your team informed of system activity
- Customize notification content in Slack's webhook settings

### [Victoria Metrics](https://victoriametrics.com/)

**Purpose:** Time-series database and monitoring solution for collecting and visualizing API metrics.

**Configuration:**
- Set the `VICTORIA_METRICS_URL` environment variable with your Victoria Metrics instance URL
- Set the `VICTORIA_METRICS_AUTH_TOKEN` environment variable with your authentication token
- Example:
  ```
  VICTORIA_METRICS_URL=https://your-vm-instance.example.com
  VICTORIA_METRICS_AUTH_TOKEN=your-auth-token
  ```

**Benefits:**
- High-performance metrics collection
- Long-term storage of monitoring data
- Compatible with Prometheus querying and visualization tools

### [IP-API Geolocation](https://ip-api.com/)

**Purpose:** Automatically enriches device IP addresses with geolocation data including country, city, ISP, and coordinates.

**Configuration:**
- Set the `IP_API_KEY` environment variable with your IP-API Pro key
- Example: `IP_API_KEY=your-pro-api-key`

**Features:**
- **Smart Updates:** Only updates geolocation data when it's older than 24 hours, minimizing API calls
- **Background Processing:** Geolocation lookups happen asynchronously without blocking device ping responses
- **Comprehensive Data:** Collects country, city, region, ISP, coordinates, proxy/hosting detection
- **Graceful Fallback:** When no API key is configured, only stores IP addresses without geolocation data

**Database Schema:**
The system automatically stores geolocation data in the `ip_address` table with the following fields:
- `continent`, `continent_code`
- `country_code`, `country`
- `region`, `city`
- `isp`
- `coordinates` (PostgreSQL POINT type for latitude/longitude)
- `proxy`, `hosting` (boolean flags)
- `created_at`, `updated_at` (automatic timestamps)

**Benefits:**
- Track device geographical distribution
- Identify unusual network activity (proxy/hosting detection)
- Generate location-based analytics and insights
- Minimal impact on API performance due to smart caching

## Implementation Example

Add these environment variables to your deployment configuration:

```bash
# Error Reporting
SENTRY_URL=https://your-sentry-dsn

# Event Notifications
SLACK_HOOK_URL=https://hooks.slack.com/services/your-webhook-url

# Metrics and Monitoring
VICTORIA_METRICS_URL=https://your-vm-instance.example.com
VICTORIA_METRICS_AUTH_TOKEN=your-auth-token

# IP Geolocation
IP_API_KEY=your-pro-api-key
```

## Additional Information

For more details on configuring these integrations or for troubleshooting, refer to each provider's documentation linked above.
