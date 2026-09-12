import assert from "node:assert/strict";
import { isScheduleDue, nextScheduledRun, scheduleStateFor } from "../src/schedule-model.ts";

const now = Date.parse("2026-09-12T10:00:00Z");
const minute = 60_000;
const none = { mode: "none", runAt: "", intervalMinutes: 60 };
const once = { mode: "once", runAt: "2026-09-12T09:30:00Z", intervalMinutes: 60 };
const interval = { mode: "interval", runAt: "", intervalMinutes: 30 };

const fresh = scheduleStateFor(none, null, now);
assert.equal(nextScheduledRun(none, fresh), null, "No schedule never runs");
assert.equal(isScheduleDue(none, fresh, now), false);

const onceState = scheduleStateFor(once, null, now);
assert.equal(nextScheduledRun(once, onceState), Date.parse(once.runAt), "One-off runs at its time");
assert.equal(isScheduleDue(once, onceState, now), true, "A past one-off time is due while the app is open");
assert.equal(nextScheduledRun(once, { ...onceState, lastRunMs: Date.parse(once.runAt) }), null, "A one-off never repeats");
assert.equal(nextScheduledRun({ ...once, runAt: "not a date" }, onceState), null, "Unparseable times never fire");

const intervalState = scheduleStateFor(interval, null, now);
assert.equal(nextScheduledRun(interval, intervalState), now + 30 * minute, "Intervals without a first run start one interval after being applied");
assert.equal(isScheduleDue(interval, intervalState, now + 29 * minute), false);
assert.equal(isScheduleDue(interval, intervalState, now + 30 * minute), true);
const ran = { ...intervalState, lastRunMs: now + 30 * minute };
assert.equal(nextScheduledRun(interval, ran), now + 60 * minute, "Intervals repeat from the last run");
const anchored = { ...interval, runAt: "2026-09-12T12:00:00Z" };
assert.equal(nextScheduledRun(anchored, scheduleStateFor(anchored, null, now)), Date.parse(anchored.runAt), "An interval with a first-run time starts there");
assert.equal(nextScheduledRun({ ...interval, intervalMinutes: 0 }, intervalState), now + minute, "Intervals are clamped to at least one minute");

assert.deepEqual(scheduleStateFor(interval, ran, now + 90 * minute), ran, "Saved progress survives for an unchanged schedule");
assert.equal(scheduleStateFor({ ...interval, intervalMinutes: 45 }, ran, now + 90 * minute).lastRunMs, 0, "Editing the schedule resets its progress");
assert.equal(scheduleStateFor(interval, { key: ran.key, anchorMs: Number.NaN, lastRunMs: 0 }, now).anchorMs, now, "Corrupt saved state is replaced");

console.log("Schedule model checks passed");
