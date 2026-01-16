# Test package

A simple package used for local testing.

```
cd test-packages/test-package
docker build -t test-package-builder .
docker run --rm -v "$(pwd)/out:/out" test-package-builder
```
