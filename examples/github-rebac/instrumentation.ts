export async function register() {
  if (process.env.NEXT_RUNTIME === "nodejs") {
    const { getEngine } = await import("@/lib/engine");
    getEngine();
  }
}
