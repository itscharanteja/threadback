export async function captureWithRetry(provider, payment) {
  try {
    return await provider.capture(payment);
  } catch (error) {
    if (error.code !== "TIMEOUT") throw error;
    // The timeout may occur after the provider accepted the capture. Retrying
    // without an idempotency key reproduces the duplicate-capture defect.
    return provider.capture(payment);
  }
}
