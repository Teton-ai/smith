# Validate resumable uploads

Use isolated Postgres and S3-compatible storage. Do not use a production database or bucket. The tests use `upload-assets` and `upload-sensitive` buckets.

1. Create a test database with this fixture.

   ```sql
   CREATE TABLE device (id integer PRIMARY KEY, serial_number text, token text);
   INSERT INTO device VALUES (1, 'one', 'one-token'), (2, 'two', 'two-token');
   ```

2. Create both test buckets. For MinIO, set `MINIO_DOMAIN=localhost` and ensure bucket subdomains of `localhost` resolve to loopback.
3. Set `UPLOAD_TEST_DATABASE_URL` for the isolated database.
4. Set `AWS_REGION`, `AWS_ENDPOINT`, `AWS_ACCESS_KEY_ID`, and `AWS_SECRET_ACCESS_KEY` for test storage.
5. Run the route, token, and storage checks.

   ```sh
   SQLX_OFFLINE=true cargo test -p api stateless_routes_verify_tokens_parts_and_completion -- --ignored
   SQLX_OFFLINE=true cargo test -p api upload_token::tests
   SQLX_OFFLINE=true cargo test -p api storage::tests
   ```

The route test calls the Axum router in process with `tower::ServiceExt::oneshot`. Tokens stay in memory. These route requests have no HTTP listener or network client. Postgres supplies device authentication fixtures. S3 supplies upload progress and object bytes.

The checks cover both bucket choices, exact final bytes and keys, device binding, token tampering and expiry, missing parts, final checksums, capacity headers, repeated completion, and `NoSuchUpload` recovery with a fresh upload. A lost completion reply is tested by completing the object directly in storage before the API completion request.

## External client contract

The optional contract test runs a client executable against a loopback test server with real upload handlers and test storage. Set `UPLOAD_TEST_CLIENT_BINARY` to an executable or wrapper that implements this contract:

- Read the API base URL from `UPLOAD_TEST_SMITH_URL` and the destination key from `UPLOAD_TEST_KEY`.
- Upload `5 * 1024 * 1024 + 27` bytes. Byte `n` is `(n % 251) as u8`. Select sensitive storage.
- Resume after one rejected part 2 and one lost completion response. Reload persisted transfer state between attempts.
- Exit successfully only after verified completion.

The test server supplies a test device credential to the handlers. It records part numbers and verifies the sequence `1, 2, 2`, the final object bytes, bucket, key, and completion URL. The executable receives no command arguments. A wrapper can supply arguments its client needs.

```sh
SQLX_OFFLINE=true cargo test -p api client_contract_resumes_across_real_smith_routes -- --ignored
```

## Protocol and deployment

Set `UPLOAD_TOKEN_SECRET` to a random secret with at least 32 bytes. `make init` and `make test.e2e` generate a stable throwaway secret for local use. All API instances must use the same secret. Keep it stable across restarts. Secret rotation invalidates outstanding upload tokens, so clients must open fresh uploads after rotation.

Deploy the upload endpoints before enabling resumable transfers in clients. Keep the existing whole-file route available during rollout. Clients can use it when the upload API is unavailable. No upload database migration is required.

`POST /smith/upload-sessions` accepts `key`, `size_bytes`, lowercase SHA-256 `checksum`, and optional `sensitive`. It uses the existing bucket selection and returns an S3 `upload_id`, `token`, and `part_size` of 5 MiB. Files must contain 1 to 512,000,000 bytes. The signed token binds the authenticated device, bucket, key, upload ID, size, checksum, and 24-hour expiry.

Send the token in `x-upload-token` with device authentication for each subsequent request. Keep tokens and signed storage URLs out of logs.

- `GET /smith/upload-sessions/status` lists confirmed S3 parts and missing part numbers. A verified finished object returns its `url`.
- `PUT /smith/upload-sessions/parts/{number}` streams one part to storage. Set `Content-Length` to 5 MiB, except for the final smaller part. The response contains the part number, ETag, and size.
- `POST /smith/upload-sessions/complete` requires a full ordered, size-correct S3 part set. It checks the completion HTTP status and embedded XML errors, then reads the final object to verify its size and SHA-256. Repeated completion succeeds when the finished object matches.

S3 is the only upload progress store. There are no upload session tables or cleanup workers. `410` with `NoSuchUpload` or `UploadTokenExpired` requires a fresh upload and retransmission. An invalid token or a different device receives `403`. Capacity rejection returns `503` with `Retry-After: 5`.

Merge `device-upload-lifecycle.json` into both upload buckets' existing lifecycle configuration. Keep existing rules. The one-day rule aborts incomplete multipart uploads, including uploads whose creation reply was lost. It does not delete complete objects. The infrastructure owner must apply the rule to deployed buckets.
