// Run one generated page under Node, press every button, and print the result
// as JSON. tests/preview.rs calls this.
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";

const require = createRequire(import.meta.url);
const { runPage } = require("./preview.js");

const run = runPage(readFileSync(process.argv[2], "utf8"));
run.widgets.forEach((_, index) => run.press(index));
console.log(JSON.stringify({
  widgets: run.widgets.map((w) => ({
    type: w.type, x: w.props.x, y: w.props.y, w: w.props.w, h: w.props.h, text: w.props.text,
    text_size: w.props.text_size,
  })),
  sent: run.sent,
  unknown: run.unknown,
  errors: run.errors,
}));
