# Auth service

gRPC authentication microservice. Handles user registration, login, JWT token management, and OAuth 2.0 flows

## Features

- User registration and login with email/password (Argon2 hashing)
- JWT access & refresh tokens\*\* with automatic rotation
- OAuth 2.0 support for Google and GitHub
- Refresh token management - max 10 sessions per user, automatic cleanup of expired tokens
- gRPC API via tonic with protobuf
- PostgreSQL storage with sqlx (compile-time checked queries)
- Graceful shutdown with SIGTERM/SIGINT handling

## gRPC API

| Method              | Description                                   |
| ------------------- | --------------------------------------------- |
| `Register`          | Create account with email, username, password |
| `Login`             | Authenticate with email and password          |
| `RefreshToken`      | Exchange refresh token for new token pair     |
| `ValidateToken`     | Validate access token, returns user info      |
| `Logout`            | Revoke a refresh token                        |
| `OAuthGetAuthUrl`   | Get authorization URL for Google/GitHub       |
| `OAuthAuthenticate` | Exchange OAuth code for tokens                |

## Local launch

```bash
# 1. start the database
docker compose up -d

# 2. set up environment variables
cp .env.example .env

# 3. run migrations
sqlx migrate run

# 4. run the service
cargo run --release
```

The gRPC server starts on port `50051` by default

## Environment variables

| Variable                  | Required | Default | Description                         |
| ------------------------- | -------- | ------- | ----------------------------------- |
| `GRPC_PORT`               | yes      | —       | gRPC server port                    |
| `DATABASE_URL`            | yes      | —       | PostgreSQL connection string        |
| `JWT_SECRET`              | yes      | —       | Secret key for signing JWT tokens   |
| `JWT_ACCESS_EXPIRATION`   | yes      | —       | Access token lifetime (seconds)     |
| `JWT_REFRESH_EXPIRATION`  | yes      | —       | Refresh token lifetime (seconds)    |
| `DB_MAX_CONNECTIONS`      | no       | 10      | Max database pool connections       |
| `DB_MIN_CONNECTIONS`      | no       | 2       | Min database pool connections       |
| `DB_ACQUIRE_TIMEOUT_SECS` | no       | 5       | Database connection acquire timeout |
| `GOOGLE_CLIENT_ID`        | no       | —       | Google OAuth client ID              |
| `GOOGLE_CLIENT_SECRET`    | no       | —       | Google OAuth client secret          |
| `GITHUB_CLIENT_ID`        | no       | —       | GitHub OAuth client ID              |
| `GITHUB_CLIENT_SECRET`    | no       | —       | GitHub OAuth client secret          |

## Database schema

- **users** — user accounts (email, username, optional password for OAuth users)
- **refresh_tokens** — active refresh tokens with expiration, max 10 per user
- **oauth_accounts** — linked OAuth providers (Google, GitHub) per user
- **channels** — communication channels
- **channel_subscribers** — channel membership with ownership flag
- **contacts** — user contact list
