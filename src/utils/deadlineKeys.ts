import type { PaperDeadlineInfo } from "../types/config";

export function deadlineInstanceKey(deadline: Pick<PaperDeadlineInfo, "title" | "year" | "deadline_utc">): string {
  return [
    deadline.title.trim().toLowerCase(),
    deadline.year.trim().toLowerCase(),
    deadline.deadline_utc.trim(),
  ].join("|");
}

export function deadlineTitleEquals(a: string, b: string): boolean {
  return a.trim().toLowerCase() === b.trim().toLowerCase();
}
