import assert from "node:assert/strict";
import test from "node:test";

import { createCluster } from "../src/payment-retry.mjs";

test("both workers observe the same lock owner", async () => {
  const provider = { async capture() { return { status: "captured" }; } };
  const { workers } = createCluster(provider);
  const [first, second] = workers;

  await first.process({ paymentId: "pay_123" });
  await second.process({ paymentId: "pay_123" });

  assert.equal(
    second.lock.ownerOf("pay_123"),
    first.lock.ownerOf("pay_123"),
    `worker-b sees owner ${second.lock.ownerOf("pay_123")} on ${second.lock.name}, ` +
      `worker-a sees ${first.lock.ownerOf("pay_123")} on ${first.lock.name}`,
  );
});
