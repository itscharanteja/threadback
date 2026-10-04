// Prints far more output than ThreadBack stores, then fails on the last line.
// The exit code is set rather than calling process.exit(), which can discard
// output still queued for a slow reader.
for (let line = 1; line <= 20000; line += 1) {
  console.log(`retry trace ${String(line).padStart(5, "0")}: awaiting provider acknowledgement`);
}
console.error("FAIL noisy run: expected exactly one provider call, received two");
process.exitCode = 1;
