# Development Guide

This guide will help you get started developing Smith, the open-source fleet management system.

## Prerequisites

Before you begin, ensure you have the following installed:

### Required
- **Docker & Docker Compose** - For running the full stack locally
- **Rust** (1.75+) - The workspace uses Rust 2024 edition
- **Node.js** (18+) - For the dashboard frontend
- **PostgreSQL Client** - For database operations (`psql` command)

### Optional but Recommended  
- **1Password CLI** - For accessing shared development secrets
- **Git** - For version control (obviously)

## Quick Start

1. **Clone the repository**
   ```bash
   git clone https://github.com/teton-ai/smith.git
   cd smith
   ```

2. **Initialize the project**
   ```bash
   make init
   ```
   This creates the necessary `.env` files from templates. You'll need to fill in missing values (see [Configuration](#configuration) below).

3. **Start the backend services**
   ```bash
   make up
   ```
   This starts:
   - PostgreSQL database
   - API server (port 8080)
   - Simulated device running smithd
   - Bore tunnel server

4. **Seed the database**
   ```bash
   make seed
   ```

5. **Start the dashboard**
   ```bash
   cd dashboard
   npm install
   npm run dev
   ```
   Dashboard runs on http://localhost:3000

6. **Test the setup**
   ```bash
   curl http://localhost:8080/health
   # Should return: I'm good: <version>
   ```

## Project Structure

```
smith/
├── api/                    # Rust API service
│   ├── src/               # API source code
│   ├── migrations/        # Database migrations
│   ├── seeds/            # Database seed files
│   ├── .env              # API environment config
│   └── .env.template     # API environment template
├── smithd/               # Rust daemon (runs on devices)
│   ├── src/              # Smithd source code  
│   └── debian/           # Debian packaging
├── updater/              # Rust updater daemon
├── cli/                  # Rust CLI tool
├── models/               # Shared Rust models/types
├── dashboard/            # React dashboard (Next.js routing only)
│   ├── .env             # Dashboard environment config
│   └── .env.template    # Dashboard environment template
├── docker/               # All Dockerfiles
│   ├── api.Dockerfile
│   ├── device.Dockerfile
│   └── postgres.Dockerfile
└── scripts/              # Utility scripts
    ├── init-db.sh
    └── install.sh
```

## Configuration

### API Configuration (`api/.env`)

Copy from `api/.env.template` and fill in:

```bash
DATABASE_URL=postgres://postgres:postgres@postgres:5432/postgres

# AWS S3 buckets (required for package/asset storage)
PACKAGES_BUCKET_NAME=your-packages-bucket
ASSETS_BUCKET_NAME=your-assets-bucket  
AWS_REGION=eu-north-1

# Auth0 configuration (required for authentication)
AUTH0_ISSUER=https://your-domain.auth0.com
AUTH0_AUDIENCE=https://your-api-audience
AUTH0_DOMAIN=your-domain.auth0.com
AUTH0_CLIENT_ID=your-client-id
AUTH0_REDIRECT_URI=http://localhost:3000

# Optional: CloudFront for CDN
CLOUDFRONT_DOMAIN_NAME=""
CLOUDFRONT_PACKAGE_KEY_PAIR_ID=""
CLOUDFRONT_PACKAGE_PRIVATE_KEY=""

# Development/debugging
RUST_LOG=debug
SENTRY_URL=your-sentry-dsn (optional)
```

### Dashboard Configuration (`dashboard/.env`)

Copy from `dashboard/.env.template`:

```bash
API_BASE_URL=http://localhost:8080
AUTH0_DOMAIN=your-domain.auth0.com
AUTH0_CLIENT_ID=your-client-id  
AUTH0_REDIRECT_URI=http://localhost:3000
AUTH0_AUDIENCE=https://your-api-audience
```

## Development Workflows

### Backend Development

**Running individual services:**
```bash
# API only
cargo run --bin api

# Smithd daemon  
cargo run --bin smithd

# CLI tool
cargo run --bin sm -- --help
```

**Database operations:**
```bash
# Run migrations
cd api && sqlx migrate run

# Reset database
make db-reset  # if this target exists, otherwise manually drop/create

# Seed with test data
make seed
```

**Testing:**
```bash
# Run all tests
cargo test

# Run API tests only  
cargo test -p api

# Run with logs
RUST_LOG=debug cargo test
```

### Frontend Development

**Dashboard development:**
```bash
cd dashboard
npm run dev          # Development server
npm run build        # Production build
npm run gen-api-client  # Regenerate API client from OpenAPI spec
```

### Docker Development

**Build individual services:**
```bash
# API service
docker compose build api

# Device simulator  
docker compose build device

# All services
docker compose build
```

**View logs:**
```bash
# All services
docker compose logs -f

# Specific service
docker compose logs -f api
docker compose logs -f postgres
```

### Database Schema

The database schema is managed through SQLx migrations in `api/migrations/`. The `schema.sql` file is the source of truth for the current schema.

**Working with migrations:**
```bash
cd api

# Create new migration
sqlx migrate add your_migration_name

# Run migrations
sqlx migrate run  

# Revert last migration
sqlx migrate revert
```

## Common Development Tasks

### Adding New Features

1. **Create a new branch** from main
2. **Update the models** in `models/` if adding new data types
3. **Add database migrations** if schema changes are needed
4. **Implement backend logic** in the appropriate service (`api/`, `smithd/`, etc.)
5. **Update the frontend** in `dashboard/` if UI changes are needed
6. **Add tests** for your changes
7. **Update documentation** as needed

### Debugging

**Enable verbose logging:**
```bash
export RUST_LOG=debug
make up
```

**Connect to database directly:**
```bash
psql postgres://postgres:postgres@localhost:5432/postgres
```

**View API documentation:**
Visit http://localhost:8080/swagger-ui/ when the API is running.

### Package Management

**Rust dependencies:**
```bash
# Add new dependency to workspace
cargo add <package> --workspace

# Add to specific crate
cargo add <package> -p api
```

**Node.js dependencies:**
```bash
cd dashboard
npm install <package>
npm install --save-dev <package>
```

## Troubleshooting

### Common Issues

**Port already in use:**
```bash
# Find process using port
lsof -i :8080
kill -9 <PID>

# Or use different ports in docker-compose
```

**Database connection errors:**
- Ensure PostgreSQL container is running: `docker compose ps`
- Check database URL in `api/.env`
- Verify database exists: `psql postgres://postgres:postgres@localhost:5432/postgres -c "\\l"`

**Docker build failures:**
```bash
# Clean Docker cache
docker system prune -a

# Rebuild without cache
docker compose build --no-cache
```

**Rust compilation errors:**
```bash
# Clean build artifacts
cargo clean

# Update Rust toolchain
rustup update

# Check formatting
cargo fmt --check
```

**Frontend build errors:**
```bash
cd dashboard
rm -rf node_modules package-lock.json
npm install
```

### Getting Help

- **Check existing issues** on GitHub
- **Review logs** with `docker compose logs -f`
- **Use debug logging** with `RUST_LOG=debug`
- **Ask in discussions** or create an issue

## Contributing

1. **Fork the repository** and create a feature branch
2. **Follow the coding standards** (run `cargo fmt`)
3. **Add tests** for new functionality
4. **Update documentation** if needed
5. **Submit a pull request** with a clear description

### Code Style

- **Rust**: Use `cargo fmt` and follow project conventions
- **React/TypeScript**: Use project ESLint/Prettier configuration  
- **Commits**: Use conventional commit messages
- **PRs**: Include clear descriptions and link related issues

## Architecture Notes

- **smithd** is the critical component that runs on devices - handle with extra care
- **Next.js** is used for routing only - all dashboard code runs client-side
- **Database schema** in `schema.sql` is the source of truth, not migrations
- **No panic calls** in Rust code - use `?` for error propagation
- **Environment variables** are service-specific, not global

## Performance Tips

- **Use `cargo build --release`** for production builds
- **Enable SQLx offline mode** with `SQLX_OFFLINE=true` for faster builds  
- **Use Docker layer caching** in CI/CD
- **Profile with `cargo flamegraph`** for performance bottlenecks