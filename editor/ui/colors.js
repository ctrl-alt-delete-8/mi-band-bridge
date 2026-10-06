// The color rules the form shows, so a swatch can follow a change at once,
// with no round trip to Rust.
(function (root) {
  // The automatic pressed color: each channel half way to white. This is a
  // copy of `lighter` in src/layout.rs, which is what the band draws.
  // tests/preview.rs checks that the two still agree.
  function autoPressColor(hex) {
    const match = /^#([0-9a-fA-F]{6})$/.exec(String(hex).trim());
    if (!match) return hex;
    const half = (c) => c + Math.floor((255 - c) / 2);
    const channel = (i) => half(parseInt(match[1].slice(i, i + 2), 16));
    return "#" + [0, 2, 4].map((i) => channel(i).toString(16).padStart(2, "0")).join("");
  }

  // A typed color as the layout stores it, #rrggbb in lower case, or null
  // while it is not a whole color. The # may be left out.
  function readHex(text) {
    const match = /^#?([0-9a-fA-F]{6})$/.exec(String(text).trim());
    return match ? "#" + match[1].toLowerCase() : null;
  }

  if (typeof module !== "undefined" && module.exports) {
    module.exports = { autoPressColor, readHex };
  } else {
    root.BandColors = { autoPressColor, readHex };
  }
})(this);
