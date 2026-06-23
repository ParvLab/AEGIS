export function handleEngineError(err: any) {
  const msg = err?.message ?? "Unknown error";
  // Extract structured error code from engine message prefix
  const code = msg.match(/^[A-Z][a-zA-Z]+Error/)?.[0] 
    ?? msg.match(/engine panic: ([A-Za-z]+Error)/)?.[1]
    ?? msg.match(/([A-Za-z]+Error):/)?.[1]
    ?? "UnknownError";
  return { error: msg, code };
}
