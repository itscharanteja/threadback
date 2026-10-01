import assert from "node:assert/strict";
import test from "node:test";

import { captureWithRetry } from "../src/payment-retry.mjs";

test("a timed-out capture is not sent to the provider twice", async () => {
  const calls = [];
  const provider = {
    async capture(payment) {
      calls.push(payment);
      if (calls.length === 1) {
        const error = new Error("provider timed out after accepting capture");
        error.code = "TIMEOUT";
        throw error;
      }
      return { providerId: "cap_002", status: "captured" };
    },
  };

  await captureWithRetry(provider, { paymentId: "pay_123", amount: 4200 });
  assert.equal(calls.length, 1, "expected exactly one provider call");
});
