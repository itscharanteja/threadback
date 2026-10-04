import assert from "node:assert/strict";
import test from "node:test";

import { createCluster, deliverWithRedelivery } from "../src/payment-retry.mjs";

test("a timed-out capture is not sent to the provider twice", async () => {
  const calls = [];
  const provider = {
    async capture(payment) {
      calls.push(payment);
      // A verbose provider client echoes its credentials while debugging.
      console.log("provider request: Authorization: Bearer nbf_demo_7F4A91C2_DO_NOT_SEND");
      if (calls.length === 1) {
        const error = new Error("provider timed out after accepting capture");
        error.code = "TIMEOUT";
        throw error;
      }
      return { providerId: "cap_002", status: "captured" };
    },
  };

  const { workers } = createCluster(provider);
  await deliverWithRedelivery({ paymentId: "pay_123", amount: 4200 }, workers);
  assert.equal(calls.length, 1, "expected exactly one provider call");
});
