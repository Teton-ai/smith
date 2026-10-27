# Verify resumable device uploads

Use an isolated Postgres database and S3-compatible test server. Do not use a production database or bucket.
The tests use `upload-assets` and `upload-sensitive` buckets.

1. Create a local Postgres database with this fixture.

   ```sql
   CREATE TABLE device (id integer PRIMARY KEY, serial_number text, token text);
   INSERT INTO device VALUES (1, 'one', 'one-token'), (2, 'two', 'two-token');
   ```

2. Apply `api/migrations/20261027000000_device_upload_sessions.sql` to that database.
3. Start MinIO with `MINIO_DOMAIN=localhost` and test credentials. Create the two test buckets.
4. Build the Plex tests from the companion branch with `cargo test server::upload --no-run`.
5. Set `UPLOAD_TEST_PLEX_BINARY` to the test executable that Cargo prints.
6. Set `UPLOAD_TEST_DATABASE_URL` to the isolated test database.
7. Set `AWS_REGION`, `AWS_ENDPOINT`, `AWS_ACCESS_KEY_ID`, and `AWS_SECRET_ACCESS_KEY` for the test S3 server.
   For MinIO, use `http://localhost:<port>` as the endpoint. Bucket subdomains of `localhost` must resolve to loopback.
8. Run the checks from Smith.

   ```sh
   SQLX_OFFLINE=true CARGO_BUILD_JOBS=6 NEXTEST_TEST_THREADS=4 cargo test -p api smith::uploads -- --include-ignored
   SQLX_OFFLINE=true CARGO_BUILD_JOBS=6 NEXTEST_TEST_THREADS=4 cargo test -p api storage::tests
   ```

The route test checks both bucket choices, the final bytes and key, device binding, missing parts, and cleanup.
It also completes S3 before the database receipt to check recovery from a lost S3 reply.
The contract test runs the Plex test executable against the real Smith routes, Postgres, and S3.
It rejects part 2 once and discards one successful completion response.
Plex restarts its upload server after each failure. The expected part request sequence is `1, 2, 2`.

## Roll out Smith before Plex

1. Merge the abort rule in `device-upload-lifecycle.json` into each upload bucket's existing lifecycle configuration.
   Apply it to the configured assets and data-engine buckets. Keep existing rules.
   The rule only aborts incomplete multipart uploads. It does not delete complete objects.
2. Deploy Smith with its migration and session endpoints.
3. Deploy Plex with resumable uploads.

Smith aborts sessions after 14 idle days or 14 days from creation for incomplete uploads.
It keeps completed receipts for 14 idle days. Brain's durable queue has a seven-day limit.
The 30-day bucket rule also covers an S3 upload whose creation reply never reaches Smith.
Bucket lifecycle configuration belongs to the infrastructure owner. This repository supplies the rule, but does not apply it to production.

Plex uses Smith-proxied parts. Its network checks establish egress to the configured Smith host, but do not establish direct S3 access.
A failed part keeps all confirmed parts. Smith checks each part's SHA-256 and verifies the final object's SHA-256 and size.
The final check reads the object from S3 on the server side.
Plex uses a separate HTTP pool for uploads and a 120-second progress timeout.
An old Smith server returns 404 or 405 for the session route. Plex then streams the existing whole-file request from disk.

MinIO does not support this S3 lifecycle action in `PutBucketLifecycle`.
The integration tests verify Smith's explicit abort path.
The lifecycle rule uses the [AWS S3 configuration format](https://docs.aws.amazon.com/AmazonS3/latest/userguide/lifecycle-configuration-examples.html).
The [MinIO compatibility reference](https://minio.community/community/minio-object-store/reference/s3-api-compatibility.html) lists this limitation.
