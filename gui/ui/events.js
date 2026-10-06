// The event log's rows, as the design frame "BandBridge Window — Light" draws
// them in its section "Events from the device".
//
// The band task sends a record for each press, and for each app installed or
// removed (Record, in src/event.rs). This file says what a record's row
// shows, and keeps count of which records the log holds, so the page knows
// whether a record is new, a result for a row it shows, or late for a row
// that is gone. It touches no page, so gui/tests/event_log.rs runs it under
// Node.

// A moment as the log shows it: "21:04:11", local time, on a 24-hour clock.
function clock(at) {
  const moment = new Date(at);
  if (Number.isNaN(moment.getTime())) return "--:--:--";
  const two = (n) => String(n).padStart(2, "0");
  return `${two(moment.getHours())}:${two(moment.getMinutes())}:${two(moment.getSeconds())}`;
}

// The chip for how an action went: its words, its tone and its mark, and a
// tooltip with the words in full, for a chip too narrow to hold them. The
// words are the ones a terminal prints: the HTTP status, the exit code,
// done, or the error. The band task says whether it worked (`ok`), so the
// page never needs a rule for which codes are good.
function resultChip(result) {
  let words;
  if (result.type === "status") words = String(result.code);
  else if (result.type === "exit") words = [`exit ${result.code}`, result.error].filter(Boolean).join(": ");
  else if (result.type === "done") words = "done";
  else words = result.error || "failed";
  const ok = result.ok === true;
  return {
    words,
    tone: ok ? "good" : "bad",
    mark: ok ? "check" : "x",
    title: result.type === "status" ? `HTTP ${result.code}` : words,
  };
}

// Whether a result says more than that the action worked, and so has a
// chip, as the design draws it: a web request's HTTP status always does,
// and so does any failure. A plain success, done or exit 0, does not.
function tellsMore(result) {
  return result.type === "status" || result.ok !== true;
}

// What the row for `record` shows, or null for a kind this page does not
// know. Every row has the time and the kind's tag. A press has its signal,
// then the name of the action it fired, after an arrow, and, once the
// action ends, the result's chip when the result tells more than that it
// worked. Until then it is running. A press that no action answers says
// so, with no arrow and no chip. An app row says what happened to the app,
// and the app's name.
function eventRow(record) {
  const row = { id: record.id, kind: record.kind, at: record.at, time: clock(record.at) };
  if (record.kind === "press") {
    const fired = typeof record.action === "string" && record.action !== "";
    return {
      ...row,
      tag: "press",
      signal: record.signal,
      action: fired ? record.action : null,
      unset: fired ? null : "no action set",
      running: fired && !record.result,
      result: fired && record.result && tellsMore(record.result) ? resultChip(record.result) : null,
    };
  }
  if (record.kind === "app") {
    return { ...row, tag: "app", change: record.change, app: record.app };
  }
  return null;
}

// Which records the log shows, by id, newest first, and which it no longer
// shows. Ids count up from 1 while the app runs, and a record comes a second
// time, under the same id, when the action it fired ends.
//
// `take(id)` says what to do with a record: "replace" the row that has its
// id, "add" a row for it above the row with the id `before` (at the end when
// that is null), or "drop" it. A record whose row is gone is dropped: Clear
// took the row away, or the row left a full log, which keeps its newest
// rows. Whatever `trim` names is the oldest row, to take away because the
// log is full.
function eventLedger(limit) {
  let shown = [];
  // Every id up to this one that is not shown, Clear took away.
  let clearedThrough = 0;
  return {
    take(id) {
      if (shown.includes(id)) return { act: "replace", before: null, trim: [] };
      if (id <= clearedThrough) return { act: "drop", before: null, trim: [] };
      let at = shown.findIndex((other) => other < id);
      if (at === -1) at = shown.length;
      // Older than every row of a full log, as the late result of a row
      // that left it is.
      if (at >= limit) return { act: "drop", before: null, trim: [] };
      const before = at < shown.length ? shown[at] : null;
      shown.splice(at, 0, id);
      return { act: "add", before, trim: shown.splice(limit) };
    },
    // Clear: every row goes, and a result that comes for one of them later
    // is dropped, not shown as a new row.
    clear() {
      for (const id of shown) clearedThrough = Math.max(clearedThrough, id);
      shown = [];
    },
    shown: () => [...shown],
  };
}
