// Scheduling runs inside the open desktop app: no background service exists, so a schedule
// only fires while the app is running and idle.

export type ScheduleMode = "none" | "once" | "interval";
export type ScheduleConfig = { mode: ScheduleMode; runAt: string; intervalMinutes: number };
export type ScheduleState = { key: string; anchorMs: number; lastRunMs: number };

export const scheduleKey = (schedule: ScheduleConfig) => JSON.stringify([schedule.mode, schedule.runAt, schedule.intervalMinutes]);

const parseRunAt = (value: string) => {
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : null;
};

// Reuse saved progress for the same schedule; a changed schedule starts from `now`.
export function scheduleStateFor(schedule: ScheduleConfig, saved: ScheduleState | null | undefined, now: number): ScheduleState {
  const key = scheduleKey(schedule);
  if (saved && saved.key === key && Number.isFinite(saved.anchorMs) && Number.isFinite(saved.lastRunMs)) return saved;
  return { key, anchorMs: now, lastRunMs: 0 };
}

// Timestamp of the next run, or null when nothing is pending.
export function nextScheduledRun(schedule: ScheduleConfig, state: ScheduleState): number | null {
  const runAt = parseRunAt(schedule.runAt);
  if (schedule.mode === "once") return runAt !== null && state.lastRunMs < runAt ? runAt : null;
  if (schedule.mode === "interval") {
    const interval = Math.max(1, Math.floor(schedule.intervalMinutes)) * 60_000;
    if (state.lastRunMs > 0) return state.lastRunMs + interval;
    return runAt ?? state.anchorMs + interval;
  }
  return null;
}

export const isScheduleDue = (schedule: ScheduleConfig, state: ScheduleState, now: number) => {
  const next = nextScheduledRun(schedule, state);
  return next !== null && next <= now;
};
