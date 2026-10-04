// A stand-in for a distributed lock. Each backend remembers which worker owns
// which key.
export function createLockBackend(name) {
  const owners = new Map();
  return {
    name,
    acquire(key, workerId) {
      if (owners.has(key)) return { acquired: false, owner: owners.get(key) };
      owners.set(key, workerId);
      return { acquired: true, owner: workerId };
    },
    ownerOf(key) {
      return owners.get(key) ?? null;
    },
  };
}
