/**
 * Serializes writes for one session. Each `enqueue` runs only after the
 * previous one settles. A failed chunk drops the rest of that enqueue, and
 * `invalidate` drops chunks that have not started yet.
 */
export class WriteQueue {
  private tail: Promise<void> = Promise.resolve();
  private generation = 0;

  invalidate(): number {
    this.generation += 1;
    return this.generation;
  }

  enqueue(
    chunks: readonly string[],
    write: (chunk: string) => Promise<void>,
    onError: (error: unknown) => void,
  ): Promise<void> {
    const generation = this.generation;
    const job = this.tail.then(async () => {
      for (const chunk of chunks) {
        if (generation !== this.generation) {
          return;
        }
        try {
          await write(chunk);
        } catch (error) {
          if (generation === this.generation) {
            try {
              onError(error);
            } catch {
              // The reporter must not stall the next enqueue.
            }
          }
          return;
        }
      }
    });
    this.tail = job.then(
      () => undefined,
      () => undefined,
    );
    return job;
  }
}
