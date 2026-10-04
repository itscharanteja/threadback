import { createLockBackend } from "./lock.mjs";
import { withRetry } from "./retry-middleware.mjs";

export function createWorker({ id, lock, provider }) {
  return {
    id,
    lock,
    async process(payment) {
      const claim = lock.acquire(payment.paymentId, id);
      if (process.env.LOCK_DEBUG) {
        console.log(
          `lock-debug worker=${id} backend=${lock.name} acquired=${claim.acquired} owner=${claim.owner}`,
        );
      }
      if (!claim.acquired) return { status: "skipped", owner: claim.owner };
      return withRetry(() => provider.capture(payment));
    },
  };
}

// The queue hands a failed job to the next worker.
export async function deliverWithRedelivery(job, workers) {
  let lastError;
  for (const worker of workers) {
    try {
      return await worker.process(job);
    } catch (error) {
      lastError = error;
    }
  }
  throw lastError;
}

// Production wiring for two workers.
export function createCluster(provider) {
  const workers = [
    createWorker({ id: "worker-a", lock: createLockBackend("lock-backend-1"), provider }),
    createWorker({ id: "worker-b", lock: createLockBackend("lock-backend-2"), provider }),
  ];
  return { workers };
}
