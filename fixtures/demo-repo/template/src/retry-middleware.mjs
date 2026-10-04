// Retries a provider call when the provider asks us to slow down. A timeout is
// not retried here: the job goes back to the queue instead.
export const retryPolicy = {
  maxAttempts: 2,
  backoffMs: 0,
  retryOn: ["RATE_LIMITED"],
};

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export async function withRetry(operation, policy = retryPolicy) {
  for (let attempt = 1; ; attempt += 1) {
    try {
      return await operation();
    } catch (error) {
      const retryable = policy.retryOn.includes(error.code);
      if (!retryable || attempt >= policy.maxAttempts) throw error;
      await sleep(policy.backoffMs * attempt);
    }
  }
}
