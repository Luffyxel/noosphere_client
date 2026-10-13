export function noteCloseHandler(
  flush: () => Promise<void>,
  close: () => Promise<unknown>,
  report: (error: unknown) => void,
) {
  let closing = false;
  return async (event: { preventDefault: () => void }) => {
    event.preventDefault();
    if (closing) return;
    closing = true;
    try {
      await flush();
      await close();
    } catch (error) {
      report(error);
    } finally {
      closing = false;
    }
  };
}
