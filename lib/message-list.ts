import type { Message } from '@/lib/desktop-bridge';

export type DisplayMessage = Message & {
  renderKey: string;
  pending?: boolean;
};

export function mergeMessages(
  current: DisplayMessage[],
  synchronized: Message[],
): DisplayMessage[] {
  const currentById = new Map(current.map((message) => [message.id, message]));
  const synchronizedIds = new Set(synchronized.map((message) => message.id));
  return [
    ...synchronized.map((message) => ({
      ...message,
      renderKey: currentById.get(message.id)?.renderKey ?? message.id,
    })),
    ...current.filter(
      (message) => message.pending && !synchronizedIds.has(message.id),
    ),
  ].sort(
    (left, right) =>
      left.sentAt.localeCompare(right.sentAt) ||
      left.id.localeCompare(right.id),
  );
}
