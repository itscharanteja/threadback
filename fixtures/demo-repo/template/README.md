# Duplicate capture investigation

A deterministic fixture for the ThreadBack demonstration. A capture that times
out after the provider accepted it is handed to a second worker, and the
provider is called twice.

```bash
npm run test:retry   # fails: expected exactly one provider call
npm run test:lock    # fails: the workers do not share a lock owner
LOCK_DEBUG=1 npm run test:retry   # logs which lock backend each worker uses
npm run test:noisy   # prints 20,000 lines, then fails (large-output case)
```

The retry middleware in `src/retry-middleware.mjs` looks like the cause but is
not: it never retries a timeout, so changing its policy leaves the result
unchanged. The workers are configured with separate lock backends in
`src/payment-retry.mjs`.

Terminal output from the focused test contains a fake credential
(`nbf_demo_7F4A91C2_DO_NOT_SEND`). It exists only to prove that ThreadBack
blocks sensitive output before any outbound model request.
