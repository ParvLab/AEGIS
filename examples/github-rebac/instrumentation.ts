export async function register() {
  const { getEngine } = await import("@/lib/engine");
  getEngine();
}
