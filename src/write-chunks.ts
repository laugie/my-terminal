/** Matches the backend `write_session` limit. Chunks are counted in UTF-8 bytes. */
export const MAX_WRITE_BYTES = 64 * 1024;

/**
 * Split `text` into pieces that each encode to at most `MAX_WRITE_BYTES`.
 * A chunk never starts or ends in the middle of a UTF-8 character.
 * An empty string produces no chunks, so callers do not issue a write.
 */
export function splitWriteChunks(text: string): string[] {
  const encoded = new TextEncoder().encode(text);
  if (encoded.byteLength === 0) {
    return [];
  }
  if (encoded.byteLength <= MAX_WRITE_BYTES) {
    return [text];
  }

  const chunks: string[] = [];
  const decoder = new TextDecoder();
  let offset = 0;
  while (offset < encoded.byteLength) {
    let end = Math.min(offset + MAX_WRITE_BYTES, encoded.byteLength);
    // `end` is exclusive. If it lands on a continuation byte, walk back to
    // the lead byte and leave the whole character for the next chunk.
    if (end < encoded.byteLength) {
      while (end > offset) {
        const byte = encoded[end];
        if (byte === undefined || (byte & 0xc0) !== 0x80) {
          break;
        }
        end -= 1;
      }
    }
    if (end === offset) {
      end = Math.min(offset + MAX_WRITE_BYTES, encoded.byteLength);
    }
    chunks.push(decoder.decode(encoded.subarray(offset, end)));
    offset = end;
  }
  return chunks;
}
