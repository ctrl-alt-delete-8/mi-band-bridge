// Where the connection stands, in the words of the pill and the card.
//
// Each state line from the band task carries facts: the state (connected,
// connecting, retrying or disconnected), the reason it began (started, you,
// dropped or failed), and `since`, the local time it began. This file words
// them, as the design frame "State variations — Connection" does. It touches
// no page, so gui/tests/connection_card.rs runs it under Node, and checks the
// words for each state and reason.

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
                "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

// A moment in local time, as the design writes it: "24 Sep 2026, 21:06".
// Empty for a time that cannot be read.
function when(text) {
  const at = new Date(text);
  if (Number.isNaN(at.getTime())) return "";
  const two = (n) => String(n).padStart(2, "0");
  return `${at.getDate()} ${MONTHS[at.getMonth()]} ${at.getFullYear()}, ` +
         `${two(at.getHours())}:${two(at.getMinutes())}`;
}

// The parts of a line, joined as the design joins them. An empty part is
// left out.
function joined(...parts) {
  return parts.filter(Boolean).join(" \u00b7 ");
}

// The battery's icon for its level. The design draws 74% half full.
function batteryIcon(battery) {
  if (battery.charging) return "battery-charging";
  if (battery.percent >= 80) return "battery-full";
  return battery.percent >= 30 ? "battery-medium" : "battery-low";
}

// What the pill and the card say for where the connection stands `s`, with
// the band's battery, or null while it is not known: the pill's words and
// tone, the line's icon, tone and words, the buttons, and Connect's label
// when it is not "Connect". The design draws five states. The three it does
// not draw follow its pattern: what happened, when, and what happens next.
function card(s, battery) {
  const at = when(s.since);
  if (s.state === "connected") {
    const since = at ? ` since ${at}` : "";
    return {
      pill: "Connected", pillTone: "good",
      icon: battery ? batteryIcon(battery) : "circle-check", tone: "dim",
      text: battery
        ? joined(`${battery.percent}%${battery.charging ? ", charging" : ""}`, `connected${since}`)
        : `Connected${since}`,
      buttons: ["disconnect"],
    };
  }
  if (s.state === "connecting") {
    // No moment has passed yet, so no time.
    return { pill: "Connecting", pillTone: "warn", icon: "loader", tone: "warn",
             text: "Reaching out\u2026", buttons: ["cancel"] };
  }
  if (s.state === "retrying") {
    // A band that was here and dropped is reconnecting. One that was never
    // reached is still connecting.
    const dropped = s.reason === "dropped";
    return {
      pill: dropped ? "Reconnecting" : "Connecting", pillTone: "warn",
      icon: "refresh-cw", tone: "warn",
      text: joined(dropped ? "Connection Dropped" : "Could not connect", at, "trying again"),
      buttons: ["stop-trying"],
    };
  }
  // Nothing tries until a person asks. Forget shows here and nowhere else:
  // only while the band is idle, as the design draws it. A band that is
  // connected, connecting or retrying is let go first.
  const idle = { pill: "Not connected", pillTone: "idle", buttons: ["connect", "forget"] };
  if (s.reason === "dropped") {
    return { ...idle, pill: "Connection lost", pillTone: "bad", icon: "triangle-alert",
             tone: "bad", text: joined("Connection Dropped", at), connect: "Reconnect" };
  }
  if (s.reason === "failed") {
    return { ...idle, icon: "triangle-alert", tone: "bad", text: joined("Could not connect", at) };
  }
  if (s.reason === "you") {
    return { ...idle, icon: "circle-slash", tone: "dim", text: joined("Disconnected", at) };
  }
  // BandBridge started, and had no way to reach the band: no moment to name.
  return { ...idle, icon: "circle-slash", tone: "dim", text: "Not connected" };
}
