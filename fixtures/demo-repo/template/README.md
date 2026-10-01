# Duplicate capture investigation

This deterministic fixture reproduces a retry that sends two provider capture
requests after a simulated timeout. The focused test is intentionally failing.

```bash
npm run test:retry
```

The file under `logs/` contains a fake secret used only to prove that
ThreadBack blocks sensitive output before an outbound model request.
